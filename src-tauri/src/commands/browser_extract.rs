//! 浏览器一键提取 JWT 登录：通过 CDP 驱动内置 Playwright 浏览器，
//! 拦截 trae API 请求的 Authorization 头完成账号保存。
//! 设计文档：docs/superpowers/specs/2026-09-30-playwright-browser-extract-design.md

use std::path::{Path, PathBuf};

/// 扫描目录下的 chrome.exe 可执行文件
fn find_chrome_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    for candidate in [
        dir.join("chrome.exe"),
        dir.join("chrome-win64").join("chrome.exe"),
        dir.join("chrome-win").join("chrome.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 扫描本地 %LOCALAPPDATA%\ms-playwright 目录下的 Chromium 浏览器（开发环境自动兜底）
pub fn scan_ms_playwright_dir() -> Option<PathBuf> {
    let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
    let playwright_dir = PathBuf::from(local_appdata).join("ms-playwright");
    if !playwright_dir.is_dir() {
        return None;
    }

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&playwright_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("chromium-") {
                        entries.push(path);
                    }
                }
            }
        }
    }

    // 按名称倒序排序（例如 chromium-1243 在 chromium-1140 之前）
    entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    for dir in entries {
        if let Some(chrome) = find_chrome_in_dir(&dir) {
            return Some(chrome);
        }
    }
    None
}

/// 常用系统浏览器路径（官方正版 Chrome / Edge，具备完整风控与解码器支持）
const STANDARD_BROWSERS: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
];

fn find_system_browser() -> Option<PathBuf> {
    for p in STANDARD_BROWSERS {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 内置浏览器发现：
/// 1. 设置中配置了自定义路径（browser_path）→ 优先使用它（配错直接报错）
/// 2. 应用 resource 目录: <resource_dir>/browser/
/// 3. exe 同级 resources/browser/ 目录
/// 4. 当前工作目录 resources/browser/ 目录
/// 5. 本机官方 Chrome/Edge（官方正版环境，完美通过字节风控）
/// 6. 开发期本地 Playwright 目录兜底
pub fn find_builtin_browser(
    app: &tauri::AppHandle,
    custom_path: Option<&str>,
) -> Option<PathBuf> {
    // 1. 设置了自定义路径：只用它
    if let Some(c) = custom_path {
        if !c.trim().is_empty() {
            let p = PathBuf::from(c.trim());
            if p.is_file() {
                return Some(p);
            }
            return None;
        }
    }

    // 2. Tauri 资源目录 (生产环境打包资源)
    if let Ok(res_dir) = app.path().resource_dir() {
        let browser_dir = res_dir.join("browser");
        if let Some(p) = find_chrome_in_dir(&browser_dir) {
            return Some(p);
        }
    }

    // 3. exe 同级 resources/browser (便携版解压布局)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let browser_dir = exe_dir.join("resources").join("browser");
            if let Some(p) = find_chrome_in_dir(&browser_dir) {
                return Some(p);
            }
        }
    }

    // 4. 当前工作目录 resources/browser
    let cwd_browser = PathBuf::from("resources").join("browser");
    if let Some(p) = find_chrome_in_dir(&cwd_browser) {
        return Some(p);
    }

    // 5. 本机官方 Chrome / Edge（官方正版，人机验证与风控通过率 100%）
    if let Some(p) = find_system_browser() {
        return Some(p);
    }

    // 6. 本地 Playwright 目录兜底
    scan_ms_playwright_dir()
}

/// 调试端口扫描范围（避开常用 9222，减少与用户自己开的调试端口冲突）
const DEBUG_PORT_RANGE: std::ops::RangeInclusive<u16> = 9333..=9433;

/// 归一化 token：兼容 `Cloud-IDE-JWT x` / `Bearer x` / 裸 token，
/// 统一为 `Cloud-IDE-JWT x` 前缀格式（与 oauth.rs / accounts.rs 保存格式一致）
pub(crate) fn normalize_token(raw: &str) -> String {
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Cloud-IDE-JWT ")
        .or_else(|| trimmed.strip_prefix("Bearer "))
        .unwrap_or(trimmed)
        .trim();
    format!("Cloud-IDE-JWT {}", token)
}

