use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::Command;
use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::state::AppState;

#[tauri::command]
pub fn switch_account(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    let ps_dir = crate::state::resolve_ps_dir();
    let bridge = ps_dir.join("trae-switch-bridge.ps1");
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "switch/重置/保存 已到达 Rust: ps_dir={:?}, bridge 存在={}",
            ps_dir,
            bridge.exists()
        ),
    );
    if !bridge.exists() {
        return Err(format!("找不到切换脚本: {}", bridge.display()));
    }

    fs_utils::app_log(&state.data_dir, &format!("开始切换账号: user_id={user_id}"));

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &bridge.to_string_lossy(),
            "-Action",
            "Switch",
            "-UserId",
            &user_id,
            "-Json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW：隐藏切换时闪出的黑色控制台窗口
        .spawn()
        .map_err(|e| format!("启动切换失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("切换脚本无输出")?;
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let data_dir = state.data_dir.clone();

    // stdout 线程：NDJSON -> switch-progress 事件
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut done_emitted = false;
        for line in reader.lines() {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app2.emit("switch-progress", &l);
                // 检测 done / fatal 行，emit switch-done 事件
                if l.contains("\"stage\":\"done\"") || l.contains("\"stage\":\"fatal\"") {
                    let success = l.contains("\"stage\":\"done\"");
                    done_emitted = true;
                    let _ = app2.emit("switch-done", serde_json::json!({ "success": success, "raw": l }));
                }
            }
        }
        let exit_status = child.wait();
        // 仅当脚本未输出 done/fatal 时才在结束时兜底 emit，避免对同一次切换重复发两次 switch-done
        if !done_emitted {
            let success = matches!(&exit_status, Ok(s) if s.success());
            let _ = app2.emit("switch-done", serde_json::json!({ "success": success, "raw": format!("exit: {:?}", exit_status) }));
        }
    });

    // stderr 线程：防止管道缓冲区写满导致子进程死锁
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path = data_dir.join("logs").join("switcher.log");
            let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = writeln!(f, "[{}] {}", fs_utils::now_ts(), l);
                    }
                }
            }
        });
    }

    Ok(())
}

/// 保存当前登录态：关闭 Trae → 精准备份到 userId 槽位 → 重新启动
/// 通过 NDJSON 事件流式返回进度，前端订阅 save-login-progress / save-login-done
#[tauri::command]
pub fn save_current_login(
    app: AppHandle,
    state: State<AppState>,
    user_id: String,
) -> Result<(), String> {
    let ps_dir = crate::state::resolve_ps_dir();
    let bridge = ps_dir.join("trae-switch-bridge.ps1");
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "switch/重置/保存 已到达 Rust: ps_dir={:?}, bridge 存在={}",
            ps_dir,
            bridge.exists()
        ),
    );
    if !bridge.exists() {
        return Err(format!("找不到切换脚本: {}", bridge.display()));
    }

    fs_utils::app_log(&state.data_dir, &format!("开始保存当前登录态: user_id={user_id}"));

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &bridge.to_string_lossy(),
            "-Action",
            "SaveCurrentLogin",
            "-UserId",
            &user_id,
            "-Json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW：隐藏控制台窗口
        .spawn()
        .map_err(|e| format!("启动保存登录态失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("保存登录态脚本无输出")?;
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let data_dir = state.data_dir.clone();

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut done_emitted = false;
        for line in reader.lines() {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app2.emit("save-login-progress", &l);
                if l.contains("\"stage\":\"done\"") || l.contains("\"stage\":\"fatal\"") {
                    let success = l.contains("\"stage\":\"done\"");
                    done_emitted = true;
                    let _ = app2.emit(
                        "save-login-done",
                        serde_json::json!({ "success": success, "raw": l }),
                    );
                }
            }
        }
        let exit_status = child.wait();
        if !done_emitted {
            let success = matches!(&exit_status, Ok(s) if s.success());
            let _ = app2.emit(
                "save-login-done",
                serde_json::json!({ "success": success, "raw": format!("exit: {:?}", exit_status) }),
            );
        }
    });

    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path = data_dir.join("logs").join("switcher.log");
            let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = writeln!(f, "[{}] {}", fs_utils::now_ts(), l);
                    }
                }
            }
        });
    }

    Ok(())
}

