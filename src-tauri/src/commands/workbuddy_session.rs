//! WorkBuddy 会话 / 客户端切换相关 Tauri 命令。
//!
//! M1：客户端登录态切换（auth 文件备份 + 原子写回 + 进程重启）
//! M2：会话迁移（session_db / session_files / session_lineage / session_jobs）
//! M3：.wds 加密归档导入导出

use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::state::AppState;
use crate::workbuddy::auth_file;
use crate::workbuddy::session_db::{self, SessionView};
use crate::workbuddy::session_files;
use crate::workbuddy::session_jobs;
use crate::workbuddy::session_transfer;

// ==================== M1：客户端账号切换 ====================

#[derive(Serialize, Clone)]
pub struct WbSwitchResult {
    pub uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    pub reloaded: bool,
    pub hint: String,
}

#[derive(Deserialize)]
pub struct WbSwitchOpts {
    /// 切换后是否自动重启 WorkBuddy 客户端（默认 true）
    #[serde(default = "default_true")]
    pub reload: bool,
    /// 会话跟随策略："auto" | "ask" | "off"（M1 仅占位，M2 实现）
    #[serde(default = "default_migrate_off")]
    pub migrate: String,
}

fn default_true() -> bool {
    true
}
fn default_migrate_off() -> String {
    "off".to_string()
}

/// 列出已备份的 WorkBuddy 客户端登录文件。
#[tauri::command]
pub fn wb_auth_list() -> Vec<auth_file::StoredAuthMeta> {
    auth_file::list_stored_auth()
}

/// 获取当前 WorkBuddy 客户端登录的账号信息（只读，不创建备份）。
#[tauri::command]
pub fn wb_auth_current() -> Option<auth_file::WbAuthInfo> {
    auth_file::read_current_auth_info()
}

/// 把当前 WorkBuddy 客户端的登录文件备份到本应用数据目录。
#[tauri::command]
pub fn wb_auth_backup_current(state: State<AppState>) -> Result<auth_file::WbAuthInfo, String> {
    let info = auth_file::backup_current_auth()?;
    fs_utils::app_log(
        &state.data_dir,
        &format!("WorkBuddy 客户端登录已备份: uid={}", info.uid),
    );
    Ok(info)
}

/// 切换 WorkBuddy 客户端登录到指定账号：原子写回登录文件 + 可选重启客户端。
#[tauri::command(async)]
pub async fn wb_switch_account(
    app: AppHandle,
    state: State<'_, AppState>,
    uid: String,
    opts: Option<WbSwitchOpts>,
) -> Result<WbSwitchResult, String> {
    let opts = opts.unwrap_or(WbSwitchOpts {
        reload: true,
        migrate: "off".to_string(),
    });

    // 记录切换前的当前账号（M2 会话迁移会用到 sourceUid）
    let source_uid = auth_file::read_current_auth_info().map(|a| a.uid);

    // 1. 原子写回登录文件
    let info = auth_file::switch_auth_to(&uid)?;
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "WorkBuddy 客户端切换登录: {} -> {}",
            source_uid.as_deref().unwrap_or("-"),
            uid
        ),
    );

    let mut reloaded = false;
    let mut hint = "登录文件已切换，请重启 WorkBuddy 使新账号生效".to_string();

    // 2. 可选重启 WorkBuddy 客户端
    if opts.reload {
        match restart_workbuddy() {
            Ok(_) => {
                reloaded = true;
                hint = "已切换并重启 WorkBuddy 客户端".to_string();
            }
            Err(e) => {
                hint = format!("登录文件已切换，但重启客户端失败（{}），请手动重启 WorkBuddy", e);
            }
        }
    }

    // 3. 若开启会话跟随，启动自动复制队列（异步，不阻塞切换响应）
    let mut job_id: Option<String> = None;
    if opts.migrate == "auto" {
        if let Some(src) = source_uid.as_deref() {
            if src != uid {
                match session_jobs::start_job(app.clone(), src.to_string(), uid.clone()) {
                    Ok(job) => {
                        let jid = job.id.clone();
                        job_id = Some(jid.clone());
                        fs_utils::app_log(
                            &state.data_dir,
                            &format!("WorkBuddy 会话迁移已启动: {} -> {} (job={})", src, uid, jid),
                        );
                    }
                    Err(e) => {
                        fs_utils::app_log(
                            &state.data_dir,
                            &format!("WorkBuddy 会话迁移启动失败: {}", e),
                        );
                    }
                }
            }
        }
    }

    // 4. emit 切换完成事件
    let _ = app.emit(
        "wb-switch-done",
        serde_json::json!({
            "success": true,
            "uid": uid,
            "nickname": info.nickname,
            "sourceUid": source_uid,
            "reloaded": reloaded,
            "jobId": job_id,
        }),
    );

    Ok(WbSwitchResult {
        uid: info.uid,
        nickname: info.nickname,
        reloaded,
        hint,
    })
}

// ==================== WorkBuddy 进程管理 ====================

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 结束所有 WorkBuddy 进程，然后重新启动。
fn restart_workbuddy() -> Result<(), String> {
    kill_workbuddy_processes();
    // 等待进程完全退出
    std::thread::sleep(std::time::Duration::from_millis(800));
    launch_workbuddy()
}

