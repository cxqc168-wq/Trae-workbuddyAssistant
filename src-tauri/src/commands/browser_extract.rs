//! 浏览器一键提取 JWT 登录：通过 CDP 驱动系统 Edge/Chrome，
//! 拦截 trae API 请求的 Authorization 头完成账号保存。
//! 设计文档：docs/superpowers/specs/2026-09-03-browser-extract-jwt-design.md

/// 常用浏览器可执行文件候选路径（Edge 优先于 Chrome，内核一致且更普及）
const BROWSER_CANDIDATES: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

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

/// 是否为 trae API 请求（JWT 出现在这些请求的 Authorization 头中）
pub(crate) fn is_trae_api_url(url: &str) -> bool {
    url.contains("api.trae.com.cn")
}

/// 从 CDP 请求头 JSON 中提取 Authorization 值（头名大小写不敏感）。
/// 入参为 CDP `Network.Request.headers`（Headers newtype 的 inner，
/// 形如 {"Authorization": "Bearer x", ...}；非对象时返回 None）
pub(crate) fn auth_header(headers: &serde_json::Value) -> Option<String> {
    let obj = headers.as_object()?;
    for (k, v) in obj {
        if k.eq_ignore_ascii_case("authorization") {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 从候选路径中选出第一个存在的文件
pub(crate) fn pick_existing(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

/// 浏览器发现：配置了 browser_path 就只用它（配错直接报错，不静默回退）；
/// 未配置则按内置候选顺序探测 Edge → Chrome
pub(crate) fn find_browser(custom: Option<&str>) -> Option<std::path::PathBuf> {
    match custom {
        Some(c) if !c.trim().is_empty() => {
            let p = std::path::PathBuf::from(c.trim());
            if p.is_file() { Some(p) } else { None }
        }
        _ => pick_existing(
            &BROWSER_CANDIDATES
                .iter()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>(),
        ),
    }
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

// ==================== 核心实现：启动/监听/停止 ====================

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chromiumoxide::Browser;
use chromiumoxide::cdp::browser_protocol::network::{EnableParams, EventRequestWillBeSent};
use futures::StreamExt;
use serde_json::json;
use tauri::{Emitter, Manager, State};
use tokio::process::{Child, Command};

use crate::fs_utils;
use crate::jwt;
use crate::models::{AccountsFile, GroupsFile, RawAccount};
use crate::state::AppState;

/// 提取会话句柄：浏览器子进程 + CDP 任务集合
pub struct BrowserExtractHandle {
    pub child: Child,
    pub browser: Browser,
    pub tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl BrowserExtractHandle {
    /// 同步强杀（应用退出等无 await 环境使用）。
    /// tokio 1.53 的 `Child::kill` 是 async（kill + wait），
    /// 同步上下文只能用 `start_kill` 发出终止信号。
    pub fn kill_now(&mut self) {
        let _ = self.child.start_kill();
        for t in self.tasks.drain(..) {
            t.abort();
        }
    }
}

/// progress 事件载荷：{"type": "started|exited|error", "message": "..."}
fn progress(kind: &str, message: &str) -> serde_json::Value {
    json!({ "type": kind, "message": message })
}

/// 启动提取浏览器并开始监听 JWT（幂等：浏览器已存活则直接成功）
#[tauri::command]
pub async fn browser_extract_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Mutex<Option<BrowserExtractHandle>>>,
    group_id: Option<String>,
) -> Result<(), String> {
    // 0. 旧会话清理：浏览器已退出 → 清掉旧句柄重开；仍在运行 → 幂等返回
    {
        let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
        let running = match guard.as_mut() {
            Some(h) => h.child.try_wait().map(|s| s.is_none()).unwrap_or(false),
            None => false,
        };
        if running {
            let _ = app.emit(
                "browser-extract-progress",
                progress("started", "提取浏览器已在运行，请在浏览器中登录 trae.cn"),
            );
            return Ok(());
        }
        if let Some(mut old) = guard.take() {
            old.kill_now();
        }
    }

    // 1. 浏览器发现
    let settings = state.settings();
    let browser_path = match find_browser(settings.browser_path.as_deref()) {
        Some(p) => p,
        None => {
            return Err(match settings.browser_path.as_deref() {
                Some(c) if !c.trim().is_empty() => {
                    format!("设置中的浏览器路径无效：{c}，请到「设置」页修正后重试")
                }
                _ => "未找到 Edge/Chrome 浏览器，请到「设置」页填写「提取浏览器路径」".to_string(),
            });
        }
    };

    // 2. 选调试端口，启动浏览器（持久 profile：登录态跨会话保留）
    let port = find_free_port().ok_or("9333-9433 范围内无可用调试端口")?;
    let profile_dir = state.data_path("browser_profile");
    let mut cmd = Command::new(&browser_path);
    cmd.arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("https://www.trae.cn/");
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW，防控制台闪烁
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动浏览器失败（{}）：{e}", browser_path.display()))?;

    // 3. 等待 CDP HTTP 端点就绪并取 ws 地址
    let ws_url = match wait_debug_endpoint(port).await {
        Ok(u) => u,
        Err(e) => {
            let _ = child.kill().await;
            return Err(e);
        }
    };

    // 4. 连接 CDP（spawn 后所有失败路径统一走 cleanup_spawned：
    //    杀子进程 + abort 已启动任务。tokio 默认 kill_on_drop=false，
    //    仅 drop Child 不会终止进程；且持久 profile 下孤儿实例会抢占
    //    --user-data-dir，导致后续 start 永远等不到调试端口而超时）
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let (browser, mut handler) = match Browser::connect(ws_url).await {
        Ok(v) => v,
        Err(e) => {
            cleanup_spawned(&mut child, &mut tasks).await;
            return Err(format!("连接浏览器调试协议失败：{e}"));
        }
    };

    // handler 驱动任务：流结束 = 浏览器退出 → 通知前端
    let app_exit = app.clone();
    tasks.push(tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
        let _ = app_exit.emit(
            "browser-extract-progress",
            progress("exited", "浏览器已关闭，可重新启动提取"),
        );
    }));

    // 5. 挂 Network 监听：命令行已带 trae.cn 首页，正常至少 1 个页面；
    //    极端时序下 pages 为空则主动开新页兜底
    let mut pages = match browser.pages().await {
        Ok(p) => p,
        Err(e) => {
            cleanup_spawned(&mut child, &mut tasks).await;
            return Err(format!("获取页面失败：{e}"));
        }
    };
    if pages.is_empty() {
        let page = match browser.new_page("https://www.trae.cn/").await {
            Ok(p) => p,
            Err(e) => {
                cleanup_spawned(&mut child, &mut tasks).await;
                return Err(format!("打开 trae.cn 失败：{e}"));
            }
        };
        pages = vec![page];
    }

    let captured: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    for page in pages {
        if page.execute(EnableParams::default()).await.is_err() {
            continue; // 单页启用失败不影响其它页
        }
        let Ok(mut events) = page.event_listener::<EventRequestWillBeSent>().await else {
            continue;
        };
        let app_c = app.clone();
        let captured_c = captured.clone();
        let group_c = group_id.clone();
        tasks.push(tokio::spawn(async move {
            let _keep_alive = page; // 持有页面句柄，防止事件源被释放
            while let Some(ev) = events.next().await {
                if !is_trae_api_url(&ev.request.url) {
                    continue;
                }
                let Some(auth) = auth_header(ev.request.headers.inner()) else {
                    continue;
                };
                let jwt = normalize_token(&auth);
                let info = jwt::parse(&jwt);
                let Some(user_id) = info.user_id else {
                    continue; // 无效 token（解析不出 user_id），静默忽略
                };
                // 会话内去重：每个 user_id 只保存一次
                {
                    let mut set = captured_c.lock().unwrap_or_else(|e| e.into_inner());
                    if set.contains(&user_id) {
                        continue;
                    }
                    set.insert(user_id.clone());
                }
                // 保存走 spawn_blocking：内部含同步网络请求（GetUserInfo 最多 120s）
                let res = tokio::task::spawn_blocking({
                    let app = app_c.clone();
                    let jwt = jwt.clone();
                    let user_id = user_id.clone();
                    let group = group_c.clone();
                    move || save_captured_account(&app, &jwt, &user_id, &group)
                })
                .await;
                // JoinError 与保存失败统一为 String，便于同一分支处理
                let res = match res {
                    Ok(r) => r,
                    Err(e) => Err(format!("后台任务异常：{e}")),
                };
                match res {
                    Ok((name, is_new)) => {
                        let _ = app_c.emit(
                            "browser-extract-captured",
                            json!({
                                "user_id": user_id,
                                "name": name,
                                "exp_hours": info.exp_hours,
                                "is_new": is_new,
                            }),
                        );
                    }
                    Err(e) => {
                        let _ = app_c.emit(
                            "browser-extract-progress",
                            progress("error", &format!("保存账号 [{user_id}] 失败：{e}")),
                        );
                        // 回退去重标记，允许下一次请求重试
                        let mut set = captured_c.lock().unwrap_or_else(|e| e.into_inner());
                        set.remove(&user_id);
                    }
                }
            }
        }));
    }

    // 6. 记录句柄并通知前端
    *runtime.lock().unwrap_or_else(|e| e.into_inner()) = Some(BrowserExtractHandle {
        child,
        browser,
        tasks,
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
                "已启动 {}（调试端口 {}），请在打开的页面中登录 trae.cn",
                browser_path.display(),
                port
            ),
        ),
    );
    Ok(())
}

/// spawn 后中途失败的统一清理：杀浏览器子进程 + abort 已启动任务。
/// 必须显式 kill——tokio 默认 kill_on_drop=false，drop Child 不会终止进程，
/// 孤儿浏览器会一直占用持久 profile，导致后续 start 全部超时失败。
async fn cleanup_spawned(child: &mut Child, tasks: &mut Vec<tokio::task::JoinHandle<()>>) {
    let _ = child.kill().await;
    for t in tasks.drain(..) {
        t.abort();
    }
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
        .find(|a| a.user_id.as_deref() == Some(user_id))
    {
        acct.jwt = jwt.to_string();
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

/// 停止提取并关闭浏览器（幂等）：优先 CDP 优雅关闭（保留 profile 且下次不弹恢复提示），
/// 3s 未退出则强杀兜底
#[tauri::command]
pub async fn browser_extract_stop(
    runtime: State<'_, Mutex<Option<BrowserExtractHandle>>>,
) -> Result<(), String> {
    // 取出句柄后立即释放锁：std MutexGuard 非 Send，不能跨 await（Tauri 命令要求 Send future）
    let Some(mut h) = runtime
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        return Ok(()); // 未运行：幂等
    };
    if h.browser.close().await.is_err() {
        h.kill_now();
        return Ok(());
    }
    if tokio::time::timeout(std::time::Duration::from_secs(3), h.child.wait())
        .await
        .is_err()
    {
        let _ = h.child.kill().await;
    }
    for t in h.tasks.drain(..) {
        t.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn tra_api_url_rejects_site() {
        assert!(!is_trae_api_url("https://www.trae.cn/"));
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
    fn pick_existing_finds_first() {
        let tmp = std::env::temp_dir();
        let a = tmp.join("be_test_missing_a.exe");
        let b = tmp.join(format!("be_test_exists_{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&b);
        std::fs::write(&b, b"x").unwrap();
        assert_eq!(pick_existing(&[a, b.clone()]), Some(b.clone()));
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn pick_existing_none_when_missing() {
        let tmp = std::env::temp_dir();
        let a = tmp.join("be_test_missing_b1.exe");
        let b = tmp.join("be_test_missing_b2.exe");
        assert_eq!(pick_existing(&[a, b]), None);
    }

    #[test]
    fn find_browser_custom_valid() {
        let tmp = std::env::temp_dir();
        let p = tmp.join(format!("be_test_browser_{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(find_browser(Some(p.to_str().unwrap())), Some(p.clone()));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn find_browser_custom_invalid_returns_none() {
        assert_eq!(find_browser(Some(r"C:\nonexistent\browser.exe")), None);
    }
}