/// 6 层设备标识重置：调用 PowerShell 脚本的 ResetDeviceIds 动作
/// 通过 NDJSON 事件流式返回进度，前端订阅 device-reset-progress / device-reset-done
#[tauri::command]
pub fn reset_device_ids(
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let ps_dir = crate::state::resolve_ps_dir();
    let bridge = ps_dir.join("trae-switch-bridge.ps1");
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "switch/重置/保存 已到达 Rust: ps_dir={:?}, bridge 存在={}",
            ps_dir,
            bridge.exists()
        ),
    );
    if !bridge.exists() {
        return Err(format!("找不到切换脚本: {}", bridge.display()));
    }

    fs_utils::app_log(&state.data_dir, "开始 6 层设备标识重置");

    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &bridge.to_string_lossy(),
            "-Action",
            "ResetDeviceIds",
            "-Json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW：隐藏控制台窗口
        .spawn()
        .map_err(|e| format!("启动设备标识重置失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("设备重置脚本无输出")?;
    let stderr = child.stderr.take();
    let app2 = app.clone();
    let data_dir = state.data_dir.clone();

    // stdout 线程：NDJSON -> device-reset-progress 事件
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut done_emitted = false;
        for line in reader.lines() {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app2.emit("device-reset-progress", &l);
                if l.contains("\"stage\":\"done\"") || l.contains("\"stage\":\"fatal\"") {
                    let success = l.contains("\"stage\":\"done\"");
                    done_emitted = true;
                    let _ = app2.emit(
                        "device-reset-done",
                        serde_json::json!({ "success": success, "raw": l }),
                    );
                }
            }
        }
        let exit_status = child.wait();
        if !done_emitted {
            let success = matches!(&exit_status, Ok(s) if s.success());
            let _ = app2.emit(
                "device-reset-done",
                serde_json::json!({ "success": success, "raw": format!("exit: {:?}", exit_status) }),
            );
        }
    });

    // stderr 线程：防止管道缓冲区写满导致子进程死锁
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path = data_dir.join("logs").join("switcher.log");
            let _ = std::fs::create_dir_all(log_path.parent().unwrap_or(std::path::Path::new(".")));
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = writeln!(f, "[{}] {}", fs_utils::now_ts(), l);
                    }
                }
            }
        });
    }

    Ok(())
}

// ==================== 重置设备码（TraeReset 工具复刻） ====================

/// TraeReset 兼容的凭据键模糊匹配子串：storage.json 顶层键命中任一子串即删除（强制登出）。
const AUTH_HINTS: [&str; 20] = [
    "token", "auth", "cookie", "session", "account", "login", "userinfo", "userid",
    "refresh", "access", "credential", "passwd", "signin", "oauth", "bearer",
    "email", "phone", "avatar", "nickname", "member",
];

/// Trae 系列用户数据目录的候选名（对齐 TraeReset：Trae / Trae CN / TraeCN / TRAE SOLO CN）。
const TRAE_DIR_NAMES: [&str; 4] = ["Trae", "Trae CN", "TraeCN", "TRAE SOLO CN"];

/// 生成 uuid4 字符串（小写带连字符），等价 TraeReset 的 gen_uuid。
fn tr_gen_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 生成 64 位十六进制字符（等价 Python secrets.token_hex(32)），用于 telemetry.machineId。
fn tr_gen_machine_id() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 生成大写带花括号 GUID，用于 telemetry.sqmId。
fn tr_gen_guid() -> String {
    format!("{{{}}}", uuid::Uuid::new_v4().to_string().to_uppercase())
}

/// 扫描存在的 Trae 系列用户数据目录（%APPDATA% 与 %LOCALAPPDATA%）。
fn tr_find_trae_dirs() -> Vec<std::path::PathBuf> {
    let mut bases: Vec<std::path::PathBuf> = Vec::new();
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                bases.push(std::path::PathBuf::from(v));
            }
        }
    }
    let home = std::path::PathBuf::from(
        std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string()),
    );
    // 兜底：Linux/macOS 布局（与 TraeReset 一致），Windows 上通常不存在
    bases.push(home.join(".config"));

    let mut found = Vec::new();
    for base in bases {
        if !base.is_dir() {
            continue;
        }
        for name in TRAE_DIR_NAMES {
            let p = base.join(name);
            if p.is_dir() {
                found.push(p);
            }
        }
    }
    found
}

