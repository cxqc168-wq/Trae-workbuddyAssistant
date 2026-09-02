use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, State};

use crate::fs_utils;
use crate::jwt;
use crate::models::{AccountsFile, RawAccount};
use crate::state::AppState;

/// OAuth 常量
const OAUTH_CLIENT_ID: &str = "en1oxy7wnw8j9n";
const OAUTH_CLIENT_SECRET: &str = "-";
const OAUTH_APP_ID: &str = "6eefa01c-1036-4c7e-9ca5-d891f63bfcd8";
const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:17388/authorize";
const OAUTH_CALLBACK_PORT: u16 = 17388;

/// OAuth 回调解析结果
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OAuthCallbackInfo {
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub avatar: Option<String>,
}

/// OAuth 登录 URL 响应
#[derive(Serialize)]
pub struct OAuthLoginUrl {
    pub url: String,
    pub state: String,
    pub redirect_uri: String,
}

/// OAuth 登录完成后的账号信息
#[derive(Serialize)]
pub struct OAuthLoginResult {
    pub user_id: String,
    pub name: String,
    pub jwt: String,
    pub refresh_token: String,
    pub has_refresh_token: bool,
}

/// 短请求 Agent
fn short_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .max_idle_connections(20)
        .max_idle_connections_per_host(20)
        .build()
}

/// 生成随机 hex 字符串
fn random_hex(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        // 简单 LCG
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let nibble = ((seed >> 32) & 0xF) as u8;
        out.push(if nibble < 10 {
            (b'0' + nibble) as char
        } else {
            (b'a' + nibble - 10) as char
        });
    }
    out
}

/// 生成 OAuth 登录 URL
#[tauri::command]
pub fn oauth_get_login_url() -> OAuthLoginUrl {
    let state = random_hex(32);
    let machine_id = random_hex(32);
    let device_id: String = (0..15).map(|_| {
        let n = (random_hex(2).chars().next().unwrap() as u8).wrapping_rem(10);
        (b'0' + n) as char
    }).collect();

    let url = format!(
        "https://www.trae.cn/authorization?\
        client_id={client_id}\
        &client_secret={client_secret}\
        &app_id={app_id}\
        &auth_callback_url={redirect_uri}\
        &state={state}\
        &machine_id={machine_id}\
        &device_id={device_id}\
        &response_type=code",
        client_id = OAUTH_CLIENT_ID,
        client_secret = OAUTH_CLIENT_SECRET,
        app_id = OAUTH_APP_ID,
        redirect_uri = urlencoding::encode(OAUTH_REDIRECT_URI),
        state = state,
        machine_id = machine_id,
        device_id = device_id,
    );

    OAuthLoginUrl {
        url,
        state,
        redirect_uri: OAUTH_REDIRECT_URI.to_string(),
    }
}

/// 解析 OAuth 回调 URL
#[tauri::command]
pub fn oauth_parse_callback(callback_url: String) -> Result<OAuthCallbackInfo, String> {
    // 回调 URL 格式：http://127.0.0.1:port/authorize?refreshToken=xxx&accessToken=xxx&userId=xxx&userName=xxx&avatar=xxx
    // 或可能带 code 参数需要交换
    let query_str = callback_url
        .split('?')
        .nth(1)
        .ok_or_else(|| "回调 URL 中缺少查询参数".to_string())?;

    let mut params: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for pair in query_str.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("").to_string();
        let value = kv.next().unwrap_or("").to_string();
        // URL decode
        let decoded = urlencoding::decode(&value)
            .map(|c| c.to_string())
            .unwrap_or(value);
        params.insert(key, decoded);
    }

    // 优先从 refreshToken 参数获取
    let refresh_token = params
        .get("refreshToken")
        .or_else(|| params.get("refresh_token"))
        .cloned()
        .ok_or_else(|| "回调 URL 中缺少 refreshToken 参数".to_string())?;

    let access_token = params
        .get("accessToken")
        .or_else(|| params.get("access_token"))
        .cloned();

    let user_id = params
        .get("userId")
        .or_else(|| params.get("user_id"))
        .or_else(|| params.get("UserID"))
        .cloned();

    let user_name = params
        .get("userName")
        .or_else(|| params.get("user_name"))
        .or_else(|| params.get("nickname"))
        .cloned();

    let avatar = params.get("avatar").cloned();

    Ok(OAuthCallbackInfo {
        refresh_token,
        access_token,
        user_id,
        user_name,
        avatar,
    })
}