/// 是否为 trae 相关请求（JWT 可能出现在这些请求的 Authorization / Token / Cookie 头中）。
/// 包含 trae.cn / trae.com.cn / zijieapi.com / bytedance.com。
pub(crate) fn is_trae_api_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    (u.contains("trae.cn")
        || u.contains("trae.com.cn")
        || u.contains("zijieapi.com")
        || u.contains("bytedance.com"))
        && !u.ends_with(".js")
        && !u.ends_with(".css")
        && !u.ends_with(".png")
        && !u.ends_with(".jpg")
        && !u.ends_with(".svg")
        && !u.ends_with(".ico")
        && !u.ends_with(".woff2")
}

/// 在任意文本中智能提取有效 JWT 字符串（支持包含在 JSON、URL、Cookie、Header 中的 token）
pub(crate) fn find_jwt_in_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 搜索文本中所有形如 eyJ... 的 JWT 片段（标准 JWT 以 {" 编码开头的 eyJ 起始）
    let mut search_from = 0;
    while let Some(start_idx) = trimmed[search_from..].find("eyJ") {
        let abs_start = search_from + start_idx;
        let rest = &trimmed[abs_start..];
        // JWT 仅由 [a-zA-Z0-9_\-] 和 '.' 组成
        let len = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.')
            .unwrap_or(rest.len());
        let candidate = &rest[..len];
        let dot_count = candidate.chars().filter(|&c| c == '.').count();
        if dot_count >= 2 {
            let candidate_norm = normalize_token(candidate);
            let candidate_info = jwt::parse(&candidate_norm);
            if candidate_info.user_id.is_some() {
                return Some(candidate_norm);
            }
        }
        search_from = abs_start + 3;
    }

    None
}