/// 检测 Trae 系列进程是否在运行：tasklist 优先，PowerShell Get-Process 兜底。
/// 任一方式命中即视为运行中，避免受限环境下 tasklist 被拒后误判为"未运行"。
fn tr_is_trae_running() -> bool {
    // 1) tasklist
    if let Ok(out) = Command::new("tasklist")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
            return ["trae.exe", "trae cn", "trae solo"]
                .iter()
                .any(|t| text.contains(t));
        }
    }
    // 2) PowerShell Get-Process 计数兜底
    if let Ok(out) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Process -Name 'trae*' -ErrorAction SilentlyContinue | Measure-Object).Count",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000)
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(n) = s.parse::<u32>() {
                return n > 0;
            }
        }
    }
    false
}

/// 生成带时间戳的备份副本：<原路径>.traereset_bak_YYYYmmdd_HHMMSS
fn tr_backup(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let bak = path.with_file_name(format!("{file_name}.traereset_bak_{ts}"));
    std::fs::copy(path, &bak).map_err(|e| format!("备份失败: {e}"))?;
    Ok(bak)
}

/// storage.json 候选路径（与 TraeReset 一致，按优先级取第一个存在的）。
fn tr_find_storage_json(trae_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    [
        trae_dir.join("User").join("globalStorage").join("storage.json"),
        trae_dir.join("User").join("storage.json"),
        trae_dir.join("storage.json"),
    ]
    .into_iter()
    .find(|c| c.is_file())
}

/// 顶层键是否命中凭据子串（小写包含匹配）。
fn tr_is_auth_key(key: &str) -> bool {
    let k = key.to_lowercase();
    AUTH_HINTS.iter().any(|h| k.contains(h))
}

/// 重置 machineid 文件：读旧值 → 写新 uuid4 → 返回 (旧, 新)。
fn tr_reset_machineid(path: &std::path::Path) -> Result<(String, String), String> {
    let old = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 machineid 失败: {e}"))?
        .trim()
        .to_string();
    let new = tr_gen_uuid();
    std::fs::write(path, &new).map_err(|e| format!("写入 machineid 失败: {e}"))?;
    Ok((old, new))
}

/// 校验文件内容包含期望值（写入验证）。
fn tr_verify(path: &std::path::Path, expected: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|c| c.contains(expected))
        .unwrap_or(false)
}

/// 重置 storage.json：
/// 1) 删除全部命中 AUTH_HINTS 的顶层键（清除凭据、强制登出）
/// 2) 重生成 telemetry.machineId / devDeviceId / sqmId
/// 返回 (被删除的键, 新 telemetry ID)。
fn tr_reset_storage(
    path: &std::path::Path,
) -> Result<(Vec<String>, [String; 3]), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 storage.json 失败: {e}"))?;
    // utf-8-sig 容错：剥掉 BOM 后再解析
    let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let mut data: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(text).map_err(|e| format!("storage.json 解析失败: {e}"))?;

    // 先收集再删除，避免迭代时修改
    let removed: Vec<String> = data
        .keys()
        .filter(|k| tr_is_auth_key(k))
        .cloned()
        .collect();
    for k in &removed {
        data.remove(k);
    }

    let new_ids = [tr_gen_machine_id(), tr_gen_uuid(), tr_gen_guid()];
    data.insert(
        "telemetry.machineId".into(),
        serde_json::Value::String(new_ids[0].clone()),
    );
    data.insert(
        "telemetry.devDeviceId".into(),
        serde_json::Value::String(new_ids[1].clone()),
    );
    data.insert(
        "telemetry.sqmId".into(),
        serde_json::Value::String(new_ids[2].clone()),
    );

    // 对齐 TraeReset 的 json.dump(indent=2, ensure_ascii=False)：serde_json 默认不转义非 ASCII
    let out = serde_json::to_string_pretty(&data)
        .map_err(|e| format!("序列化 storage.json 失败: {e}"))?;
    std::fs::write(path, out).map_err(|e| format!("写入 storage.json 失败: {e}"))?;
    Ok((removed, new_ids))
}

