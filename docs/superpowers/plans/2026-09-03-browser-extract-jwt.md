# 浏览器一键提取 JWT 登录 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在账户页新增「浏览器提取」入口：应用启动系统 Edge/Chrome（持久 profile），用户登录 trae.cn 后通过 CDP 拦截 API 请求的 Authorization 头提取 JWT 并自动保存账号，解决 OAuth 授权页卡「认证中」的问题。

**Architecture:** 新增 Rust 模块 `commands/browser_extract.rs`，用 chromiumoxide（纯 Rust CDP 客户端）连接自管的浏览器子进程（随机调试端口 + 持久 user-data-dir），监听 `Network.requestWillBeSent` 提取 JWT → 按 user_id 去重 → 复用 oauth.rs 的账号保存模式写入 `checkin_accounts.json` → Tauri 事件推给前端弹窗。前端 `Accounts.tsx` 新增 `BrowserExtractModal`。

**Tech Stack:** Rust (tauri 2, chromiumoxide 0.7, futures, tokio), TypeScript/React。

**设计文档:** `docs/superpowers/specs/2026-09-03-browser-extract-jwt-design.md`

**注意:** chromiumoxide 依赖较大（chromiumoxide_cdp 生成代码多），首次 `cargo check` 会多花 1-3 分钟下载编译。若编译时发现 chromiumoxide API 与计划代码有出入（版本差异），以本地 `cargo doc` / crate 源码为准调整调用方式，**保持逻辑不变**。

---

### Task 1: Rust 依赖与模块骨架

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/commands/browser_extract.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: 添加依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]` 末尾追加：

```toml
chromiumoxide = { version = "0.7", default-features = true }
futures = "0.3"
```

同时把 tokio 一行的 features 增加 `time` 和 `process`：

```toml
tokio = { version = "1", features = ["sync", "io-util", "net", "rt", "macros", "time", "process"] }
```

- [ ] **Step 2: 创建模块骨架**

`src-tauri/src/commands/browser_extract.rs`：

```rust
//! 浏览器一键提取 JWT 登录：通过 CDP 驱动系统 Edge/Chrome，
//! 拦截 trae API 请求的 Authorization 头完成账号保存。
//! 设计文档：docs/superpowers/specs/2026-09-03-browser-extract-jwt-design.md
```

- [ ] **Step 3: 注册模块**

`src-tauri/src/commands/mod.rs`（当前按字母排序）在 `pub mod cert;` 之后插入：

```rust
pub mod browser_extract;
```

- [ ] **Step 4: 验证编译**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 编译通过（首次较慢，下载 chromiumoxide 及其依赖）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/commands/browser_extract.rs src-tauri/src/commands/mod.rs
git commit -m "feat(browser-extract): 添加 chromiumoxide 依赖与模块骨架"
```

---

### Task 2: 纯函数实现（TDD）

**Files:**
- Modify: `src-tauri/src/commands/browser_extract.rs`