/// 从 CDP 请求头 JSON 中提取有效 Token 值（头名大小写不敏感，支持多头、Cookie 以及通用 JWT 扫描）。
/// 入参为 CDP `Network.Request.headers`（Headers newtype 的 inner，
/// 形如 {"Authorization": "Bearer x", ...}；非对象时返回 None）
pub(crate) fn auth_header(headers: &serde_json::Value) -> Option<String> {
    let obj = headers.as_object()?;
    // 1. 优先检查显式 Token 头（大小写不敏感）
    for (k, v) in obj {
        let lk = k.to_ascii_lowercase();
        if lk == "authorization"
            || lk == "x-cloudide-token"
            || lk == "x-icube-token"
            || lk == "x-tt-token"
            || lk == "token"
        {
            if let Some(s) = v.as_str() {
                let token = s.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }
    // 2. 检查 Cookie 头中可能携带的 cloud_ide_jwt / passport_jwt / token
    for (k, v) in obj {
        if k.eq_ignore_ascii_case("cookie") {
            if let Some(s) = v.as_str() {
                for part in s.split(';') {
                    let trimmed = part.trim();
                    if let Some(val) = trimmed
                        .strip_prefix("cloud_ide_jwt=")
                        .or_else(|| trimmed.strip_prefix("passport_jwt="))
                        .or_else(|| trimmed.strip_prefix("token="))
                    {
                        let token = val.trim();
                        if !token.is_empty() {
                            return Some(token.to_string());
                        }
                    }
                }
            }
        }
    }
    // 3. 通用深度扫描：对任意 Header 键值尝试提取 JWT
    for (_k, v) in obj {
        if let Some(s) = v.as_str() {
            if let Some(jwt) = find_jwt_in_text(s) {
                return Some(jwt);
            }
        }
    }
    None
}

/// 在调试端口范围内找一个当前空闲的端口（存在极小竞争窗口，CDP 连接失败会走报错路径）
fn find_free_port() -> Option<u16> {
    for port in DEBUG_PORT_RANGE {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(port);
        }
    }
    None
}

/// 异步安全清理临时 Profile 目录（带延迟重试，防 Windows 文件锁占用）
pub async fn clean_temp_profile(dir: &Path) {
    if !dir.exists() {
        return;
    }
    for i in 0..5 {
        if tokio::fs::remove_dir_all(dir).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300 * (i + 1))).await;
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ==================== 核心实现：启动/监听/停止 ====================

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chromiumoxide::Browser;
use chromiumoxide::cdp::browser_protocol::network::{EnableParams, EventRequestWillBeSent};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use futures::StreamExt;
use serde_json::json;
use tauri::{Emitter, Manager, State};
use tokio::process::{Child, Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::fs_utils;
use crate::jwt;
use crate::models::{AccountsFile, GroupsFile, RawAccount};
use crate::state::AppState;

/// 提取会话句柄：浏览器子进程 + CDP 任务集合 + 临时 Profile 路径
pub struct BrowserExtractHandle {
    pub child: Child,
    pub browser: Arc<tokio::sync::Mutex<Browser>>,
    pub tasks: Vec<tokio::task::JoinHandle<()>>,
    pub profile_dir: PathBuf,
}

impl BrowserExtractHandle {
    /// 强杀进程树（彻底杀掉 Chrome / Edge 的 renderer、gpu-process、crashpad 等子进程）
    pub fn kill_process_tree(&mut self) {
        #[cfg(windows)]
        if let Some(pid) = self.child.id() {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }
        let _ = self.child.start_kill();
    }

    /// 同步强杀并清理临时目录（应用退出等无 await 环境使用）。
    pub fn kill_now(&mut self) {
        self.kill_process_tree();
        for t in self.tasks.drain(..) {
            t.abort();
        }
        let profile = self.profile_dir.clone();
        tokio::spawn(async move {
            clean_temp_profile(&profile).await;
        });
    }
}

/// progress 事件载荷：{"type": "started|exited|error", "message": "..."}
fn progress(kind: &str, message: &str) -> serde_json::Value {
    json!({ "type": kind, "message": message })
}

const STEALTH_SCRIPT: &str = r#"
try {
    delete Navigator.prototype.webdriver;
} catch(e) {}
try {
    Object.defineProperty(navigator, 'webdriver', { get: () => undefined });
} catch(e) {}
if (!window.chrome) { window.chrome = {}; }
if (!window.chrome.loadTimes) {
    window.chrome.loadTimes = function() {
        return {
            requestTime: Date.now() / 1000,
            startLoadTime: Date.now() / 1000,
            commitLoadTime: Date.now() / 1000,
            finishDocumentLoadTime: Date.now() / 1000,
            finishLoadTime: Date.now() / 1000,
            firstPaintTime: Date.now() / 1000,
            firstPaintAfterLoadTime: 0,
            navigationType: 'Other',
            wasFetchedViaSpdy: true,
            wasNpnNegotiated: true,
            npnNegotiatedProtocol: 'h2',
            wasAlternateProtocolAvailable: false,
            connectionInfo: 'h2'
        };
    };
}
if (!window.chrome.csi) {
    window.chrome.csi = function() {
        return { startE: Date.now(), onloadT: Date.now(), pageT: 100, tran: 15 };
    };
}
"#;

/// 处理捕获到的 JWT：去重、保存账号、通知前端并触发延迟关闭
pub(crate) async fn handle_jwt_capture(
    jwt: &str,
    app: &tauri::AppHandle,
    captured: &Arc<Mutex<HashSet<String>>>,
    group_id: &Option<String>,
) -> bool {
    let norm = normalize_token(jwt);
    let info = jwt::parse(&norm);
    let Some(user_id) = info.user_id else {
        return false;
    };

    {
        let mut set = captured.lock().unwrap_or_else(|e| e.into_inner());
        if set.contains(&user_id) {
            return false;
        }
        set.insert(user_id.clone());
    }

    let app_c = app.clone();
    let jwt_c = norm.clone();
    let uid_c = user_id.clone();
    let gid_c = group_id.clone();
    let res = tokio::task::spawn_blocking(move || {
        save_captured_account(&app_c, &jwt_c, &uid_c, &gid_c)
    })
    .await;

    let res = match res {
        Ok(r) => r,
        Err(e) => Err(format!("后台任务异常：{e}")),
    };

    match res {
        Ok((name, is_new)) => {
            let _ = app.emit(
                "browser-extract-captured",
                json!({
                    "user_id": user_id,
                    "name": name,
                    "exp_hours": info.exp_hours,
                    "is_new": is_new,
                }),
            );
            let _ = app.emit(
                "browser-extract-progress",
                progress("started", &format!("已成功抓取账号 [{name}] JWT，正在自动保存并关闭浏览器…")),
            );
            // 满足需求：用户完成登录并成功抓取到 JWT 后自动关闭浏览器
            let app_stop = app.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                let rt = app_stop.state::<Mutex<Option<BrowserExtractHandle>>>();
                let _ = browser_extract_stop(app_stop.clone(), rt).await;
            });
            true
        }
        Err(e) => {
            let _ = app.emit(
                "browser-extract-progress",
                progress("error", &format!("保存账号 [{user_id}] 失败：{e}")),
            );
            let mut set = captured.lock().unwrap_or_else(|e| e.into_inner());
            set.remove(&user_id);
            false
        }
    }
}

