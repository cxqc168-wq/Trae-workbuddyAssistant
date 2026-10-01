//! WorkBuddy 会话关联文件的收集、复制、路径 remap、删除。
//!
//! 5 类文件（相对 dataRoot = ~/.workbuddy）：
//!   projects/<hash>/<id>.jsonl          消息正文
//!   projects/<hash>/<id>/...            会话附属目录
//!   workspace/sessions/<id>/...         工作区会话数据
//!   tasks/<id>/...                      任务数据
//!   file-history/<id>/...               文件历史
//!   artifact-index/<id>.json            产物索引（需改 _meta.ownerConversationId）

use serde::Serialize;
use std::path::{Path, PathBuf};

use super::session_db::{is_valid_session_id, workbuddy_data_root};

#[derive(Serialize, Clone)]
pub struct SessionFile {
    /// 相对 dataRoot 的路径
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct CopyReport {
    pub copied: u64,
    pub failed: u64,
    pub failed_files: Vec<String>,
}

/// 收集指定会话的所有关联文件（拒绝符号链接）。
pub fn collect(session_id: &str) -> Result<Vec<SessionFile>, String> {
    if !is_valid_session_id(session_id) {
        return Err("无效的会话 ID".into());
    }
    let root = workbuddy_data_root();
    let mut files = Vec::new();

    // projects/<hash>/<id>.jsonl 和 projects/<hash>/<id>/
    let projects = root.join("projects");
    if let Ok(entries) = std::fs::read_dir(&projects) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let project_root = entry.path();
            collect_file(&project_root.join(format!("{}.jsonl", session_id)), &root, &mut files);
            collect_dir(&project_root.join(session_id), &root, &mut files);
        }
    }

    collect_dir(&root.join("workspace").join("sessions").join(session_id), &root, &mut files);
    collect_dir(&root.join("tasks").join(session_id), &root, &mut files);
    collect_dir(&root.join("file-history").join(session_id), &root, &mut files);
    collect_file(
        &root.join("artifact-index").join(format!("{}.json", session_id)),
        &root,
        &mut files,
    );

    Ok(files)
}

fn collect_file(target: &Path, root: &Path, files: &mut Vec<SessionFile>) {
    if let Ok(stat) = std::fs::symlink_metadata(target) {
        if stat.is_file() && !stat.file_type().is_symlink() {
            if let Ok(rel) = target.strip_prefix(root) {
                files.push(SessionFile {
                    path: rel.to_string_lossy().to_string(),
                    size: stat.len(),
                });
            }
        }
    }
}