先写测试（失败），再实现。纯函数不依赖 async/浏览器，可直接单测。

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/commands/browser_extract.rs` 追加：

```rust
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
        let mut h = serde_json::Map::new();
        h.insert("Authorization".into(), json!("Bearer xyz"));
        assert_eq!(auth_header(&h), Some("Bearer xyz".to_string()));
    }

    #[test]
    fn auth_header_case_insensitive() {
        let mut h = serde_json::Map::new();
        h.insert("authorization".into(), json!("Bearer xyz"));
        assert_eq!(auth_header(&h), Some("Bearer xyz".to_string()));
    }

    #[test]
    fn auth_header_absent() {
        let mut h = serde_json::Map::new();
        h.insert("Content-Type".into(), json!("application/json"));
        assert_eq!(auth_header(&h), None);
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml browser_extract`
Expected: FAIL，报 `cannot find function normalize_token in this module`（编译错误）

- [ ] **Step 3: 实现纯函数**

在 `browser_extract.rs` 模块文档注释后追加：

```rust
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

/// 从 CDP 请求头 Map 中提取 Authorization 值（头名大小写不敏感）
pub(crate) fn auth_header(headers: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    for (k, v) in headers {
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml browser_extract`
Expected: 全部 PASS（find_free_port 会有 unused 警告，Task 3 会用到，可暂容忍或加 `#[allow(dead_code)]`）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/browser_extract.rs
git commit -m "feat(browser-extract): token 归一/请求头提取/浏览器发现纯函数（含单测）"
```

---

### Task 3: oauth.rs 辅助函数开放复用

**Files:**
- Modify: `src-tauri/src/commands/oauth.rs:46` 和 `src-tauri/src/commands/oauth.rs:211`

- [ ] **Step 1: 开放 short_agent 与 get_user_info**

[oauth.rs:46](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\commands\oauth.rs#L46) 处：

```rust
/// 短请求 Agent
pub(crate) fn short_agent() -> ureq::Agent {
```

[oauth.rs:211](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\commands\oauth.rs#L211) 处：

```rust
/// GetUserInfo：获取用户信息
pub(crate) fn get_user_info(access_token: &str) -> Result<(String, String), String> {
```

只把 `fn` 改成 `pub(crate) fn`，函数体不动。

- [ ] **Step 2: 验证编译**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 通过（可能有 unused 警告，Task 4 使用后消失）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands/oauth.rs
git commit -m "refactor(oauth): short_agent/get_user_info 开放为 pub(crate) 供浏览器提取复用"
```

---

### Task 4: Settings 增加 browser_path

**Files:**
- Modify: `src-tauri/src/models.rs:71-108`（Settings 结构体）
- Modify: `src/types.ts:76-95`（TS Settings 接口）
- Modify: `src/pages/Settings.tsx`（代理与签到 section，trae_path 输入框之后）

- [ ] **Step 1: Rust 结构体加字段**

[models.rs](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\models.rs#L93) 的 `pub trae_path: Option<String>,` 之后追加：

```rust
    #[serde(default)]
    pub browser_path: Option<String>,
```

- [ ] **Step 2: TS 类型加字段**

[types.ts](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\types.ts#L87) 的 `trae_path: string | null;` 之后追加：

```ts
  browser_path: string | null;
```

- [ ] **Step 3: Settings 页面加输入框**

[Settings.tsx](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Settings.tsx#L315-L332)「Trae Work 安装路径」div 关闭标签 `</div>` 之后（即 332 行后）插入：

```tsx
            <div>
              <label className="label">提取浏览器路径</label>
              <input
                type="text"
                value={form.browser_path ?? ''}
                onChange={(e) => update('browser_path', e.target.value.trim() || null)}
                placeholder="留空则自动检测（Edge 优先，其次 Chrome）"
                className="input"
              />
              <p className="mt-1 text-xs text-slate-400">
                「浏览器提取 JWT」功能使用的浏览器 exe 路径；留空将按 Edge → Chrome 顺序自动探测。
              </p>
            </div>
```

- [ ] **Step 4: 验证**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 通过

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src/types.ts src/pages/Settings.tsx
git commit -m "feat(settings): 新增提取浏览器路径配置项 browser_path"
```

---

### Task 5: 后端核心 — 启动/捕获/停止

**Files:**
- Modify: `src-tauri/src/commands/browser_extract.rs`（追加核心实现）
- Modify: `src-tauri/src/main.rs`（manage + 命令注册 + 退出清理）

**背景知识（给零上下文工程师）:**
- CDP（Chrome DevTools Protocol）：浏览器调试协议。启动浏览器时传 `--remote-debugging-port=<port>`，然后 HTTP GET `http://127.0.0.1:<port>/json/version` 返回的 `webSocketDebuggerUrl` 可建立 WebSocket 连接。`Network.requestWillBeSent` 事件会携带页面发出的每个请求的 URL 和请求头。
- Edge/Chrome 136+ 出于安全禁止在**默认**用户数据目录上开调试端口，但允许自定义 `--user-data-dir`——本功能用自己的数据目录，天然合规，这也是必须传自定义 profile 的另一原因。
- chromiumoxide 的 `Browser::connect(ws_url)` 返回 `(Browser, Handler)`，**Handler 必须被 spawn 的任务持续 poll**（`while let Some(h) = handler.next().await`），否则收不到任何事件。
- tauri 异步命令中 `State<'_, T>` 可跨 await 持有（需 `T: Send + Sync`）。
- 账号保存逻辑参照 `src-tauri/src/commands/oauth.rs` 的 `oauth_login`（第 252-354 行）：存在同 user_id 则更新 jwt，否则 push 新 `RawAccount` 并写 groups.json。

- [ ] **Step 1: 追加 handle 结构与事件载荷定义**

在 `browser_extract.rs` 纯函数之后追加：

```rust
use std::collections::HashSet;
use std::sync::Arc;

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
    /// 同步强杀（应用退出等无 await 环境使用）
    pub fn kill_now(&mut self) {
        let _ = self.child.kill();
        for t in self.tasks.drain(..) {
            t.abort();
        }
    }
}

/// progress 事件载荷：{"type": "started|exited|error", "message": "..."}
fn progress(kind: &str, message: &str) -> serde_json::Value {
    json!({ "type": kind, "message": message })
}
```

- [ ] **Step 2: 追加 start 命令**

```rust
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
        if let Some(h) = guard.as_mut() {
            let running = h.child.try_wait().map(|s| s.is_none()).unwrap_or(false);
            if running {
                let _ = app.emit(
                    "browser-extract-progress",
                    progress("started", "提取浏览器已在运行，请在浏览器中登录 trae.cn"),
                );
                return Ok(());
            }
            let mut old = guard.take().unwrap();
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
            let _ = child.kill();
            return Err(e);
        }
    };

    // 4. 连接 CDP
    let (browser, mut handler) = Browser::connect(ws_url)
        .await
        .map_err(|e| format!("连接浏览器调试协议失败：{e}"))?;

    // handler 驱动任务：流结束 = 浏览器退出 → 通知前端
    let app_exit = app.clone();
    let handler_task = tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            if h.is_err() {
                break;
            }
        }
        let _ = app_exit.emit(
            "browser-extract-progress",
            progress("exited", "浏览器已关闭，可重新启动提取"),
        );
    });
    let mut tasks = vec![handler_task];

    // 5. 挂 Network 监听：命令行已带 trae.cn 首页，正常至少 1 个页面；
    //    极端时序下 pages 为空则主动开新页兜底
    let mut pages = browser.pages().await.map_err(|e| format!("获取页面失败：{e}"))?;
    if pages.is_empty() {
        let page = browser
            .new_page("https://www.trae.cn/")
            .await
            .map_err(|e| format!("打开 trae.cn 失败：{e}"))?;
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
                let Some(auth) = auth_header(&ev.request.headers) else {
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
                match res {
                    Ok(Ok((name, is_new))) => {
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
                    Ok(Err(e)) | Err(e) => {
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
```

注意顶部还需要 `use std::sync::Mutex;`（与 Task 5 Step 1 的 `use std::sync::Arc;` 合并为 `use std::sync::{Arc, Mutex};`）。

- [ ] **Step 3: 追加保存函数与 stop 命令**

```rust
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
        fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;
        fs_utils::app_log(&state.data_dir, &format!("浏览器提取：更新账号 [{user_id}] JWT"));
        return Ok((acct.name.clone(), false));
    }

    // 新账号：GetUserInfo 取昵称（尽力而为）
    let name = crate::commands::oauth::get_user_info(jwt)
        .ok()
        .and_then(|(_, uname)| {
            if uname.trim().is_empty() { None } else { Some(uname) }
        })
        .unwrap_or_else(|| format!("账号_{}", &user_id[..user_id.len().min(8)]));

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
    let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut h) = guard.take() else {
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
```

- [ ] **Step 4: main.rs 注册**

[main.rs:36](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\main.rs#L36) 的 `.manage(Mutex::new(Option::<commands::oauth::OAuthCallbackHandle>::None))` 之后追加一行：

```rust
        .manage(Mutex::new(Option::<commands::browser_extract::BrowserExtractHandle>::None))
```

[main.rs:101](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\main.rs#L101) 的 `commands::oauth::oauth_callback_stop,` 之后追加两行：

```rust
            commands::browser_extract::browser_extract_start,
            commands::browser_extract::browser_extract_stop,
```

- [ ] **Step 5: main.rs 应用退出清理**

[main.rs:246](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\main.rs#L239-L246)（`RunEvent::Exit` 中「应用退出时停止 API 服务」块之后、`clear_win_proxy` 之前）插入：

```rust
            // 应用退出时关闭提取浏览器，防止孤儿浏览器进程
            let be_state = app_handle
                .state::<Mutex<Option<commands::browser_extract::BrowserExtractHandle>>>();
            let mut bg = be_state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(mut h) = bg.take() {
                if let Some(st) = app_handle.try_state::<AppState>() {
                    fs_utils::app_log(&st.data_dir, "应用退出：关闭提取浏览器");
                }
                h.kill_now();
            }
```

- [ ] **Step 6: 验证编译与测试**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 通过。若 chromiumoxide API 报错（如 `event_listener` 泛型签名、`Browser::close` 返回类型不同），按编译器提示以本地 crate 源码（`~/.cargo/registry/src/.../chromiumoxide-0.7*/`）为准微调调用，逻辑不变。

Run: `cargo test --manifest-path src-tauri/Cargo.toml browser_extract`
Expected: Task 2 的全部单测仍 PASS

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/browser_extract.rs src-tauri/src/main.rs
git commit -m "feat(browser-extract): CDP 启动/监听/保存账号/优雅停止核心实现"
```

---

### Task 6: 前端类型与 API 封装

**Files:**
- Modify: `src/types.ts`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: types.ts 追加事件载荷类型**

文件末尾（或 CheckinOpts 附近）追加：

```ts
// ---- 浏览器提取 JWT ----
export interface BrowserExtractCaptured {
  user_id: string;
  name: string;
  exp_hours: number | null;
  is_new: boolean;
}

export interface BrowserExtractProgress {
  type: 'started' | 'exited' | 'error';
  message: string;
}
```

- [ ] **Step 2: lib/tauri.ts 追加 api 封装**

[tauri.ts](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\lib\tauri.ts#L142) 的 `oauth: {...}` 块之后追加（与 oauth 平级）：

```ts
  browserExtract: {
    start: (groupId?: string) =>
      invoke('browser_extract_start', { groupId }),
    stop: () => invoke('browser_extract_stop'),
  },
```

顶部 `import type {...}` 列表中追加 `BrowserExtractCaptured`（`BrowserExtractProgress` 若无直接引用可不加）。

- [ ] **Step 3: 验证**

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add src/types.ts src/lib/tauri.ts
git commit -m "feat(browser-extract): 前端事件载荷类型与 api 封装"
```

---

### Task 7: Accounts 页面 — 入口按钮与 BrowserExtractModal

**Files:**
- Modify: `src/pages/Accounts.tsx`

**背景:** 页面顶部 `PageHeader` 的 actions 里已有「OAuth 登录」按钮（约 198 行）；`OAuthLoginModal` 在约 442 行渲染；`JwtStatusBadge`（38 行）接收 `hours: number | null` 显示有效期徽标，可直接复用；`Modal` 组件的 `onClose` 在 Esc/遮罩/X 时触发。

- [ ] **Step 1: 导入图标与类型**

[Accounts.tsx:2-26](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L2-L26) 的 lucide 导入列表中加入 `ScanSearch`。

[Accounts.tsx:32](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L32) 类型导入改为：

```ts
import type { AccountView, BrowserExtractCaptured, BrowserExtractProgress, GroupView, JwtParseResult, ProfileInfo } from '../types';
```

- [ ] **Step 2: 新增状态与按钮**

[Accounts.tsx:127](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L127) `const [oauthOpen, setOAuthOpen] = useState(false);` 之后追加：

```ts
  const [extractOpen, setExtractOpen] = useState(false);
```

[Accounts.tsx:198-200](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L198-L200)「OAuth 登录」按钮之后追加：

```tsx
            <button onClick={() => setExtractOpen(true)} className="btn-outline" title="启动浏览器登录 trae.cn 并自动提取 JWT">
              <ScanSearch size={15} /> 浏览器提取
            </button>
```

- [ ] **Step 3: 渲染 Modal**

[Accounts.tsx:442](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L442) 附近（`<OAuthLoginModal ... />` 渲染之后）追加：

```tsx
      <BrowserExtractModal
        open={extractOpen}
        onClose={() => setExtractOpen(false)}
        groups={groups}
      />
```

- [ ] **Step 4: 实现 BrowserExtractModal 组件**

在 `OAuthLoginModal` 函数定义之后（约 1291 行后）追加完整组件：

```tsx
function BrowserExtractModal({
  open,
  onClose,
  groups,
}: {
  open: boolean;
  onClose: () => void;
  groups: GroupView[];
}) {
  const toast = useAppStore((s) => s.pushToast);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const [starting, setStarting] = useState(false);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [captured, setCaptured] = useState<BrowserExtractCaptured[]>([]);
  const [gid, setGid] = useState('');

  useEffect(() => {
    if (!open) {
      setStarting(false);
      setRunning(false);
      setStopping(false);
      setLogs([]);
      setCaptured([]);
      setGid('');
    }
  }, [open]);

  // 监听后端事件：progress（状态日志）与 captured（捕获账号）
  useEffect(() => {
    if (!open) return;
    const unsubs: Array<() => void> = [];
    let alive = true;
    void import('@tauri-apps/api/event').then(({ listen }) => {
      if (!alive) return;
      void listen<BrowserExtractProgress>('browser-extract-progress', (e) => {
        setLogs((ls) => [...ls, e.payload.message]);
        if (e.payload.type === 'exited') setRunning(false);
      }).then((f) => unsubs.push(f));
      void listen<BrowserExtractCaptured>('browser-extract-captured', (e) => {
        setCaptured((cs) => [...cs, e.payload]);
        toast(
          'success',
          `已捕获账号 ${e.payload.name}（${e.payload.is_new ? '新增' : '更新'}）`,
        );
      }).then((f) => unsubs.push(f));
    });
    return () => {
      alive = false;
      unsubs.forEach((f) => f());
    };
  }, [open, toast]);

  const start = async () => {
    setStarting(true);
    try {
      await api.browserExtract.start(gid || undefined);
      setRunning(true);
    } catch (err) {
      toast('error', `启动提取浏览器失败：${String(err)}`);
    } finally {
      setStarting(false);
    }
  };

  // 关闭弹窗前必停浏览器：X/取消/完成共用此路径，防止孤儿进程
  const finish = async () => {
    setStopping(true);
    try {
      await api.browserExtract.stop();
      await refreshAccounts();
    } catch (err) {
      toast('warn', `关闭提取浏览器失败：${String(err)}`);
    } finally {
      setStopping(false);
      onClose();
    }
  };

  return (
    <Modal
      open={open}
      onClose={() => void finish()}
      title="浏览器提取 JWT"
      footer={
        <>
          <button onClick={() => void finish()} className="btn-ghost" disabled={stopping}>
            关闭
          </button>
          {!running && (
            <button onClick={() => void start()} disabled={starting} className="btn-primary">
              {starting ? '正在启动…' : captured.length > 0 ? '重新启动' : '启动提取浏览器'}
            </button>
          )}
          {running && (
            <button onClick={() => void finish()} className="btn-primary" disabled={stopping}>
              {stopping ? '正在关闭…' : '完成并关闭'}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        {/* 步骤说明 */}
        <div className="rounded-lg border border-slate-200 bg-slate-50 p-3 text-xs leading-relaxed text-slate-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
          1. 点击「启动提取浏览器」——应用会打开一个专用浏览器窗口（登录态会保留，下次免登录）；
          <br />
          2. 在浏览器中登录 trae.cn，系统自动拦截登录凭证并保存为账号；
          <br />
          3. 多账号：在浏览器中退出当前账号、登录下一个，即可连续提取；
          <br />
          4. 全部完成后点击「完成并关闭」。适合 OAuth 授权页卡「认证中」时使用。
        </div>

        {/* 分组（启动前选择，仅对新增账号生效） */}
        {!running && (
          <div>
            <label className="label">新账号分组（可选）</label>
            <select value={gid} onChange={(e) => setGid(e.target.value)} className="input">
              <option value="">不分组</option>
              {groups.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.name}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* 运行状态日志 */}
        {(running || logs.length > 0) && (
          <div>
            <label className="label">状态</label>
            <div className="max-h-40 space-y-1 overflow-auto rounded-lg border border-slate-200 bg-white p-2 font-mono text-xs text-slate-600 dark:border-zinc-700 dark:bg-zinc-950 dark:text-zinc-300">
              {running && (
                <div className="flex items-center gap-2 text-brand-600 dark:text-brand-400">
                  <Loader2 size={12} className="animate-spin" />
                  监听中——请在浏览器登录 trae.cn…
                </div>
              )}
              {logs.map((l, i) => (
                <div key={i} className="text-slate-500 dark:text-zinc-400">
                  {l}
                </div>
              ))}
            </div>
          </div>
        )}

        {/* 捕获列表 */}
        {captured.length > 0 && (
          <div>
            <label className="label">本次捕获（{captured.length}）</label>
            <div className="max-h-52 space-y-1.5 overflow-auto">
              {captured.map((c, i) => (
                <div
                  key={`${c.user_id}-${i}`}
                  className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm dark:border-zinc-700 dark:bg-zinc-900"
                >
                  <div className="min-w-0">
                    <span className="font-medium">{c.name}</span>
                    <span className="ml-2 text-xs text-slate-400">{c.user_id}</span>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Badge tone={c.is_new ? 'green' : 'blue'}>
                      {c.is_new ? '新增' : '更新'}
                    </Badge>
                    <JwtStatusBadge hours={c.exp_hours} />
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
```

- [ ] **Step 5: 验证**

Run: `npx tsc --noEmit`
Expected: 无错误（Badge 的 `blue`/`green` tone 已确认存在于 [ui.tsx](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\components\ui.tsx#L5-L12) 的 Tone 类型）

- [ ] **Step 6: Commit**

```bash
git add src/pages/Accounts.tsx
git commit -m "feat(browser-extract): 账户页浏览器提取入口与实时捕获弹窗"
```

---

### Task 8: OAuth 弹窗提示与 refreshJwt 报错文案

**Files:**
- Modify: `src/pages/Accounts.tsx:1254-1256`（OAuth 弹窗 step 2 提示）
- Modify: `src-tauri/src/commands/accounts.rs:632`（refresh_jwt 报错）

- [ ] **Step 1: OAuth 弹窗 step 2 增加提示**

[Accounts.tsx:1254-1256](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src\pages\Accounts.tsx#L1254-L1256) 的提示段落：

```tsx
            <p className="mt-2 text-xs text-slate-400">
              授权完成后会自动填入。若浏览器未跳转或自动填充失败，可将地址栏完整 URL 手动复制粘贴到此处
            </p>
```

替换为：

```tsx
            <p className="mt-2 text-xs text-slate-400">
              授权完成后会自动填入。若浏览器未跳转或自动填充失败，可将地址栏完整 URL 手动复制粘贴到此处
            </p>
            <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
              若长时间停留在「认证中」（常见于指纹浏览器代理、17388 端口被占用），请关闭本弹窗，改用「浏览器提取」登录
            </p>
```

- [ ] **Step 2: refresh_jwt 无 refresh_token 报错优化**

[accounts.rs:628-632](d:\My_Codeproject\trae-daily\20260902-093039\TraeWorkAssistant-main\TraeWorkAssistant-main\src-tauri\src\commands\accounts.rs#L628-L632) 的：

```rust
    let refresh_token = account
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or("该账号无 refresh_token，无法自动刷新")?;
```

替换为：

```rust
    let refresh_token = account
        .refresh_token
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or("该账号无 refresh_token（浏览器提取的账号不支持自动刷新），请用「浏览器提取」重新获取 JWT")?;
```

- [ ] **Step 3: 验证**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 通过

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add src/pages/Accounts.tsx src-tauri/src/commands/accounts.rs
git commit -m "feat(browser-extract): OAuth 卡认证中提示引导改用浏览器提取；优化刷新报错文案"
```

---

### Task 9: 全链路验证

**Files:** 无新改动（只验证）

- [ ] **Step 1: Rust 全量测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全部 PASS（含原有 35 个 + 新增 ~13 个 browser_extract 单测）

- [ ] **Step 2: Rust 编译检查**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: 无 error

- [ ] **Step 3: 前端类型检查**

Run: `npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 4: 手动验证清单（npm run tauri dev 启动桌面窗口）**

1. 账户页出现「浏览器提取」按钮 → 点击打开弹窗
2. 选择分组 →「启动提取浏览器」→ Edge/Chrome 打开并导航到 trae.cn，弹窗显示"监听中"
3. 登录一个账号 → 弹窗实时出现捕获条目（新增徽标 + 有效期徽标），toast 提示
4. 账户列表出现该账号，昵称来自 GetUserInfo（失败则 账号_xxxxxxxx）
5. 浏览器内退出登录、登录第二个账号 → 第二条目捕获（验证多账号连续提取）
6. 「完成并关闭」→ 浏览器优雅关闭（下次打开不弹"恢复页面"）
7. 再次启动提取 → 免登录直接捕获（验证持久 profile 生效）
8. 已存在账号重复提取 → 显示"更新"徽标而非新增
9. 提取期间直接关闭弹窗（X）→ 浏览器同样被关闭
10. 提取期间用户手动关掉浏览器 → 弹窗状态日志出现"浏览器已关闭"
11. 应用运行中做提取 → 退出应用 → 浏览器进程被杀（任务管理器确认无孤儿 msedge/chrome 持有 browser_profile 目录）
12. OAuth 登录弹窗 step 2 出现"改用浏览器提取"提示

- [ ] **Step 5: 记录验证结果，完成后提交（若有微调）**

```bash
git status
# 若验证期间修复了小问题：
git add -A
git commit -m "fix(browser-extract): 验证期间修复（描述具体问题）"
```