/// 为指定页面挂载 Network 监听，拦截 trae API 请求的 Authorization/Cookie/URL 中的 JWT
fn attach_page_network_listener(
    page: chromiumoxide::Page,
    app: tauri::AppHandle,
    captured: Arc<Mutex<HashSet<String>>>,
    attached_page_ids: Arc<Mutex<HashSet<String>>>,
    group_id: Option<String>,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    let tid = page.target_id().as_ref().to_string();
    tasks.push(tokio::spawn(async move {
        let _ = page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)).await;
        if page.execute(EnableParams::default()).await.is_err() {
            attached_page_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&tid);
            return;
        }
        let Ok(mut events) = page.event_listener::<EventRequestWillBeSent>().await else {
            attached_page_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&tid);
            return;
        };
        let _keep_alive = page;
        while let Some(ev) = events.next().await {
            if !is_trae_api_url(&ev.request.url) {
                continue;
            }
            // 1. 优先尝试从 URL 中嗅探 JWT
            if let Some(jwt) = find_jwt_in_text(&ev.request.url) {
                if handle_jwt_capture(&jwt, &app, &captured, &group_id).await {
                    continue;
                }
            }
            // 2. 从 Headers 中提取 Token / Cookie / JWT
            if let Some(auth) = auth_header(ev.request.headers.inner()) {
                if handle_jwt_capture(&auth, &app, &captured, &group_id).await {
                    continue;
                }
            }
        }
        // 页面跳转或刷新后旧 Target 事件流退出，从 attached 集合中移除以允许重新挂载监听
        attached_page_ids.lock().unwrap_or_else(|e| e.into_inner()).remove(&tid);
    }));
}

