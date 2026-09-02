use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::state::AppState;

/// 看门狗标记：用户主动停止代理时置 true，用于区分「主动停止」与「代理进程意外崩溃」。
/// 代理进程意外退出时，系统代理仍指向死端口 127.0.0.1:8899，需自动还原以避免全局断网。
static PROXY_INTENTIONAL_STOP: AtomicBool = AtomicBool::new(false);

/// 启动代理前已存在的系统代理（通常是用户的 VPN 梯子，如 Clash/v2rayN 的本地代理）。
/// 我们启动时会把系统代理全局指向本机 127.0.0.1:8899，并把这个外部代理作为「上游」透传，
/// 停止时再还原回去，避免覆盖/丢失用户原有的 VPN 代理设置。
/// 存储 (enabled, server, override)。
static PREV_SYSTEM_PROXY: Mutex<Option<(bool, String, String)>> = Mutex::new(None);

pub struct ProxyHandle {
    pub child: Child,
    pub port: u16,
    pub started_at: i64,
    pub captured: Arc<AtomicI64>,
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(serde::Serialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub captured: i64,
    pub started_at: Option<i64>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 安全获取 Mutex 锁，即使中毒也能恢复（避免 panic 级联）。
fn safe_lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
pub fn proxy_start(
    app: AppHandle,
    state: State<AppState>,
    proxy_state: State<Mutex<Option<ProxyHandle>>>,
    port: u16,
) -> Result<ProxyStatus, String> {
    // 标记「非主动停止」，供看门狗区分崩溃与用户停止
    PROXY_INTENTIONAL_STOP.store(false, Ordering::Relaxed);
    // [诊断] 记录命令是否到达 Rust 与关键路径解析
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "proxy_start 已到达 Rust: python_dir={:?}, python_exe={}, device_proxy.py 存在={}",
            state.python_dir,
            state.python_exe,
            state.python_dir.join("device_proxy.py").exists()
        ),
    );
    {
        let guard = safe_lock(&proxy_state);
        if guard.is_some() {
            return Err("代理已在运行".into());
        }
    }
    // 兜底：端口为 0 时退化为固定端口 8899，避免注入 TRAE 的代理地址无效（见 store.ts 同款兜底）
    let port = if port == 0 { 8899 } else { port };
    // 本机代理监听地址（系统代理将指向它）
    let proxy_addr = format!("127.0.0.1:{port}");
    let script_path = state.python_dir.join("device_proxy.py");
    if !script_path.exists() {
        return Err(format!("找不到脚本: {}", script_path.display()));
    }
    let data_dir = state.data_dir.to_string_lossy().to_string();
    let port_s = port.to_string();
    let settings = state.settings();
    let proxy_domains = settings.proxy_domains.clone();
    let proxy_log_path = settings.proxy_log_path.clone().unwrap_or_else(|| {
        // 默认路径：%APPDATA%\TraeWorkAssistant\logs（代理请求日志直接存放在 logs/ 下）
        state.logs_dir().to_string_lossy().to_string()
    });
    // 捕获启动前的系统代理（通常是用户的 VPN 梯子，如 Clash/v2rayN 本地代理）。
    // 启动后我们会把系统代理全局指向本机 127.0.0.1:8899，从而拦截所有流量；
    // 若不把原本的 VPN 代理作为「上游」透传，外网(google/github)会直接连不通 ——
    // 这正是「开代理后外网打不开、但关代理+开VPN就正常」的根因。
    let upstream_proxy: Option<String> = {
        #[cfg(target_os = "windows")]
        {
            match get_existing_win_proxy() {
                Some((en, sv, ov))
                    if sv != proxy_addr && !sv.contains(&format!("127.0.0.1:{port}")) =>
                {
                    // 这是外部代理(VPN)，作为上游透传，并在停止时还原
                    *PREV_SYSTEM_PROXY
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) =
                        Some((en, sv.clone(), ov));
                    Some(sv)
                }
                _ => {
                    *PREV_SYSTEM_PROXY
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = None;
                    None
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    };

    let mut cmd = Command::new(&state.python_exe);
    cmd.arg(&script_path)
        .creation_flags(0x08000000)
        .env("TRAEDATA_DIR", &data_dir)
        .env("PROXY_PORT", &port_s)
        .env("AUTO_CAPTURE_JWT", "1")
        .env("PROXY_DOMAINS", &proxy_domains)
        .env("PROXY_LOG_PATH", &proxy_log_path)
        .env("PYTHONIOENCODING", "utf-8");
    if let Some(up) = &upstream_proxy {
        cmd.env("UPSTREAM_PROXY", up);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("启动代理失败: {e}"))?;
    let stdout = child.stdout.take().ok_or("代理无标准输出")?;
    let stderr = child.stderr.take();

    let started_at = now_secs();
    let captured = Arc::new(AtomicI64::new(0));
    {
        let mut g = safe_lock(&proxy_state);
        *g = Some(ProxyHandle {
            child,
            port,
            started_at,
            captured: captured.clone(),
        });
    }

    fs_utils::app_log(
        &state.data_dir,
        &format!("代理已启动: port={port}, pid 已归入 ProxyHandle"),
    );

    // 启动成功：向「实时代理输出」面板明确推送监听地址，便于一眼确认代理是否接上流量
    let listen_line = format!("代理已监听 127.0.0.1:{port}（等待 Trae Work 流量…）");
    let _ = app.emit("proxy-log", &listen_line);
    {
        let log_path =
            std::path::Path::new(&data_dir).join("logs").join("proxy.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        append_log(&log_path, &listen_line);
    }

    let app_for_thread = app.clone();
    let app_for_stderr = app.clone();
    let data_dir2 = data_dir.clone();
    let data_dir3 = data_dir.clone();
    let captured_thread = captured.clone();

    // stdout 线程：逐行读取 -> 事件 emit + 日志追加
    std::thread::spawn(move || {
        let log_path = std::path::Path::new(&data_dir2).join("logs").join("proxy.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let _ = app_for_thread.emit("proxy-log", &l);
                if let Some(uid) = extract_uid(&l) {
                    captured_thread.fetch_add(1, Ordering::Relaxed);
                    let _ = app_for_thread.emit("account-captured", &uid);
                }
                let _ = append_log(&log_path, &l);
            }
        }

        // ── 看门狗 ────────────────────────────────────────────────────────────
        // 走到这里说明子进程 stdout 已 EOF（进程退出）。
        // 若不是「用户主动点击停止」（PROXY_INTENTIONAL_STOP 为 false），
        // 则说明是代理进程「意外崩溃」。此时系统代理仍指向死端口 127.0.0.1:8899，
        // 会导致本机全局断网（签到、Trae 自身流量全部 10061 失败）。
        // 主动还原系统代理并向前端报警。
        if !PROXY_INTENTIONAL_STOP.load(Ordering::Relaxed) {
            let _ = app_for_thread.emit(
                "proxy-log",
                "[严重] 代理进程异常退出，正在还原系统代理以避免全局断网…",
            );
            fs_utils::app_log(std::path::Path::new(&data_dir2), "代理进程异常退出，自动还原系统代理");
            if let Err(e) = clear_win_proxy() {
                fs_utils::app_log(std::path::Path::new(&data_dir2), &format!("还原系统代理失败: {e}"));
            }
            let _ = app_for_thread.emit("proxy-crashed", "");
        }
    });

    // stderr 线程：单独读取，防止管道缓冲区写满导致子进程死锁
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            let log_path =
                std::path::Path::new(&data_dir3).join("logs").join("proxy.log");
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = format!("[stderr] {}", l.trim());
                    // 同时发到前端，避免 Python 启动即崩（如缺依赖）时面板空白、错误只进文件
                    let _ = app_for_stderr.emit("proxy-log", &l);
                    let _ = append_log(&log_path, &l);
                }
            }
        });
    }

    // 同步把 Windows 系统代理指向本机端口，使 TRAE 鉴权请求(api.trae.cn)汇入本代理
    match set_win_proxy(&proxy_addr) {
        Ok(()) => {
            let _ = app.emit(
                "proxy-log",
                &format!("已设置系统代理 -> {proxy_addr}（TRAE 鉴权流量将汇入本代理）"),
            );
            fs_utils::app_log(&state.data_dir, &format!("已设置系统代理 -> {proxy_addr}"));
        }
        Err(e) => {
            let _ = app.emit("proxy-log", &format!("[warn] 设置系统代理失败: {e}"));
        }
    }

    Ok(ProxyStatus {
        running: true,
        port,
        captured: 0,
        started_at: Some(started_at),
    })
}