/// 重置设备码：TraeReset 工具的 Rust 原生复刻。
/// 与「6 层重置」的区别——本功能额外清除 storage.json 全部凭据键（强制登出 Trae 内账号），
/// 让服务端把本机识别为全新设备；仅操作用户数据文件，无需管理员权限。
/// 后台线程执行，通过 device-code-reset-progress / device-code-reset-done 事件流式返回。
#[tauri::command]
pub fn reset_device_code(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    fs_utils::app_log(&data_dir, "开始重置设备码（TraeReset 复刻）");

    std::thread::spawn(move || {
        let mut overall_ok = true;
        let emit_line = |line: &str| {
            let _ = app.emit("device-code-reset-progress", line);
        };

        let dirs = tr_find_trae_dirs();
        if dirs.is_empty() {
            overall_ok = false;
            emit_line("[!] 未找到 Trae 用户数据目录。请确认 Trae 已安装。");
        } else if tr_is_trae_running() {
            overall_ok = false;
            emit_line("[!] 检测到 Trae 正在运行，请先完全退出 Trae 后再重置。");
        } else {
            for trae_dir in &dirs {
                emit_line(&format!("\n[目录] {}", trae_dir.display()));

                let machineid_path = trae_dir.join("machineid");
                let storage_path = tr_find_storage_json(trae_dir);

                if !machineid_path.is_file() && storage_path.is_none() {
                    emit_line("    - 未发现 machineid 或 storage.json，跳过。");
                    continue;
                }

                // machineid：备份 → 换 uuid4 → 验证
                if machineid_path.is_file() {
                    let result = tr_backup(&machineid_path)
                        .and_then(|bak| {
                            let (old, new) = tr_reset_machineid(&machineid_path)?;
                            let ok = tr_verify(&machineid_path, &new);
                            Ok((bak, old, new, ok))
                        });
                    match result {
                        Ok((bak, old, new, ok)) => {
                            overall_ok = overall_ok && ok;
                            emit_line(&format!(
                                "    - machineid: {} -> {}  [{}]",
                                if old.is_empty() { "(空)" } else { &old },
                                new,
                                if ok { "OK" } else { "FAIL" }
                            ));
                            emit_line(&format!("      备份: {}", bak.display()));
                        }
                        Err(e) => {
                            overall_ok = false;
                            emit_line(&format!("    - machineid 写入失败: {e}"));
                        }
                    }
                }

                // storage.json：备份 → 清凭据键 + 重生成 telemetry → 验证
                if let Some(storage_path) = storage_path {
                    let result = tr_backup(&storage_path)
                        .and_then(|bak| tr_reset_storage(&storage_path).map(|r| (bak, r)));
                    match result {
                        Ok((bak, (removed, new_ids))) => {
                            let ok = tr_verify(&storage_path, &new_ids[0])
                                && tr_verify(&storage_path, &new_ids[1])
                                && tr_verify(&storage_path, &new_ids[2]);
                            overall_ok = overall_ok && ok;
                            let removed_desc = if removed.is_empty() {
                                "(无)".to_string()
                            } else {
                                format!("[{}]", removed.join(", "))
                            };
                            emit_line(&format!(
                                "    - storage.json 已清除凭据键: {removed_desc}"
                            ));
                            let prefix = &new_ids[0][..new_ids[0].len().min(8)];
                            emit_line(&format!(
                                "    - telemetry.machineId : {prefix}... [{}]",
                                if ok { "OK" } else { "FAIL" }
                            ));
                            emit_line(&format!(
                                "    - telemetry.devDeviceId: {}",
                                new_ids[1]
                            ));
                            emit_line(&format!(
                                "    - telemetry.sqmId     : {}",
                                new_ids[2]
                            ));
                            emit_line(&format!("      备份: {}", bak.display()));
                        }
                        Err(e) => {
                            overall_ok = false;
                            emit_line(&format!("    - storage.json 处理失败: {e}"));
                        }
                    }
                }
            }

            if overall_ok {
                emit_line("\n[完成] 设备码已重置。请重新打开 Trae 并登录新账号。");
            } else {
                emit_line("\n[警告] 部分步骤未完成，请查看上方日志。");
            }
        }

        fs_utils::app_log(
            &data_dir,
            &format!(
                "重置设备码结束: {}",
                if overall_ok { "成功" } else { "存在失败" }
            ),
        );
        let _ = app.emit(
            "device-code-reset-done",
            serde_json::json!({ "success": overall_ok }),
        );
    });

    Ok(())
}

/// 查询已保存的账号快照列表（profiles 目录下的子目录名）
/// 用于前端在账号列表中显示哪些账号有快照，以及切换前预检查
#[tauri::command]
pub fn list_snapshots(state: State<AppState>) -> Result<Vec<String>, String> {
    let profiles_dir = state.data_dir.join("profiles");
    if !profiles_dir.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let entries = std::fs::read_dir(&profiles_dir)
        .map_err(|e| format!("读取快照目录失败: {e}"))?;
    for entry in entries.flatten() {
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            if let Some(name) = entry.file_name().to_str() {
                // 排除 current_account.txt 等非目录项（上面已过滤），排除特殊槽位
                if name != "last" && !name.starts_with('.') {
                    result.push(name.to_string());
                }
            }
        }
    }
    Ok(result)
}
