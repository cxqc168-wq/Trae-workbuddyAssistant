use serde::Serialize;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::state::AppState;

/// 登录态快照信息
#[derive(Serialize, Clone)]
pub struct ProfileInfo {
    pub slot: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub last_modified: String,
}

/// profiles 根目录：%APPDATA%\TraeWorkAssistant\data\profiles\
fn profiles_dir(state: &State<AppState>) -> PathBuf {
    state.data_dir.join("data").join("profiles")
}

/// 递归计算目录大小和文件数
fn dir_stats(path: &std::path::Path) -> (u64, u64) {
    let mut size = 0u64;
    let mut count = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let (s, c) = dir_stats(&p);
                size += s;
                count += c;
            } else {
                size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                count += 1;
            }
        }
    }
    (size, count)
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// 列出所有已保存的登录态快照
#[tauri::command]
pub fn profile_list(state: State<AppState>) -> Vec<ProfileInfo> {
    let dir = profiles_dir(&state);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let slot = entry.file_name().to_string_lossy().to_string();
                let (size, count) = dir_stats(&entry.path());
                let last_modified = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| {
                        let dt = chrono::DateTime::<chrono::Local>::from(std::time::SystemTime::UNIX_EPOCH + d);
                        dt.format("%Y-%m-%d %H:%M:%S").to_string()
                    })
                    .unwrap_or_else(|| "-".to_string());
                out.push(ProfileInfo {
                    slot,
                    size_bytes: size,
                    file_count: count,
                    last_modified,
                });
            }
        }
    }
    // 按修改时间倒序
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    out
}

/// 备份当前 TRAE 登录态到指定 slot（调用 PowerShell 脚本）
#[tauri::command]
pub fn profile_backup(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    let ps_dir = if let Ok(r) = std::env::var("TAURI_RESOURCE_DIR") {
        PathBuf::from(r).join("ps")
    } else {
        state.python_dir.join("../ps")
    };
    let bridge = ps_dir.join("trae-switch-bridge.ps1");
    if !bridge.exists() {
        return Err(format!("找不到切换脚本: {}", bridge.display()));
    }

    fs_utils::app_log(&state.data_dir, &format!("开始备份登录态: user_id={user_id}"));

    let mut child = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &bridge.to_string_lossy(),
            "-Action",
            "BackupCurrent",
            "-UserId",
            &user_id,
            "-Json",
        ])
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动备份失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("备份脚本无输出")?;
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let data_dir = state.data_dir.clone();

    // stdout 线程：NDJSON -> profile-progress 事件
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut done_emitted = false;
        for line in std::io::BufRead::lines(reader) {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app2.emit("profile-progress", &l);
                if l.contains("\"stage\":\"done\"") || l.contains("\"stage\":\"fatal\"") {
                    let success = l.contains("\"stage\":\"done\"");
                    done_emitted = true;
                    let _ = app2.emit(
                        "profile-done",
                        serde_json::json!({ "success": success, "raw": l, "action": "backup" }),
                    );
                }
            }
        }
        let exit_status = child.wait();
        if !done_emitted {
            let success = matches!(&exit_status, Ok(s) if s.success());
            let _ = app2.emit(
                "profile-done",
                serde_json::json!({ "success": success, "raw": format!("exit: {:?}", exit_status), "action": "backup" }),
            );
        }
    });

    // stderr 线程
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path = data_dir.join("logs").join("switcher.log");
            let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
            let reader = std::io::BufReader::new(stderr);
            for line in std::io::BufRead::lines(reader) {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = std::writeln!(f, "[{}] {}", fs_utils::now_ts(), l);
                    }
                }
            }
        });
    }

    Ok(())
}

/// 恢复指定 slot 的登录态（关闭 TRAE → 恢复 → 启动 TRAE）
#[tauri::command]
pub fn profile_restore(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    // 检查快照是否存在
    let slot_dir = profiles_dir(&state).join(&user_id);
    if !slot_dir.exists() {
        return Err(format!("账号 {} 的登录态快照不存在", user_id));
    }

    let ps_dir = if let Ok(r) = std::env::var("TAURI_RESOURCE_DIR") {
        PathBuf::from(r).join("ps")
    } else {
        state.python_dir.join("../ps")
    };
    let bridge = ps_dir.join("trae-switch-bridge.ps1");
    if !bridge.exists() {
        return Err(format!("找不到切换脚本: {}", bridge.display()));
    }

    fs_utils::app_log(&state.data_dir, &format!("开始恢复登录态: user_id={user_id}"));

    let mut child = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &bridge.to_string_lossy(),
            "-Action",
            "RestoreOnly",
            "-UserId",
            &user_id,
            "-Json",
        ])
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动恢复失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("恢复脚本无输出")?;
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let data_dir = state.data_dir.clone();

    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut done_emitted = false;
        for line in std::io::BufRead::lines(reader) {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app2.emit("profile-progress", &l);
                if l.contains("\"stage\":\"done\"") || l.contains("\"stage\":\"fatal\"") {
                    let success = l.contains("\"stage\":\"done\"");
                    done_emitted = true;
                    let _ = app2.emit(
                        "profile-done",
                        serde_json::json!({ "success": success, "raw": l, "action": "restore" }),
                    );
                }
            }
        }
        let exit_status = child.wait();
        if !done_emitted {
            let success = matches!(&exit_status, Ok(s) if s.success());
            let _ = app2.emit(
                "profile-done",
                serde_json::json!({ "success": success, "raw": format!("exit: {:?}", exit_status), "action": "restore" }),
            );
        }
    });

    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path = data_dir.join("logs").join("switcher.log");
            let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
            let reader = std::io::BufReader::new(stderr);
            for line in std::io::BufRead::lines(reader) {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = std::writeln!(f, "[{}] {}", fs_utils::now_ts(), l);
                    }
                }
            }
        });
    }

    Ok(())
}

/// 删除指定 slot 的登录态快照
#[tauri::command]
pub fn profile_delete(state: State<AppState>, user_id: String) -> Result<(), String> {
    let slot_dir = profiles_dir(&state).join(&user_id);
    if !slot_dir.exists() {
        return Ok(()); // 不存在视为已删除
    }
    std::fs::remove_dir_all(&slot_dir)
        .map_err(|e| format!("删除快照失败: {e}"))?;
    fs_utils::app_log(&state.data_dir, &format!("已删除登录态快照: user_id={user_id}"));
    Ok(())
}

/// 格式化辅助函数（给前端展示用）
#[tauri::command]
pub fn profile_format_size(bytes: u64) -> String {
    format_size(bytes)
}