#[tauri::command]
pub fn proxy_stop(
    _app: AppHandle,
    state: State<AppState>,
    proxy_state: State<Mutex<Option<ProxyHandle>>>,
) -> Result<ProxyStatus, String> {
    let mut g = safe_lock(&proxy_state);
    let (port, captured) = match &*g {
        Some(h) => (h.port, h.captured.load(Ordering::Relaxed)),
        None => (0, 0),
    };
    if let Some(h) = g.take() {
        // 标记「主动停止」，避免看门狗把正常停止误判为崩溃而重复还原代理
        PROXY_INTENTIONAL_STOP.store(true, Ordering::Relaxed);
        let c = h.captured.load(Ordering::Relaxed);
        fs_utils::app_log(&state.data_dir, &format!("代理已停止: 共捕获 {c} 个账号"));
        // h 在此处 drop，Drop trait 会 kill + wait 子进程
    }
    // 还原系统代理：若启动前存在外部代理(VPN)，则还原之；否则清空，避免本机全局断网
    #[cfg(target_os = "windows")]
    {
        let prev = PREV_SYSTEM_PROXY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let res = match prev {
            Some((en, sv, ov)) if en => apply_proxy(true, &sv, &ov),
            _ => clear_win_proxy(),
        };
        if let Err(e) = res {
            fs_utils::app_log(
                &state.data_dir,
                &format!("还原系统代理失败(可手动在设置中关闭): {e}"),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Err(e) = clear_win_proxy() {
            fs_utils::app_log(
                &state.data_dir,
                &format!("还原系统代理失败(可手动在设置中关闭): {e}"),
            );
        }
    }
    Ok(ProxyStatus {
        running: false,
        port,
        captured,
        started_at: None,
    })
}

#[tauri::command]
pub fn proxy_status(
    _app: AppHandle,
    _state: State<AppState>,
    proxy_state: State<Mutex<Option<ProxyHandle>>>,
) -> ProxyStatus {
    let g = safe_lock(&proxy_state);
    match &*g {
        Some(h) => ProxyStatus {
            running: true,
            port: h.port,
            captured: h.captured.load(Ordering::Relaxed),
            started_at: Some(h.started_at),
        },
        None => ProxyStatus {
            running: false,
            port: 0,
            captured: 0,
            started_at: None,
        },
    }
}

fn extract_uid(line: &str) -> Option<String> {
    // 形如 "...user=4487568582777872..." 或 "user_id=..."
    if let Some(idx) = line.find("user=") {
        let rest = &line[idx + 5..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '_'))
            .unwrap_or(rest.len());
        let uid = &rest[..end];
        if uid.chars().all(|c| c.is_ascii_digit()) && !uid.is_empty() {
            return Some(uid.to_string());
        }
    }
    if let Some(idx) = line.find("user_id=") {
        let rest = &line[idx + 8..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '_'))
            .unwrap_or(rest.len());
        let uid = &rest[..end];
        if uid.chars().all(|c| c.is_ascii_digit()) && !uid.is_empty() {
            return Some(uid.to_string());
        }
    }
    None
}