/// ExchangeToken：用 refresh_token 换取 access_token
fn exchange_token(refresh_token: &str) -> Result<(String, Option<String>), String> {
    let resp = short_agent()
        .post("https://api.trae.com.cn/cloudide/api/v3/trae/oauth/ExchangeToken")
        .set("content-type", "application/json")
        .set("accept", "*/*")
        .send_json(ureq::json!({
            "ClientID": OAUTH_CLIENT_ID,
            "RefreshToken": refresh_token,
            "ClientSecret": OAUTH_CLIENT_SECRET,
            "UserID": ""
        }))
        .map_err(|e| format!("ExchangeToken 请求失败: {}", e))?;

    let body: serde_json::Value =
        resp.into_json().map_err(|e| format!("解析响应失败: {}", e))?;

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        return Err(format!("ExchangeToken 失败 (code={}): {}", code, msg));
    }

    let data = body.get("data").ok_or("响应中缺少 data 字段")?;

    let access_token = data
        .get("access_token")
        .or_else(|| data.get("token"))
        .and_then(|v| v.as_str())
        .ok_or("响应中缺少 access_token")?;

    let new_refresh_token = data
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((access_token.to_string(), new_refresh_token))
}

/// GetUserInfo：获取用户信息
fn get_user_info(access_token: &str) -> Result<(String, String), String> {
    let auth = if access_token.starts_with("Cloud-IDE-JWT ") {
        access_token.to_string()
    } else {
        format!("Cloud-IDE-JWT {}", access_token)
    };

    let resp = short_agent()
        .post("https://api.trae.com.cn/cloudide/api/v3/trae/GetUserInfo")
        .set("authorization", &auth)
        .set("content-type", "application/json")
        .set("accept", "*/*")
        .send_json(ureq::json!({}))
        .map_err(|e| format!("GetUserInfo 请求失败: {}", e))?;

    let body: serde_json::Value =
        resp.into_json().map_err(|e| format!("解析响应失败: {}", e))?;

    let data = body.get("data").or(body.get("result")).ok_or("响应中缺少 data 字段")?;

    let user_id = data
        .get("user_id")
        .or_else(|| data.get("UserID"))
        .or_else(|| data.get("userId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let user_name = data
        .get("name")
        .or_else(|| data.get("user_name"))
        .or_else(|| data.get("userName"))
        .or_else(|| data.get("nickname"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok((user_id, user_name))
}

/// OAuth 登录闭环：解析回调 → 换取 accessToken → 获取用户信息 → 保存账号
#[tauri::command]
pub fn oauth_login(
    state: State<AppState>,
    callback_url: String,
    account_name: Option<String>,
    group_id: Option<String>,
) -> Result<OAuthLoginResult, String> {
    // 1. 解析回调 URL
    let callback_info = oauth_parse_callback(callback_url)?;

    // 2. 如果回调中没有 accessToken，则用 refresh_token 换取
    let (access_token, new_refresh_token) = if let Some(ref at) = callback_info.access_token {
        (at.clone(), None)
    } else {
        exchange_token(&callback_info.refresh_token)?
    };

    // 3. 规范化 JWT 格式
    let jwt = if access_token.starts_with("Cloud-IDE-JWT ") {
        access_token.clone()
    } else {
        format!("Cloud-IDE-JWT {}", access_token)
    };

    // 4. 解析 JWT 获取 user_id
    let jwt_info = jwt::parse(&jwt);
    let user_id = callback_info
        .user_id
        .clone()
        .or_else(|| jwt_info.user_id.clone())
        .ok_or_else(|| "无法从回调或 JWT 中获取 user_id".to_string())?;

    // 5. 尝试获取用户名
    let name = account_name
        .or(callback_info.user_name.clone())
        .or_else(|| {
            // 尝试调用 GetUserInfo
            get_user_info(&jwt)
                .map(|(uid, uname)| if uname.is_empty() { uid } else { uname })
                .ok()
        })
        .unwrap_or_else(|| format!("账号_{}", &user_id[..user_id.len().min(8)]));

    // 6. 确定最终的 refresh_token（优先使用 ExchangeToken 返回的新 token）
    let final_refresh_token = new_refresh_token
        .unwrap_or_else(|| callback_info.refresh_token.clone());

    // 7. 检查账号是否已存在
    let mut accounts: AccountsFile = fs_utils::read_json(&state.path("checkin_accounts.json"));
    if accounts
        .accounts
        .iter()
        .any(|a| a.user_id.as_deref() == Some(&user_id))
    {
        // 已存在：更新 JWT 和 refresh_token
        let acct = accounts
            .accounts
            .iter_mut()
            .find(|a| a.user_id.as_deref() == Some(&user_id))
            .unwrap();
        acct.jwt = jwt.clone();
        acct.refresh_token = Some(final_refresh_token.clone());
        acct.updated_at = Some(fs_utils::now_iso());
        fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;

        fs_utils::app_log(
            &state.data_dir,
            &format!("OAuth 登录：更新已有账号 [{}] jwt + refresh_token", name),
        );
    } else {
        // 新账号
        accounts.accounts.push(RawAccount {
            name: name.clone(),
            user_id: Some(user_id.clone()),
            jwt: jwt.clone(),
            refresh_token: Some(final_refresh_token.clone()),
            added_at: Some(fs_utils::now_iso()),
            updated_at: Some(fs_utils::now_iso()),
        });
        fs_utils::write_json(&state.path("checkin_accounts.json"), &accounts)?;

        // 设置分组
        if let Some(g) = group_id {
            let mut groups: crate::models::GroupsFile =
                fs_utils::read_json(&state.path("groups.json"));
            groups.membership.insert(user_id.clone(), g);
            fs_utils::write_json(&state.path("groups.json"), &groups)?;
        }

        fs_utils::app_log(
            &state.data_dir,
            &format!("OAuth 登录：新增账号 [{}] user_id={}", name, user_id),
        );
    }

    Ok(OAuthLoginResult {
        user_id: user_id.clone(),
        name,
        jwt,
        refresh_token: final_refresh_token,
        has_refresh_token: true,
    })
}

// ==================== 本地 OAuth 回调服务器 ====================
//
// 背景：Trae 官网授权页在"认证中，正在验证身份"阶段会探测本地
// 127.0.0.1:17388 是否有客户端在监听（正常流程中 Trae 桌面端发起
// OAuth 时会启动该本地服务），确认客户端在线后才完成授权并回调。
// 此前本项目没有监听该端口，导致官网一直卡在"认证中"。
//
// 本服务器模拟 Trae 客户端行为：
// - 探测请求（无 token 参数）→ 返回 200，表示"客户端在线"
// - 授权回调（带 token 参数）→ 通过 `oauth-callback` 事件推送给前端，
//   并返回提示页面（浏览器会显示"登录成功，请返回应用"）

use axum::http::{header, HeaderValue, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// 回调服务器句柄：用于优雅停止
pub struct OAuthCallbackHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl OAuthCallbackHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
    }
}

impl Drop for OAuthCallbackHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 给响应加上 CORS 头：官网授权页可能用 fetch 从 https 跨源探测本地端口，
/// 没有这组头时浏览器会拦截探测响应，官网同样会认为客户端不在线。
fn with_cors(body: impl IntoResponse) -> Response {
    let mut resp = body.into_response();
    let headers = resp.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
    resp
}

/// 判断查询串是否携带凭证参数（区分"授权回调"与"在线探测"）
fn query_has_token(query: &str) -> bool {
    query.contains("refreshToken")
        || query.contains("refresh_token")
        || query.contains("accessToken")
        || query.contains("access_token")
        || query.contains("code=")
}

const SUCCESS_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>登录成功</title>
<style>
  body { font-family: system-ui, sans-serif; background: #f8fafc; color: #0f172a;
         display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
  .card { text-align: center; padding: 40px 48px; background: #fff; border-radius: 12px;
          box-shadow: 0 4px 16px rgba(0,0,0,.08); }
  .icon { font-size: 40px; }
  h1 { font-size: 18px; margin: 12px 0 8px; }
  p { font-size: 14px; color: #64748b; margin: 0; }
</style>
</head>
<body>
<div class="card">
  <div class="icon">&#9989;</div>
  <h1>OAuth 登录成功</h1>
  <p>已捕获登录凭证，请返回「Trae Work 助手」完成账号保存。</p>
</div>
</body>
</html>"#;

/// /authorize 处理：带凭证 → 通知前端；无凭证 → 在线探测应答
async fn authorize_handler(uri: Uri, app: tauri::AppHandle) -> Response {
    let query = uri.query().unwrap_or("");
    if !query_has_token(query) {
        // 官网页面的在线探测请求：返回 200 表示"客户端在线"
        return with_cors("ok");
    }
    // 授权回调：把完整 URL（含凭证参数）推给前端自动完成登录
    let full_url = format!(
        "http://127.0.0.1:{}/authorize?{}",
        OAUTH_CALLBACK_PORT, query
    );
    let _ = app.emit("oauth-callback", full_url);
    with_cors(Html(SUCCESS_HTML))
}

/// 兜底：官网可能探测其它路径，一律应答 200 保持"在线"
async fn probe_fallback() -> Response {
    with_cors("ok")
}

/// 启动本地 OAuth 回调服务器（幂等：已在监听则直接成功）
#[tauri::command]
pub async fn oauth_callback_start(
    app: tauri::AppHandle,
    runtime: State<'_, Mutex<Option<OAuthCallbackHandle>>>,
) -> Result<(), String> {
    {
        let guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
    }

    let addr = format!("127.0.0.1:{}", OAUTH_CALLBACK_PORT);
    let listener = TcpListener::bind(&addr).await.map_err(|e| {
        format!(
            "本地回调端口 {} 绑定失败: {}（可能被 Trae 客户端或其它程序占用）",
            OAUTH_CALLBACK_PORT, e
        )
    })?;

    let app_for_handler = app.clone();
    let router = Router::new()
        .route(
            "/authorize",
            get(move |uri: Uri| authorize_handler(uri, app_for_handler.clone()))
                .options(|| async { with_cors("") }),
        )
        .fallback(probe_fallback);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });
    let join_handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("OAuth callback server error: {}", e);
        }
    });

    let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(OAuthCallbackHandle {
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    });
    Ok(())
}

/// 停止本地 OAuth 回调服务器（未运行则为空操作）
#[tauri::command]
pub fn oauth_callback_stop(runtime: State<'_, Mutex<Option<OAuthCallbackHandle>>>) {
    let mut guard = runtime.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut h) = guard.take() {
        h.stop();
    }
}