fn collect_dir(target: &Path, root: &Path, files: &mut Vec<SessionFile>) {
    if let Ok(stat) = std::fs::symlink_metadata(target) {
        if !stat.is_dir() || stat.file_type().is_symlink() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Ok(ft) = entry.file_type() {
                    if ft.is_symlink() {
                        continue;
                    }
                    if ft.is_dir() {
                        collect_dir(&p, root, files);
                    } else if ft.is_file() {
                        if let Ok(rel) = p.strip_prefix(root) {
                            files.push(SessionFile {
                                path: rel.to_string_lossy().to_string(),
                                size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// 把相对路径中的 old_id 精确替换为 new_id（只在规定路径段替换，避免误伤同名子串）。
pub fn remap_rel_path(rel: &str, old_id: &str, new_id: &str) -> Result<String, String> {
    if !is_valid_session_id(old_id) || !is_valid_session_id(new_id) {
        return Err("无效的会话 ID".into());
    }
    let parts: Vec<&str> = rel.split('/').collect();
    let mut out = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let mut replaced = part.to_string();
        // projects/<hash>/<id>.jsonl
        if i == 2 && parts.len() >= 3 && parts[0] == "projects" {
            if *part == format!("{}.jsonl", old_id) {
                replaced = format!("{}.jsonl", new_id);
            }
        }
        // projects/<hash>/<id>/...  或 workspace/sessions/<id>/... 或 tasks/<id>/... 或 file-history/<id>/...
        if (parts[0] == "projects" && i == 2)
            || (parts[0] == "workspace" && i == 2)
            || (parts[0] == "tasks" && i == 1)
            || (parts[0] == "file-history" && i == 1)
        {
            if *part == old_id {
                replaced = new_id.to_string();
            }
        }
        // artifact-index/<id>.json
        if parts[0] == "artifact-index" && i == 1 {
            if *part == format!("{}.json", old_id) {
                replaced = format!("{}.json", new_id);
            }
        }
        out.push(replaced);
    }
    let result = out.join("/");
    // 安全校验：结果不含 ..、绝对路径、盘符
    if result.contains("..") || result.starts_with('/') || result.contains(':') {
        return Err("路径 remap 后包含非法字符".into());
    }
    Ok(result)
}

/// 复制会话文件：old_id 路径 → new_id 路径，artifact-index 同步改 ownerConversationId。
/// overwrite=true 时覆盖已有文件（lineage 更新场景），否则跳过已存在。
pub fn copy_remap(old_id: &str, new_id: &str, overwrite: bool) -> Result<CopyReport, String> {
    if !is_valid_session_id(old_id) || !is_valid_session_id(new_id) {
        return Err("无效的会话 ID".into());
    }
    let root = workbuddy_data_root();
    let files = collect(old_id)?;
    let mut report = CopyReport::default();

    for sf in &files {
        let new_rel = match remap_rel_path(&sf.path, old_id, new_id) {
            Ok(p) => p,
            Err(e) => {
                report.failed += 1;
                report.failed_files.push(format!("{}: {}", sf.path, e));
                continue;
            }
        };
        let src = root.join(&sf.path);
        let dst = root.join(&new_rel);

        if dst.exists() && !overwrite {
            report.copied += 1;
            continue;
        }

        // 确保父目录存在且不是符号链接
        if let Some(parent) = dst.parent() {
            if let Err(e) = ensure_dir_no_symlink(parent) {
                report.failed += 1;
                report.failed_files.push(format!("{}: {}", new_rel, e));
                continue;
            }
        }

        match std::fs::copy(&src, &dst) {
            Ok(_) => {
                // artifact-index 需要改写 _meta.ownerConversationId
                if new_rel.starts_with("artifact-index/") && new_rel.ends_with(".json") {
                    if let Err(e) = rewrite_artifact_owner(&dst, new_id) {
                        report.failed += 1;
                        report.failed_files.push(format!("{}: owner改写失败: {}", new_rel, e));
                        continue;
                    }
                }
                report.copied += 1;
            }
            Err(e) => {
                report.failed += 1;
                report.failed_files.push(format!("{}: {}", new_rel, e));
            }
        }
    }

    Ok(report)
}

/// 确保目录存在且路径上无符号链接。
fn ensure_dir_no_symlink(dir: &Path) -> Result<(), String> {
    let root = workbuddy_data_root();
    let rel = dir.strip_prefix(&root).map_err(|_| "目标不在 dataRoot 内")?;
    let mut current = root.clone();
    for part in rel.components() {
        current = current.join(part);
        if let Ok(stat) = std::fs::symlink_metadata(&current) {
            if stat.file_type().is_symlink() {
                return Err("路径包含符号链接".into());
            }
            if !stat.is_dir() {
                return Err("路径上存在普通文件占位".into());
            }
        } else {
            std::fs::create_dir_all(&current).map_err(|e| format!("创建目录失败: {e}"))?;
        }
    }
    Ok(())
}

/// 改写 artifact-index/<id>.json 中的 _meta.ownerConversationId 为 new_id。
fn rewrite_artifact_owner(file: &Path, new_id: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(file).map_err(|e| format!("读取产物索引失败: {e}"))?;
    let mut json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("产物索引解析失败: {e}"))?;
    if let Some(meta) = json.get_mut("_meta") {
        if let Some(owner) = meta.get_mut("ownerConversationId") {
            *owner = serde_json::Value::String(new_id.to_string());
        }
    }
    let out = serde_json::to_vec_pretty(&json).map_err(|e| format!("序列化产物索引失败: {e}"))?;
    std::fs::write(file, out).map_err(|e| format!("写入产物索引失败: {e}"))?;
    Ok(())
}

/// 删除指定会话的所有关联文件（回滚用）。
pub fn delete_session_files(session_id: &str) -> Result<(), String> {
    if !is_valid_session_id(session_id) {
        return Err("无效的会话 ID".into());
    }
    let root = workbuddy_data_root();
    let files = collect(session_id)?;
    for sf in &files {
        let target = root.join(&sf.path);
        let _ = std::fs::remove_file(&target);
    }
    // 清理可能空的目录（不递归删除，只删空目录）
    let _ = remove_empty_dir(&root.join("workspace").join("sessions").join(session_id));
    let _ = remove_empty_dir(&root.join("tasks").join(session_id));
    let _ = remove_empty_dir(&root.join("file-history").join(session_id));
    Ok(())
}

fn remove_empty_dir(dir: &Path) {
    if let Ok(stat) = std::fs::symlink_metadata(dir) {
        if stat.is_dir() && !stat.file_type().is_symlink() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// 计算会话文件总数和总大小（供前端展示）。
pub fn session_file_stats(session_id: &str) -> (u64, u64) {
    match collect(session_id) {
        Ok(files) => {
            let count = files.len() as u64;
            let total = files.iter().map(|f| f.size).sum();
            (count, total)
        }
        Err(_) => (0, 0),
    }
}