fn append_log(path: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "[{}] {}", crate::fs_utils::now_ts(), line);
    }
}

// ---------------- Windows 系统代理设置 ----------------
// TRAE 的鉴权请求(api.trae.cn)不走 Electron `--proxy-server` 命令行代理，但会读取
// Windows 系统代理(WinINet)。故启动本地代理时同步把系统代理指向本机端口，TRAE 的全部
// 流量(含鉴权)即汇入我们的 MITM 代理；停止时还原，避免全局断网。
#[cfg(target_os = "windows")]
fn apply_proxy(enable: bool, server: &str, override_: &str) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    run_reg(key, "ProxyEnable", "REG_DWORD", if enable { "1" } else { "0" })?;
    if enable {
        run_reg(key, "ProxyServer", "REG_SZ", server)?;
        // 关键：设置 ProxyOverride 让 localhost 绕过代理
        // 这样即使代理开启，客户端仍能直连 127.0.0.1:7864（API 服务）
        let ov = if override_.is_empty() {
            "127.0.0.1;localhost;<local>"
        } else {
            override_
        };
        run_reg(key, "ProxyOverride", "REG_SZ", ov)?;
    }
    notify_wininet_changed();
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn set_win_proxy(addr: &str) -> Result<(), String> {
    apply_proxy(true, addr, "127.0.0.1;localhost;<local>")
}

