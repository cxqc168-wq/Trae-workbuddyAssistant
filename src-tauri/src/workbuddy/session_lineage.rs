//! WorkBuddy 会话跨账号谱系（lineage）：同一对话在不同账号下的副本集合。
//!
//! 持久化到 data/wb_session_lineage.json。切号时凭 lineage 找到对方已有副本，
//! 只更新文件不再 INSERT，根治重复会话。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::session_db::{is_valid_session_id, workbuddy_data_root};
use super::session_files;

#[derive(Serialize, Deserialize, Clone)]
pub struct LineageMember {
    pub uid: String,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LineageMapping {
    pub target_id: String,
    pub status: String, // "copied" | "partial" | "skipped"
    #[serde(default)]
    pub failed_files: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Lineage {
    pub created_at: i64,
    pub members: Vec<LineageMember>,
    #[serde(default)]
    pub mapping: HashMap<String, LineageMapping>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct LineageStore {
    pub version: u32,
    #[serde(default)]
    pub lineages: HashMap<String, Lineage>,
}

fn store_path() -> PathBuf {
    super::store_path().join("wb_session_lineage.json")
}

pub fn load() -> LineageStore {
    let p = store_path();
    if !p.exists() {
        return LineageStore {
            version: 1,
            lineages: HashMap::new(),
        };
    }
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<LineageStore>(&t).ok())
        .unwrap_or_default()
}

pub fn save(store: &LineageStore) -> Result<(), String> {
    let p = store_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let content = serde_json::to_vec_pretty(store).map_err(|e| format!("序列化失败: {e}"))?;
    // 原子写
    let tmp = p.with_extension(format!("json.tmp.{}", std::process::id()));
    std::fs::write(&tmp, content).map_err(|e| format!("写入失败: {e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("原子替换失败: {e}")
    })?;
    Ok(())
}

/// 为源会话确保存在 lineage，返回 lineageId。
pub fn ensure_lineage(source_uid: &str, session_id: &str) -> Result<String, String> {
    if !is_valid_session_id(session_id) {
        return Err("无效的会话 ID".into());
    }
    let mut store = load();
    // 已存在则返回
    for (lid, lineage) in &store.lineages {
        if lineage
            .members
            .iter()
            .any(|m| m.uid == source_uid && m.session_id == session_id)
        {
            return Ok(lid.clone());
        }
    }
    let lid = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    store.lineages.insert(
        lid.clone(),
        Lineage {
            created_at: now,
            members: vec![LineageMember {
                uid: source_uid.to_string(),
                session_id: session_id.to_string(),
            }],
            mapping: HashMap::new(),
        },
    );
    save(&store)?;
    Ok(lid)
}

/// 添加成员到 lineage（幂等）。
pub fn add_member(lineage_id: &str, uid: &str, session_id: &str) -> Result<(), String> {
    if !is_valid_session_id(session_id) {
        return Err("无效的会话 ID".into());
    }
    let mut store = load();
    if let Some(lineage) = store.lineages.get_mut(lineage_id) {
        if !lineage
            .members
            .iter()
            .any(|m| m.uid == uid && m.session_id == session_id)
        {
            lineage.members.push(LineageMember {
                uid: uid.to_string(),
                session_id: session_id.to_string(),
            });
            save(&store)?;
        }
    }
    Ok(())
}

/// 获取 lineage 所有成员。
pub fn get_members(lineage_id: &str) -> Vec<LineageMember> {
    load()
        .lineages
        .get(lineage_id)
        .map(|l| l.members.clone())
        .unwrap_or_default()
}

/// 获取指定 uid 的映射（目标会话 id）。
pub fn get_mapping(lineage_id: &str, uid: &str) -> Option<LineageMapping> {
    load()
        .lineages
        .get(lineage_id)
        .and_then(|l| l.mapping.get(uid).cloned())
}

/// 设置映射。
pub fn set_mapping(
    lineage_id: &str,
    uid: &str,
    mapping: LineageMapping,
) -> Result<(), String> {
    let mut store = load();
    if let Some(lineage) = store.lineages.get_mut(lineage_id) {
        lineage.mapping.insert(uid.to_string(), mapping);
        save(&store)?;
    }
    Ok(())
}

/// 选出 lineage 中文件内容 mtime 最新的成员作为同步源。
/// 比较各成员 projects/**/<id>.jsonl 的最大 mtime。
pub fn pick_latest_member(lineage_id: &str) -> Option<LineageMember> {
    let members = get_members(lineage_id);
    if members.is_empty() {
        return None;
    }
    let root = workbuddy_data_root();
    let mut best: Option<(LineageMember, i64)> = None;
    for m in &members {
        let mut latest = 0i64;
        // 扫描 projects 下该会话的 jsonl
        if let Ok(entries) = std::fs::read_dir(root.join("projects")) {
            for entry in entries.flatten() {
                let jsonl = entry.path().join(format!("{}.jsonl", m.session_id));
                if let Ok(meta) = std::fs::metadata(&jsonl) {
                    if let Ok(t) = meta.modified() {
                        if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                            let ms = d.as_millis() as i64;
                            if ms > latest {
                                latest = ms;
                            }
                        }
                    }
                }
            }
        }
        match &best {
            None => best = Some((m.clone(), latest)),
            Some((_, t)) if latest > *t => best = Some((m.clone(), latest)),
            _ => {}
        }
    }
    best.map(|(m, _)| m)
}

/// 清理无效成员（会话文件已不存在），返回清理的数量。
pub fn normalize() -> Result<usize, String> {
    let mut store = load();
    let mut removed = 0usize;
    let root = workbuddy_data_root();
    for lineage in store.lineages.values_mut() {
        lineage.members.retain(|m| {
            // 只要 projects 下存在该会话的 jsonl 或目录，就视为有效
            let mut exists = false;
            if let Ok(entries) = std::fs::read_dir(root.join("projects")) {
                for entry in entries.flatten() {
                    if entry.path().join(format!("{}.jsonl", m.session_id)).exists()
                        || entry.path().join(&m.session_id).exists()
                    {
                        exists = true;
                        break;
                    }
                }
            }
            if !exists {
                removed += 1;
            }
            exists
        });
    }
    // 移除空 lineage
    store.lineages.retain(|_, l| !l.members.is_empty());
    save(&store)?;
    Ok(removed)
}

/// 检查指定 uid 的指定会话是否属于某个 lineage，返回 lineageId。
pub fn find_lineage(uid: &str, session_id: &str) -> Option<String> {
    let store = load();
    for (lid, lineage) in &store.lineages {
        if lineage
            .members
            .iter()
            .any(|m| m.uid == uid && m.session_id == session_id)
        {
            return Some(lid.clone());
        }
    }
    None
}

/// 单会话迁移：复制记录 + 文件，处理 lineage。
/// 返回 (新会话 id, 状态, 失败文件数)。
pub fn copy_session(
    db: &super::session_db::WbSessionDb,
    src: &super::session_db::SessionRecord,
    target_uid: &str,
    lineage_id: Option<&str>,
) -> Result<(String, String, u64), String> {
    let new_id = uuid::Uuid::new_v4().to_string();

    // 1. 插入数据库记录
    db.insert_copied(src, target_uid, &new_id).map_err(|e| {
        // 回滚
        let _ = db.delete_session(&new_id);
        e
    })?;

    // 2. 复制文件
    let report = match session_files::copy_remap(&src.id, &new_id, false) {
        Ok(r) => r,
        Err(e) => {
            // 文件复制失败，回滚数据库
            let _ = db.delete_session(&new_id);
            return Err(e);
        }
    };

    // 3. 处理 lineage
    if let Some(lid) = lineage_id {
        let _ = add_member(lid, target_uid, &new_id);
        let _ = set_mapping(
            lid,
            target_uid,
            LineageMapping {
                target_id: new_id.clone(),
                status: if report.failed > 0 {
                    "partial".into()
                } else {
                    "copied".into()
                },
                failed_files: report.failed,
            },
        );
    }

    let status = if report.failed > 0 { "partial" } else { "copied" };
    Ok((new_id, status.to_string(), report.failed))
}