/// 结束 WorkBuddy.exe 进程树（taskkill，进程不存在也视为成功）。
fn kill_workbuddy_processes() {
    for exe in ["WorkBuddy.exe", "WorkBuddyAI.exe"] {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", exe, "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}

/// 探测并启动 WorkBuddy 客户端。
fn launch_workbuddy() -> Result<(), String> {
    let candidates = workbuddy_exe_candidates();
    for p in &candidates {
        if p.exists() {
            std::process::Command::new(p)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("启动 WorkBuddy 失败: {e}"))?;
            return Ok(());
        }
    }
    Err("未找到 WorkBuddy 安装路径，请手动启动客户端".into())
}

/// WorkBuddy 可执行文件候选路径（国内版优先，国际版次之）。
fn workbuddy_exe_candidates() -> Vec<PathBuf> {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    vec![
        local.join("Programs").join("WorkBuddy").join("WorkBuddy.exe"),
        local.join("Programs").join("WorkBuddyAI").join("WorkBuddyAI.exe"),
    ]
}

// ==================== M2：会话迁移 ====================

/// 列出指定账号（默认当前登录账号）的会话列表。
#[tauri::command]
pub fn wb_session_list(uid: Option<String>) -> Result<Vec<SessionView>, String> {
    let target_uid = match uid {
        Some(u) if !u.is_empty() => u,
        _ => auth_file::read_current_auth_info()
            .map(|a| a.uid)
            .ok_or("未检测到当前登录的 WorkBuddy 账号，请指定 uid")?,
    };
    let db = session_db::WbSessionDb::open_readonly()?;
    let sessions = db.list_sessions(&target_uid)?;
    let views: Vec<SessionView> = sessions
        .iter()
        .map(|s| {
            let mut v = SessionView::from(s);
            let (count, size) = session_files::session_file_stats(&s.id);
            v.file_count = count;
            v.size_bytes = size;
            v
        })
        .collect();
    Ok(views)
}

/// 把指定会话复制到目标账号（单条手动迁移）。
#[tauri::command(async)]
pub async fn wb_session_copy(
    state: State<'_, AppState>,
    source_uid: String,
    session_id: String,
    target_uid: String,
) -> Result<serde_json::Value, String> {
    if source_uid == target_uid {
        return Err("源账号与目标账号相同".into());
    }
    let (new_id, status, failed) =
        session_jobs::copy_single_session(&source_uid, &session_id, &target_uid)?;
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "WorkBuddy 会话复制: {}({}) -> {} => {} [{}]",
            source_uid, session_id, target_uid, new_id, status
        ),
    );
    Ok(serde_json::json!({
        "sourceId": session_id,
        "newId": new_id,
        "status": status,
        "failedFiles": failed,
    }))
}

/// 启动批量迁移：把源账号的所有会话复制到目标账号（异步队列）。
#[tauri::command]
pub fn wb_session_migrate_all(
    app: AppHandle,
    state: State<AppState>,
    source_uid: String,
    target_uid: String,
) -> Result<session_jobs::SessionJob, String> {
    if source_uid == target_uid {
        return Err("源账号与目标账号相同".into());
    }
    let job = session_jobs::start_job(app, source_uid.clone(), target_uid.clone())?;
    fs_utils::app_log(
        &state.data_dir,
        &format!("WorkBuddy 批量迁移已启动: {} -> {} (job={})", source_uid, target_uid, job.id),
    );
    Ok(job)
}

/// 查询最近一次迁移任务状态。
#[tauri::command]
pub fn wb_session_job_status() -> Option<session_jobs::SessionJob> {
    session_jobs::last_job()
}

/// 清理无效的 lineage 记录（会话文件已不存在的成员）。
#[tauri::command]
pub fn wb_session_lineage_normalize(state: State<AppState>) -> Result<usize, String> {
    let removed = crate::workbuddy::session_lineage::normalize()?;
    fs_utils::app_log(&state.data_dir, &format!("WorkBuddy lineage 清理: 移除 {} 个无效成员", removed));
    Ok(removed)
}

// ==================== M3：.wds 加密归档 ====================

/// 导出指定会话为 .wds 加密归档文件，返回文件路径。
#[tauri::command(async)]
pub async fn wb_session_export(
    state: State<'_, AppState>,
    session_id: String,
    password: String,
) -> Result<String, String> {
    let output_dir = state.data_dir.join("exports");
    let path = session_transfer::export_session(&session_id, &password, &output_dir)?;
    fs_utils::app_log(
        &state.data_dir,
        &format!("WorkBuddy 会话已导出: {} -> {}", session_id, path.display()),
    );
    Ok(path.to_string_lossy().to_string())
}

/// 导入 .wds 归档到当前账号，返回新会话 id。
#[tauri::command(async)]
pub async fn wb_session_import(
    state: State<'_, AppState>,
    wds_path: String,
    password: String,
) -> Result<String, String> {
    let current = auth_file::read_current_auth_info()
        .ok_or("未检测到当前登录的 WorkBuddy 账号")?;
    let path = std::path::PathBuf::from(&wds_path);
    if !path.exists() {
        return Err(format!("文件不存在: {}", wds_path));
    }
    let new_id = session_transfer::import_session(&path, &password, &current.uid)?;
    fs_utils::app_log(
        &state.data_dir,
        &format!("WorkBuddy 会话已导入: {} -> {}", wds_path, new_id),
    );
    Ok(new_id)
}