#[cfg(target_os = "windows")]
pub(crate) fn clear_win_proxy() -> Result<(), String> {
    apply_proxy(false, "", "")
}

/// 通知 WinINet 代理设置已变更，让运行中的进程立即生效
/// 不调用此函数的话，已有进程会继续使用缓存的旧代理设置
#[cfg(target_os = "windows")]
fn notify_wininet_changed() {
    #[link(name = "wininet")]
    extern "system" {
        fn InternetSetOptionW(
            h_internet: *mut std::ffi::c_void,
            option: u32,
            buffer: *mut std::ffi::c_void,
            buffer_length: u32,
        ) -> i32;
    }

    const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
    const INTERNET_OPTION_REFRESH: u32 = 37;

    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

#[cfg(target_os = "windows")]
fn reg_query_value(key: &str, name: &str) -> Option<String> {
    let out = Command::new("reg")
        .args(["query", key, "/v", name])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix(name) {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            // parts[0] = 类型(REG_SZ/REG_DWORD)，parts[1..] = 值
            if parts.len() >= 2 {
                return Some(parts[1..].join(" "));
            }
        }
    }
    None
}

/// 读取启动前的系统代理设置。返回 (enabled, server, override)。
/// 若不存在或未启用则返回 None（表示用户本来就没有系统代理/VPN）。
#[cfg(target_os = "windows")]
fn get_existing_win_proxy() -> Option<(bool, String, String)> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let enable = reg_query_value(key, "ProxyEnable")
        .map(|v| v.contains("1"))
        .unwrap_or(false);
    if !enable {
        return None;
    }
    let server = reg_query_value(key, "ProxyServer").unwrap_or_default();
    if server.is_empty() {
        return None;
    }
    let override_ = reg_query_value(key, "ProxyOverride").unwrap_or_default();
    Some((true, server, override_))
}

#[cfg(target_os = "windows")]
fn run_reg(key: &str, name: &str, kind: &str, value: &str) -> Result<(), String> {
    let status = Command::new("reg")
        .args(["add", key, "/v", name, "/t", kind, "/d", value, "/f"])
        .creation_flags(0x08000000)
        .status()
        .map_err(|e| format!("设置系统代理失败: {e}"))?;
    if !status.success() {
        return Err(format!("reg add 失败: {name}"));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_win_proxy(_addr: &str) -> Result<(), String> {
    Err("仅 Windows 支持系统代理设置".into())
}

#[cfg(not(target_os = "windows"))]
fn clear_win_proxy() -> Result<(), String> {
    Err("仅 Windows 支持系统代理设置".into())
}
