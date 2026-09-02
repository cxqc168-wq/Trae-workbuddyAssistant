//! WorkBuddy API 统一 HTTP 入口（ureq 同步）。

use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use super::accounts::get_str;

pub const WORKBUDDY_API_ENDPOINT: &str = "https://www.codebuddy.cn";
pub const WORKBUDDY_API_PREFIX: &str = "/v2/plugin";
pub const CHECKIN_API_PREFIX: &str = "/v2/billing/meter";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
}

/// 构造认证头（对齐官方客户端）。
pub fn build_auth_headers(account: &Value) -> HashMap<String, String> {
    let mut h = HashMap::new();
    h.insert(
        "Authorization".into(),
        format!("Bearer {}", get_str(account, "access_token").unwrap_or_default()),
    );
    h.insert("Accept".into(), "application/json".into());
    h.insert("Content-Type".into(), "application/json".into());
    if let Some(uid) = get_str(account, "uid") {
        h.insert("X-User-Id".into(), uid);
    }
    if let Some(eid) = get_str(account, "enterpriseId") {
        h.insert("X-Enterprise-Id".into(), eid.clone());
        h.insert("X-Tenant-Id".into(), eid);
    }
    if let Some(d) = get_str(account, "domain") {
        h.insert("X-Domain".into(), d);
    }
    h
}

/// POST JSON；任何失败都返回形状化 JSON（含 code/message），不 panic。
pub fn http_post_json(url: &str, body: &Value, headers: &HashMap<String, String>) -> Value {
    let mut req = agent().post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    match req.send_json(body.clone()) {
        Ok(resp) => resp
            .into_string()
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({"code": -1, "message": "响应解析失败"})),
        Err(ureq::Error::Status(code, resp)) => resp
            .into_string()
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| serde_json::json!({"code": code, "message": format!("HTTP {code}")})),
        Err(e) => serde_json::json!({"code": -1, "message": format!("网络错误: {e}")}),
    }
}

/// 判断是否因 token 失效被拒。
pub fn is_unauthorized(resp: &Value) -> bool {
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code == 401 || code == 403 {
        return true;
    }
    let msg = resp
        .get("message")
        .or_else(|| resp.get("msg"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    ["unauthorized", "401", "登录", "失效", "过期", "token"]
        .iter()
        .any(|k| msg.contains(k))
}
