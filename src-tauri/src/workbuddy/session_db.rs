//! WorkBuddy 会话库访问层：~/.workbuddy/workbuddy.db（明文 SQLite，表 sessions）。
//!
//! 所有 SQL 参数化，禁止字符串拼接 id/uid。会话 id 走 UUID 白名单校验。

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// sessions 表复制时涉及的 21 列（与 WorkDaddy SESSION_COPY_COLUMNS 对齐）。
pub const SESSION_COPY_COLUMNS: &[&str] = &[
    "id",
    "cwd",
    "user_id",
    "title",
    "custom_title",
    "status",
    "created_at",
    "updated_at",
    "last_activity_at",
    "is_playground",
    "source_mode",
    "is_background_automation",
    "mode",
    "model",
    "expert_id",
    "expert_locale",
    "expert_runtime_identity",
    "expert_marketplace",
    "permission_mode",
    "use_sandbox_cli",
    "project_id",
];

/// 单条会话记录（对应 sessions 表 21 列）。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionRecord {
    pub id: String,
    pub cwd: String,
    pub user_id: String,
    pub title: String,
    pub custom_title: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_activity_at: i64,
    pub is_playground: i64,
    pub source_mode: Option<String>,
    pub is_background_automation: Option<i64>,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub expert_id: Option<String>,
    pub expert_locale: Option<String>,
    pub expert_runtime_identity: Option<String>,
    pub expert_marketplace: Option<String>,
    pub permission_mode: Option<String>,
    pub use_sandbox_cli: Option<i64>,
    pub project_id: Option<String>,
}

/// WorkBuddy 数据根目录：~/.workbuddy
pub fn workbuddy_data_root() -> PathBuf {
    dirs_home().join(".workbuddy")
}

/// 会话库路径：~/.workbuddy/workbuddy.db
pub fn session_db_path() -> PathBuf {
    workbuddy_data_root().join("workbuddy.db")
}

fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 校验会话 id 为合法 UUID（防注入/防路径异常）。
pub fn is_valid_session_id(id: &str) -> bool {
    if id.len() != 36 {
        return false;
    }
    let bytes = id.as_bytes();
    let dash_pos = [8, 13, 18, 23];
    for (i, &b) in bytes.iter().enumerate() {
        if dash_pos.contains(&i) {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

pub struct WbSessionDb {
    conn: Connection,
}

impl WbSessionDb {
    /// 打开会话库（读写模式）。文件不存在时返回错误（不自动创建，避免误建空库）。
    pub fn open() -> Result<Self, String> {
        let path = session_db_path();
        if !path.exists() {
            return Err(format!(
                "未找到 WorkBuddy 会话库：{}（请先运行过 WorkBuddy 客户端）",
                path.display()
            ));
        }
        let conn = Connection::open(&path).map_err(|e| format!("打开会话库失败: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置 busy_timeout 失败: {e}"))?;
        Ok(Self { conn })
    }

    /// 以只读模式打开（用于浏览列表）。
    pub fn open_readonly() -> Result<Self, String> {
        let path = session_db_path();
        if !path.exists() {
            return Err(format!("未找到 WorkBuddy 会话库：{}", path.display()));
        }
        let conn = Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("打开会话库(只读)失败: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("设置 busy_timeout 失败: {e}"))?;
        Ok(Self { conn })
    }

    /// 列出指定账号的未删除会话，按 created_at 倒序。
    pub fn list_sessions(&self, uid: &str) -> Result<Vec<SessionRecord>, String> {
        let cols = SESSION_COPY_COLUMNS.join(",");
        let sql = format!(
            "SELECT {} FROM sessions WHERE deleted_at IS NULL AND user_id = ? ORDER BY created_at DESC;",
            cols
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| format!("准备查询失败: {e}"))?;
        let rows = stmt
            .query_map([uid], row_to_session)
            .map_err(|e| format!("查询会话失败: {e}"))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| format!("读取会话行失败: {e}"))?);
        }
        Ok(out)
    }

    /// 按 id 取单条会话。
    pub fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, String> {
        if !is_valid_session_id(id) {
            return Err("无效的会话 ID".into());
        }
        let cols = SESSION_COPY_COLUMNS.join(",");
        let sql = format!(
            "SELECT {} FROM sessions WHERE id = ? AND deleted_at IS NULL LIMIT 1;",
            cols
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| format!("准备查询失败: {e}"))?;
        stmt.query_row([id], row_to_session)
            .optional()
            .map_err(|e| format!("查询会话失败: {e}"))
    }

    /// 复制一条会话记录到目标账号，使用新 id。在事务中调用。
    pub fn insert_copied(
        &self,
        src: &SessionRecord,
        target_uid: &str,
        new_id: &str,
    ) -> Result<(), String> {
        if !is_valid_session_id(new_id) {
            return Err("无效的新会话 ID".into());
        }
        let cols = SESSION_COPY_COLUMNS.join(",");
        let placeholders = SESSION_COPY_COLUMNS.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("INSERT INTO sessions ({}) VALUES ({});", cols, placeholders);
        let now = chrono::Utc::now().timestamp_millis();
        self.conn
            .execute(
                &sql,
                rusqlite::params![
                    new_id,
                    src.cwd,
                    target_uid,
                    src.title,
                    src.custom_title,
                    src.status,
                    src.created_at,
                    now,
                    src.last_activity_at,
                    src.is_playground,
                    src.source_mode,
                    src.is_background_automation,
                    src.mode,
                    src.model,
                    src.expert_id,
                    src.expert_locale,
                    src.expert_runtime_identity,
                    src.expert_marketplace,
                    src.permission_mode,
                    src.use_sandbox_cli,
                    src.project_id,
                ],
            )
            .map_err(|e| format!("插入会话记录失败: {e}"))?;
        Ok(())
    }

    /// 删除指定会话记录（回滚用）。
    pub fn delete_session(&self, id: &str) -> Result<(), String> {
        if !is_valid_session_id(id) {
            return Err("无效的会话 ID".into());
        }
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?;", [id])
            .map_err(|e| format!("删除会话记录失败: {e}"))?;
        Ok(())
    }

    /// 执行事务闭包。
    pub fn transaction<F, T>(&mut self, f: F) -> Result<T, String>
    where
        F: FnOnce(&Connection) -> Result<T, String>,
    {
        let tx = self.conn.transaction().map_err(|e| format!("开启事务失败: {e}"))?;
        let result = f(&tx)?;
        tx.commit().map_err(|e| format!("提交事务失败: {e}"))?;
        Ok(result)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// 从 rusqlite 行映射到 SessionRecord（列顺序与 SESSION_COPY_COLUMNS 一致）。
fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        cwd: row.get(1)?,
        user_id: row.get(2)?,
        title: row.get(3)?,
        custom_title: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        last_activity_at: row.get(8)?,
        is_playground: row.get(9)?,
        source_mode: row.get(10)?,
        is_background_automation: row.get(11)?,
        mode: row.get(12)?,
        model: row.get(13)?,
        expert_id: row.get(14)?,
        expert_locale: row.get(15)?,
        expert_runtime_identity: row.get(16)?,
        expert_marketplace: row.get(17)?,
        permission_mode: row.get(18)?,
        use_sandbox_cli: row.get(19)?,
        project_id: row.get(20)?,
    })
}

/// 供前端展示的会话视图（附加文件数/大小，由 session_files 计算后填充）。
#[derive(Serialize, Clone)]
pub struct SessionView {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub custom_title: Option<String>,
    pub cwd: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_activity_at: i64,
    pub mode: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub file_count: u64,
    #[serde(default)]
    pub size_bytes: u64,
}

impl From<&SessionRecord> for SessionView {
    fn from(r: &SessionRecord) -> Self {
        SessionView {
            id: r.id.clone(),
            user_id: r.user_id.clone(),
            title: r.title.clone(),
            custom_title: r.custom_title.clone(),
            cwd: r.cwd.clone(),
            status: r.status.clone(),
            created_at: r.created_at,
            updated_at: r.updated_at,
            last_activity_at: r.last_activity_at,
            mode: r.mode.clone(),
            model: r.model.clone(),
            file_count: 0,
            size_bytes: 0,
        }
    }
}

/// 数据根目录（供 session_files 使用）。
pub fn data_root() -> &'static Path {
    // 每次调用都重新计算；PathBuf 不能是 static，这里用函数返回
    // session_files 直接调用 workbuddy_data_root()
    Box::leak(Box::new(workbuddy_data_root()))
}
