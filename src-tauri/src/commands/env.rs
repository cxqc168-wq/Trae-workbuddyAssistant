use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use tauri::{AppHandle, State};

use crate::state::AppState;

#[derive(Serialize)]
pub struct EnvStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[tauri::command]
pub fn env_check(_app: AppHandle, state: State<AppState>) -> EnvStatus {
    let (installed, path, version) = detect_trae(state.settings().trae_path);
    let running = is_running();
    EnvStatus {
        installed,
        running,
        version,
        path,
    }
}

#[tauri::command]
pub fn open_trae_website(_app: AppHandle) -> Result<(), String> {
    Command::new("cmd")
        .args(["/c", "start", "https://www.trae.cn"])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 启动本地 Trae Work 客户端。
/// 若传入 proxy_port（代理运行中），自动注入 `--proxy-server` 让 Trae 走本地代理，
/// 无需用户在 Trae 设置里手动配置代理。
#[tauri::command]
pub fn open_trae_app(_app: AppHandle, state: State<AppState>, proxy_port: Option<u16>) -> Result<(), String> {
    let (installed, path, _) = detect_trae(state.settings().trae_path);
    if !installed {
        return Err("未检测到本地 Trae Work 安装，请在「设置 → 代理与签到」中指定 exe 路径".into());
    }
    let exe = path.ok_or("未找到 Trae Work 可执行文件路径")?;
    // 代理注入要求 Trae 以 --proxy-server 启动。Electron 单实例下，已运行的窗口会忽略新启动
    // 参数，再次点击只会聚焦旧窗口，导致全程不走代理、无法捕获账号。故注入代理前先结束现有
    // 进程，确保参数真正生效。（无代理时正常打开，不杀进程。）
    if proxy_port.is_some() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "TRAE SOLO CN.exe"])
            .creation_flags(0x08000000)
            .output();
    }
    let mut cmd = Command::new(&exe);
    if let Some(port) = proxy_port {
        // Electron/Chromium 支持 --proxy-server 启动参数
        cmd.arg(format!("--proxy-server=http://127.0.0.1:{port}"));
    }
    cmd.spawn()
        .map_err(|e| format!("启动 Trae Work 失败: {e}"))?;
    Ok(())
}

fn detect_trae(custom: Option<String>) -> (bool, Option<String>, Option<String>) {
    // 优先使用用户在设置中指定的路径（兼容自定义安装目录）
    if let Some(p) = custom {
        let p = p.trim().to_string();
        if !p.is_empty() && std::path::Path::new(&p).is_file() {
            let version = version_of(&p);
            return (true, Some(p), version);
        }
    }
    let candidates = [
        "%LOCALAPPDATA%\\Programs\\TRAE SOLO CN\\TRAE SOLO CN.exe",
        "%LOCALAPPDATA%\\Programs\\TRAE SOLO\\TRAE SOLO.exe",
        "%ProgramFiles%\\TRAE SOLO CN\\TRAE SOLO CN.exe",
        "%ProgramFiles%\\TRAE SOLO\\TRAE SOLO.exe",
        "%LOCALAPPDATA%\\Programs\\Trae\\Trae.exe",
        "%ProgramFiles%\\Trae\\Trae.exe",
    ];
    for c in candidates {
        let expanded = expand_env(c);
        if std::path::Path::new(&expanded).exists() {
            let version = version_of(&expanded);
            return (true, Some(expanded), version);
        }
    }
    // 回退：注册表查询
    if let Some(p) = registry_trae_path() {
        let version = version_of(&p);
        return (true, Some(p), version);
    }
    (false, None, None)
}

fn expand_env(p: &str) -> String {
    p.replace("%LOCALAPPDATA%", &std::env::var("LOCALAPPDATA").unwrap_or_default())
        .replace("%ProgramFiles%", &std::env::var("ProgramFiles").unwrap_or_default())
}

fn version_of(path: &str) -> Option<String> {
    let ps = format!(
        "(Get-Item '{}').VersionInfo.FileVersion",
        path.replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn registry_trae_path() -> Option<String> {
    for root in ["HKCU", "HKLM"] {
        let out = match Command::new("reg")
            .args([
                "query",
                &format!("{root}\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
                "/s",
                "/f",
                "TRAE",
            ])
            .creation_flags(0x08000000)
            .output()
        {
            Ok(o) => o,
            Err(_) => continue,
        };
        let s = String::from_utf8_lossy(&out.stdout);
        // reg query /s 以 HKEY_ 开头的行分隔每个注册表键，逐键解析
        let mut icon: Option<String> = None;
        let mut loc: Option<String> = None;
        let mut name_ok = false;
        let mut best: Option<String> = None;
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with("HKEY_") {
                if name_ok {
                    if let Some(p) = resolve_reg_candidate(&icon, &loc) {
                        best = Some(p);
                        break;
                    }
                }
                icon = None;
                loc = None;
                name_ok = false;
                continue;
            }
            if let Some(v) = line.strip_prefix("DisplayName") {
                if let Some(val) = v.split("REG_SZ").nth(1) {
                    if val.to_uppercase().contains("TRAE") {
                        name_ok = true;
                    }
                }
            } else if let Some(v) = line.strip_prefix("DisplayIcon") {
                if let Some(val) = v.split("REG_SZ").nth(1) {
                    icon = Some(val.trim().to_string());
                }
            } else if let Some(v) = line.strip_prefix("InstallLocation") {
                if let Some(val) = v.split("REG_SZ").nth(1) {
                    loc = Some(val.trim().to_string());
                }
            }
        }
        if name_ok {
            if let Some(p) = resolve_reg_candidate(&icon, &loc) {
                best = Some(p);
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

/// 从注册表 DisplayIcon / InstallLocation 推导 exe 路径
fn resolve_reg_candidate(icon: &Option<String>, loc: &Option<String>) -> Option<String> {
    if let Some(icon) = icon {
        if icon.to_lowercase().ends_with(".exe") && std::path::Path::new(icon).is_file() {
            return Some(icon.clone());
        }
    }
    if let Some(loc) = loc {
        for name in ["TRAE SOLO CN.exe", "TRAE SOLO.exe", "Trae.exe"] {
            let cand = format!("{loc}\\{name}");
            if std::path::Path::new(&cand).is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn is_running() -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq TRAE SOLO CN.exe", "/NH"])
        .creation_flags(0x08000000)
        .output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("TRAE SOLO CN.exe")
        }
        Err(_) => false,
    }
}