/// 启动提取浏览器并开始监听 JWT（幂等：浏览器已存活则直接成功）
/// 每次启动创建全新独立临时 Profile，强制用户在纯净窗口登录以捕获 JWT。
#[tauri::command]
pub async fn browser_extract_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Mutex<Option<BrowserExtractHandle>>>,
    group_id: Option<String>,
) -> Result<(), String> {
    // 0. 旧会话清理：若存在旧句柄（不论是否存活），彻底杀除并清理，保证新提取拥有干净独立的进程与 Profile
    let mut old_handle = {
        let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(mut old) = old_handle.take() {
        old.kill_now();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    // 1. 内置浏览器发现
    let settings = state.settings();
    let browser_path = match find_builtin_browser(&app, settings.browser_path.as_deref()) {
        Some(p) => p,
        None => {
            return Err(match settings.browser_path.as_deref() {
                Some(c) if !c.trim().is_empty() => {
                    format!("设置中的浏览器路径无效：{c}，请到「设置」页修正后重试")
                }
                _ => "未找到软件内置浏览器组件，请确认安装包完整或已安装 Playwright Chromium".to_string(),
            });
        }
    };

    // 2. 选调试端口，创建全新临时 profile，启动浏览器
    let port = find_free_port().ok_or("9333-9433 范围内无可用调试端口")?;
    let temp_profile = std::env::temp_dir().join(format!("trae_extract_{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&temp_profile) {
        return Err(format!("创建临时浏览器配置目录失败：{e}"));
    }

    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "浏览器提取：使用内置浏览器 {} 启动 (临时Profile: {})",
            browser_path.display(),
            temp_profile.display()
        ),
    );

    let mut cmd = Command::new(&browser_path);
    cmd.arg(format!("--remote-debugging-port={port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--remote-allow-origins=*")
        .arg("--lang=zh-CN")
        .arg(format!("--user-data-dir={}", temp_profile.display()))
        // 强制直连，禁止走本机 127.0.0.1:8899 MITM 代理，防止自签 CA 证书被字节 MSSDK 拦截报 Token 无效
        .arg("--no-proxy-server")
        .arg("https://www.trae.cn/login");
    let mut child = cmd
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&temp_profile);
            format!("启动浏览器失败（{}）：{e}", browser_path.display())
        })?;

    // 3. 等待 CDP HTTP 端点就绪并取 ws 地址
    let ws_url = match wait_debug_endpoint(port).await {
        Ok(u) => u,
        Err(e) => {
            let _ = child.kill().await;
            clean_temp_profile(&temp_profile).await;
            return Err(e);
        }
    };

    // 4. 连接 CDP
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let (browser_raw, mut handler) = match Browser::connect(ws_url).await {
        Ok(v) => v,
        Err(e) => {
            cleanup_spawned(&mut child, &mut tasks, &temp_profile).await;
            return Err(format!("连接浏览器调试协议失败：{e}"));
        }
    };
    let browser = Arc::new(tokio::sync::Mutex::new(browser_raw));

    // handler 驱动任务：持续运行驱动 CDP 消息循环，绝不因单条消息解析异常而退出
    // 只有当 handler.next().await 返回 None 时才代表浏览器完全关闭断开连接
    let app_exit = app.clone();
    let exit_profile = temp_profile.clone();
    tasks.push(tokio::spawn(async move {
        while let Some(res) = handler.next().await {
            if let Err(e) = res {
                // 仅忽略非致命的 CDP 消息解析警告，绝不中断循环
                let _ = e;
            }
        }
        // 浏览器实际退出后清理临时 Profile 目录
        clean_temp_profile(&exit_profile).await;
        let _ = app_exit.emit(
            "browser-extract-progress",
            progress("exited", "浏览器已关闭，可重新启动提取"),
        );
    }));

    // 5. 挂 Network 监听：命令行已带 trae.cn 首页，正常至少 1 个页面；
    //    极端时序下 pages 为空则主动开新页兜底
    let mut pages = match browser.lock().await.pages().await {
        Ok(p) => p,
        Err(e) => {
            cleanup_spawned(&mut child, &mut tasks, &temp_profile).await;
            return Err(format!("获取页面失败：{e}"));
        }
    };
    if pages.is_empty() {
        let page = match browser.lock().await.new_page("https://www.trae.cn/login").await {
            Ok(p) => p,
            Err(e) => {
                cleanup_spawned(&mut child, &mut tasks, &temp_profile).await;
                return Err(format!("打开 trae.cn/login 失败：{e}"));
            }
        };
        pages = vec![page];
    }

    let captured: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let attached_page_ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    for page in &pages {
        let tid = page.target_id().as_ref().to_string();
        attached_page_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(tid);
        attach_page_network_listener(
            page.clone(),
            app.clone(),
            captured.clone(),
            attached_page_ids.clone(),
            group_id.clone(),
            &mut tasks,
        );
    }

    let browser_watcher = browser.clone();
    let app_watcher = app.clone();
    let captured_watcher = captured.clone();
    let gid_watcher = group_id.clone();
    let attached_watcher = attached_page_ids.clone();

    // 持续监听新打开的页面（包含 OAuth 弹窗、SSO 独立页等），确保任何登录方式都能捕获
    tasks.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let cur_pages = {
                let b = browser_watcher.lock().await;
                b.pages().await.ok()
            };
            if let Some(open_pages) = cur_pages {
                for p in open_pages {
                    let tid = p.target_id().as_ref().to_string();
                    let should_attach = {
                        let mut set = attached_watcher.lock().unwrap_or_else(|e| e.into_inner());
                        if !set.contains(&tid) {
                            set.insert(tid);
                            true
                        } else {
                            false
                        }
                    };
                    if should_attach {
                        let mut sub_tasks = Vec::new();
                        attach_page_network_listener(
                            p,
                            app_watcher.clone(),
                            captured_watcher.clone(),
                            attached_watcher.clone(),
                            gid_watcher.clone(),
                            &mut sub_tasks,
                        );
                    }
                }
            }
        }
    }));

    // 主动存储扫描引擎：每 1000ms 扫描当前全部打开页面的 LocalStorage / SessionStorage / document.cookie
    let poll_browser = browser.clone();
    let poll_app = app.clone();
    let poll_captured = captured.clone();
    let poll_gid = group_id.clone();
    tasks.push(tokio::spawn(async move {
        let js_scan_storage = r#"
            (() => {
                let texts = [];
                try {
                    for (let i = 0; i < localStorage.length; i++) {
                        let k = localStorage.key(i);
                        texts.push(k);
                        texts.push(localStorage.getItem(k));
                    }
                } catch(e) {}
                try {
                    for (let i = 0; i < sessionStorage.length; i++) {
                        let k = sessionStorage.key(i);
                        texts.push(k);
                        texts.push(sessionStorage.getItem(k));
                    }
                } catch(e) {}
                try {
                    texts.push(document.cookie);
                } catch(e) {}
                return texts.filter(Boolean).join('\n');
            })()
        "#;

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            let pages = {
                let b = poll_browser.lock().await;
                b.pages().await.ok()
            };
            if let Some(open_pages) = pages {
                for page in open_pages {
                    // 1. 页面存储与 DOM 扫描
                    if let Ok(eval_res) = page.evaluate(js_scan_storage).await {
                        if let Some(text) = eval_res.value().and_then(|v| v.as_str()) {
                            if let Some(jwt) = find_jwt_in_text(text) {
                                if handle_jwt_capture(&jwt, &poll_app, &poll_captured, &poll_gid).await {
                                    return;
                                }
                            }
                        }
                    }

                    // 2. 针对处于 trae.cn 站点的页面，适时触发带 credentials 的轻量探测
                    let _ = page.evaluate(r#"
                        (() => {
                            try {
                                if (window.location && window.location.hostname && window.location.hostname.includes('trae.cn')) {
                                    fetch('/api/v1/user/info', { credentials: 'include' }).catch(() => {});
                                }
                            } catch(e) {}
                        })()
                    "#).await;
                }
            }
        }
    }));

    // 6. 记录句柄并通知前端
    *runtime.lock().unwrap_or_else(|e| e.into_inner()) = Some(BrowserExtractHandle {
        child,
        browser,
        tasks,
        profile_dir: temp_profile,
    });
    fs_utils::app_log(
        &state.data_dir,
        &format!(
            "浏览器提取已启动: {} (调试端口 {port})",
            browser_path.display()
        ),
    );
    let _ = app.emit(
        "browser-extract-progress",
        progress(
            "started",
            &format!(
                "已启动 {}（调试端口 {}），请在打开的登录页面中登录",
                browser_path.display(),
                port
            ),
        ),
    );
    Ok(())
}

