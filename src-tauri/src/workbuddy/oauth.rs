//! WorkBuddy OAuth 扫码登录采集（复刻官方客户端流程）。
//!
//! 流程：`oauth_start` 向官方申请 state 并返回登录页 URL，
//! 前端打开浏览器让用户扫码授权后轮询 `oauth_poll`，
//! 拿到 token 再拉取账号资料并入库。
//! 参考 changexbc/workbuddy-switch（MIT）的 modules/oauth.rs。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::accounts::{account_meta, generate_id, get_str};
use super::http::{
    http_get_json, http_post_json, WORKBUDDY_API_ENDPOINT, WORKBUDDY_API_PREFIX,
};
use super::now_ms;
use super::refresh::persist_account;

const OAUTH_TIMEOUT_SECONDS: i64 = 600;
const WORKBUDDY_PLATFORM: &str = "workbuddy";

#[derive(Default)]
struct OAuthInfo {
    state: String,
    expires_at: i64,
    done: bool,
    result: Option<Value>,
    error: Option<String>,
}

static OAUTH_STATES: OnceLock<Mutex<HashMap<String, OAuthInfo>>> = OnceLock::new();

fn oauth_states() -> &'static Mutex<HashMap<String, OAuthInfo>> {
    OAUTH_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_secs() -> i64 {
    now_ms() / 1000
}

/// 时间戳字段归一化：兼容秒/毫秒两种精度。
fn norm_ts(v: Option<Value>) -> Option<i64> {
    let n = v?.as_i64()?;
    if n > 10_000_000_000 {
        Some(n)
    } else {
        Some(n * 1000)
    }
}

/// 发起登录：向官方申请 state，返回 loginId / verificationUri / expiresIn。
pub fn oauth_start() -> Result<Value, String> {
    let url = format!(
        "{WORKBUDDY_API_ENDPOINT}{WORKBUDDY_API_PREFIX}/auth/state?platform={WORKBUDDY_PLATFORM}"
    );
    let resp = http_post_json(&url, &json!({}), &HashMap::new());
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    let state = get_str(&data, "state").unwrap_or_default();
    if state.is_empty() {
        let snippet = serde_json::to_string(&resp)
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect::<String>();
        return Err(format!("auth/state 响应缺少 state: {snippet}"));
    }
    let auth_url = get_str(&data, "authUrl")
        .or_else(|| get_str(&data, "auth_url"))
        .or_else(|| get_str(&data, "url"))
        .unwrap_or_else(|| format!("{WORKBUDDY_API_ENDPOINT}/login?state={state}"));

    // login_id：时间戳 + state 哈希前 8 位（无 uuid 依赖）
    let login_id = format!("wbo-{}", generate_id(&state));

    let mut map = oauth_states().lock().unwrap();
    map.insert(
        login_id.clone(),
        OAuthInfo {
            state,
            expires_at: now_secs() + OAUTH_TIMEOUT_SECONDS,
            ..Default::default()
        },
    );
    drop(map);

    Ok(json!({
        "loginId": login_id,
        "verificationUri": auth_url,
        "expiresIn": OAUTH_TIMEOUT_SECONDS,
    }))
}

/// 轮询一次官方 token 接口。成功则拉取账号信息并入库。
pub fn oauth_poll(login_id: &str) -> Value {
    let state = {
        let mut map = oauth_states().lock().unwrap();
        let Some(info) = map.get_mut(login_id) else {
            return json!({"done": true, "error": "登录请求不存在"});
        };
        if info.done {
            return json!({"done": true, "result": info.result.clone(), "error": info.error.clone()});
        }
        if now_secs() > info.expires_at {
            info.done = true;
            info.error = Some("登录超时".to_string());
            return json!({"done": true, "error": "登录超时"});
        }
        info.state.clone()
    };

    let url = format!("{WORKBUDDY_API_ENDPOINT}{WORKBUDDY_API_PREFIX}/auth/token?state={state}");
    let resp = http_get_json(&url, &HashMap::new());
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        return json!({"done": false});
    }
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    let access_token = get_str(&data, "accessToken")
        .or_else(|| get_str(&data, "access_token"))
        .unwrap_or_default();
    if access_token.is_empty() {
        return json!({"done": false});
    }

    // 拉取账号资料
    let account_url = format!(
        "{WORKBUDDY_API_ENDPOINT}{WORKBUDDY_API_PREFIX}/login/account?state={state}"
    );
    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {access_token}"),
    );
    let domain = get_str(&data, "domain").unwrap_or_default();
    if !domain.is_empty() {
        headers.insert("X-Domain".to_string(), domain.clone());
    }
    let acc_resp = http_get_json(&account_url, &headers);
    let acc_data = acc_resp.get("data").cloned().unwrap_or_else(|| json!({}));

    let expires_at = norm_ts(data.get("expiresAt").cloned())
        .or_else(|| data.get("expiresIn").and_then(|v| v.as_i64()).map(|e| now_ms() + e * 1000));
    let refresh_expires_at = norm_ts(
        data.get("refreshExpiresAt")
            .or_else(|| data.get("refresh_expires_at"))
            .cloned(),
    )
    .or_else(|| {
        data.get("refreshExpiresIn")
            .and_then(|v| v.as_i64())
            .map(|e| now_ms() + e * 1000)
    });

    let account = json!({
        "id": generate_id(&access_token),
        "uid": get_str(&acc_data, "uid"),
        "nickname": get_str(&acc_data, "nickname"),
        "email": get_str(&acc_data, "email"),
        "enterpriseName": get_str(&acc_data, "enterpriseName"),
        "enterpriseId": get_str(&acc_data, "enterpriseId"),
        "access_token": access_token,
        "refresh_token": get_str(&data, "refreshToken")
            .or_else(|| get_str(&data, "refresh_token")),
        "token_type": get_str(&data, "tokenType")
            .or_else(|| get_str(&data, "token_type"))
            .unwrap_or_else(|| "Bearer".to_string()),
        "domain": domain,
        "expiresAt": expires_at,
        "refreshExpiresAt": refresh_expires_at,
        "createdAt": now_ms(),
    });

    persist_account(&account);

    let result = account_meta(&account);
    let mut map = oauth_states().lock().unwrap();
    if let Some(info) = map.get_mut(login_id) {
        info.done = true;
        info.result = Some(result.clone());
    }
    drop(map);

    json!({"done": true, "result": result})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_ts_converts_seconds_to_ms() {
        assert_eq!(norm_ts(Some(json!(1_700_000_000i64))), Some(1_700_000_000_000));
        assert_eq!(
            norm_ts(Some(json!(1_700_000_000_000i64))),
            Some(1_700_000_000_000)
        );
        assert_eq!(norm_ts(None), None);
    }

    #[test]
    fn poll_unknown_login_id_returns_error() {
        let r = oauth_poll("not-exist");
        assert_eq!(r["done"], true);
        assert_eq!(r["error"], "登录请求不存在");
    }
}