/// spawn 后中途失败的统一清理：杀浏览器子进程 + abort 已启动任务 + 清理临时 Profile。
async fn cleanup_spawned(
    child: &mut Child,
    tasks: &mut Vec<tokio::task::JoinHandle<()>>,
    profile_dir: &Path,
) {
    let _ = child.kill().await;
    for t in tasks.drain(..) {
        t.abort();
    }
    clean_temp_profile(profile_dir).await;
}

/// 轮询 CDP HTTP 端点直到浏览器就绪（15s 超时），返回 webSocketDebuggerUrl
async fn wait_debug_endpoint(port: u16) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if std::time::Instant::now() > deadline {
            return Err("等待浏览器调试端口就绪超时（15s）".to_string());
        }
        if let Ok(resp) = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .call()
        {
            if let Ok(body) = resp.into_json::<serde_json::Value>() {
                if let Some(ws) = body.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                    return Ok(ws.to_string());
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// 保存捕获账号：已存在则更新 JWT；新账号保存（GetUserInfo 取昵称，失败用 user_id 前 8 位）。
/// 返回 (name, is_new)。分组只对新增账号生效。
fn save_captured_account(
    app: &tauri::AppHandle,
    jwt: &str,
    user_id: &str,
    group_id: &Option<String>,
) -> Result<(String, bool), String> {
    let state = app.state::<AppState>();
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));

    // 已存在：仅更新 JWT
    if let Some(acct) = accounts
        .accounts
        .iter_mut()
        .find(|a| {
            a.user_id.as_deref() == Some(user_id)
                || jwt::parse(&a.jwt).user_id.as_deref() == Some(user_id)
        })
    {
        acct.jwt = jwt.to_string();
        acct.user_id = Some(user_id.to_string());
        acct.updated_at = Some(fs_utils::now_iso());
        let name = acct.name.clone();
        fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;
        fs_utils::app_log(&state.data_dir, &format!("浏览器提取：更新账号 [{user_id}] JWT"));
        return Ok((name, false));
    }

    // 新账号：GetUserInfo 取昵称（尽力而为）
    let name = crate::commands::oauth::get_user_info(jwt)
        .ok()
        .and_then(|(_, uname)| {
            if uname.trim().is_empty() { None } else { Some(uname) }
        })
        .unwrap_or_else(|| format!("账号_{}", user_id.chars().take(8).collect::<String>()));

    accounts.accounts.push(RawAccount {
        id: Some(format!("acc-{}", uuid::Uuid::new_v4().simple())),
        name: name.clone(),
        user_id: Some(user_id.to_string()),
        jwt: jwt.to_string(),
        refresh_token: None,
        added_at: Some(fs_utils::now_iso()),
        updated_at: Some(fs_utils::now_iso()),
    });
    fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;

    if let Some(g) = group_id {
        let mut groups: GroupsFile = fs_utils::read_json(&state.path("groups.json"));
        groups.membership.insert(user_id.to_string(), g.clone());
        fs_utils::write_json(&state.path("groups.json"), &groups)?;
    }

    fs_utils::app_log(
        &state.data_dir,
        &format!("浏览器提取：新增账号 [{name}] user_id={user_id}"),
    );
    Ok((name, true))
}

/// 停止提取并关闭浏览器（幂等）：优先 CDP 优雅关闭，
/// 3s 未退出则强杀进程树兜底，最后清理临时 Profile 目录并通知前端
#[tauri::command]
pub async fn browser_extract_stop(
    app: tauri::AppHandle,
    runtime: State<'_, Mutex<Option<BrowserExtractHandle>>>,
) -> Result<(), String> {
    // 取出句柄后立即释放锁：std MutexGuard 非 Send，不能跨 await（Tauri 命令要求 Send future）
    let Some(mut h) = runtime
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        let _ = app.emit(
            "browser-extract-progress",
            progress("exited", "提取浏览器已关闭"),
        );
        return Ok(()); // 未运行：幂等通知
    };
    let profile_dir = h.profile_dir.clone();

    // 1. 关闭 CDP 浏览器
    let _ = h.browser.lock().await.close().await;

    // 2. 强杀进程树，彻底杀死残留子进程（避免第二次启动端口占用或附加旧进程）
    h.kill_process_tree();

    // 3. 取消关联任务
    for t in h.tasks.drain(..) {
        t.abort();
    }

    // 4. 清理临时 Profile 目录
    clean_temp_profile(&profile_dir).await;

    // 5. 通知前端已完全退出（确保按钮状态与 running 标志位正确重置）
    let _ = app.emit(
        "browser-extract-progress",
        progress("exited", "浏览器已关闭，可重新启动提取"),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_find_system_browser() {
        let b = find_system_browser();
        if let Some(p) = b {
            assert!(p.is_file());
            let s = p.to_string_lossy().to_lowercase();
            assert!(s.contains("chrome.exe") || s.contains("msedge.exe"));
        }
    }

    #[test]
    fn test_scan_ms_playwright_dir() {
        let found = scan_ms_playwright_dir();
        if let Some(path) = found {
            assert!(path.is_file());
            assert!(path.to_string_lossy().to_lowercase().contains("chrome.exe"));
        }
    }

    #[test]
    fn normalize_token_bearer() {
        assert_eq!(normalize_token("Bearer abc123"), "Cloud-IDE-JWT abc123");
    }

    #[test]
    fn normalize_token_cloud_ide_prefix_kept() {
        assert_eq!(
            normalize_token("Cloud-IDE-JWT abc123"),
            "Cloud-IDE-JWT abc123"
        );
    }

    #[test]
    fn normalize_token_bare() {
        assert_eq!(normalize_token("abc123"), "Cloud-IDE-JWT abc123");
    }

    #[test]
    fn normalize_token_trims_whitespace() {
        assert_eq!(normalize_token("  Bearer   abc \n"), "Cloud-IDE-JWT abc");
    }

    #[test]
    fn tra_api_url_matches() {
        assert!(is_trae_api_url(
            "https://api.trae.com.cn/cloudide/api/v3/trae/GetUserInfo"
        ));
    }

    #[test]
    fn tra_api_cn_domain_matches() {
        // 国内站实际 API 域名为 api.trae.cn（非 .com.cn），必须能匹配
        assert!(is_trae_api_url(
            "https://api.trae.cn/trae/api/v2/pay/cn_credits_billing_status"
        ));
    }

    #[test]
    fn tra_api_url_rejects_site() {
        assert!(!is_trae_api_url("https://www.google.com/"));
        assert!(!is_trae_api_url("https://api.trae.cn/static/bundle.js"));
    }

    #[test]
    fn auth_header_from_custom_token_and_cookie() {
        assert_eq!(
            auth_header(&json!({"x-cloudide-token": "abc123"})),
            Some("abc123".to_string())
        );
        assert_eq!(
            auth_header(&json!({"Cookie": "session=1; cloud_ide_jwt=token_xyz; other=2"})),
            Some("token_xyz".to_string())
        );
    }

    #[test]
    fn auth_header_found() {
        assert_eq!(
            auth_header(&json!({"Authorization": "Bearer xyz"})),
            Some("Bearer xyz".to_string())
        );
    }

    #[test]
    fn auth_header_case_insensitive() {
        assert_eq!(
            auth_header(&json!({"authorization": "Bearer xyz"})),
            Some("Bearer xyz".to_string())
        );
    }

    #[test]
    fn auth_header_absent() {
        assert_eq!(
            auth_header(&json!({"Content-Type": "application/json"})),
            None
        );
    }

    #[test]
    fn auth_header_non_object_returns_none() {
        assert_eq!(auth_header(&json!("not an object")), None);
        assert_eq!(auth_header(&json!(null)), None);
    }

    #[test]
    fn auth_header_whitespace_value_returns_none() {
        assert_eq!(auth_header(&json!({"Authorization": "   "})), None);
    }

    #[test]
    fn find_chrome_in_dir_finds_exe() {
        let tmp = std::env::temp_dir().join(format!("be_test_dir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let chrome = tmp.join("chrome.exe");
        std::fs::write(&chrome, b"x").unwrap();
        assert_eq!(find_chrome_in_dir(&tmp), Some(chrome));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_chrome_in_dir_missing_returns_none() {
        let tmp = std::env::temp_dir().join(format!("be_test_empty_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        assert_eq!(find_chrome_in_dir(&tmp), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_jwt_in_text_various_sources() {
        let fake_jwt = "eyJhbGciOiJub25lIn0.eyJkYXRhIjp7ImlkIjoiMTIzNDU2Nzg5In0sImV4cCI6MTk5OTk5OTk5OX0.dummy";
        // 裸 token
        assert_eq!(
            find_jwt_in_text(fake_jwt),
            Some(format!("Cloud-IDE-JWT {}", fake_jwt))
        );
        // Cookie 字符串
        let cookie_str = format!("sessionid=abc; cloud_ide_jwt={}; path=/", fake_jwt);
        assert_eq!(
            find_jwt_in_text(&cookie_str),
            Some(format!("Cloud-IDE-JWT {}", fake_jwt))
        );
        // JSON 字符串
        let json_str = format!(r#"{{"token":"Bearer {}","code":0}}"#, fake_jwt);
        assert_eq!(
            find_jwt_in_text(&json_str),
            Some(format!("Cloud-IDE-JWT {}", fake_jwt))
        );
        // URL 参数
        let url_str = format!("https://www.trae.cn/?token={}&user=123", fake_jwt);
        assert_eq!(
            find_jwt_in_text(&url_str),
            Some(format!("Cloud-IDE-JWT {}", fake_jwt))
        );
    }
}
