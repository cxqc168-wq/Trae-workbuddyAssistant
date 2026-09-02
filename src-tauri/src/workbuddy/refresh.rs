//! token 刷新与惰性刷新。

use serde_json::{json, Value};
use std::path::PathBuf;

use super::accounts::{get_str, load_accounts_from_path, save_accounts_to_path, upsert_collected_account};
use super::http::{build_auth_headers, http_post_json, WORKBUDDY_API_ENDPOINT, WORKBUDDY_API_PREFIX};

fn accounts_file() -> PathBuf {
    super::store_path().join("workbuddy_accounts.json")
}

pub fn load_accounts() -> Vec<Value> {
    load_accounts_from_path(&accounts_file())
}

pub fn persist_account(acc: &Value) {
    let mut list = load_accounts();
    upsert_collected_account(&mut list, acc.clone());
    let _ = save_accounts_to_path(&accounts_file(), &list);
}

/// 惰性刷新阈值（小时）。
const LAZY_REFRESH_HOURS: i64 = 24;

/// 刷新 token；成功更新并落盘，失败标记 needs_relogin。
pub fn refresh_account_token(mut account: Value) -> Value {
    let rt = get_str(&account, "refresh_token").unwrap_or_default();
    if rt.is_empty() {
        account["needs_relogin"] = json!(true);
        account["needs_relogin_reason"] = json!("缺少 refresh token，无法刷新，需重新导入账号");
        persist_account(&account);
        return account;
    }
    let mut headers = build_auth_headers(&account);
    headers.insert("X-Refresh-Token".into(), rt);
    let url = format!("{WORKBUDDY_API_ENDPOINT}{WORKBUDDY_API_PREFIX}/auth/token/refresh");
    let resp = http_post_json(&url, &json!({}), &headers);
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        account["needs_relogin"] = json!(true);
        account["needs_relogin_reason"] = json!(format!(
            "刷新失败(code={code}): {}",
            resp.get("message").or_else(|| resp.get("msg")).and_then(|v| v.as_str()).unwrap_or("未知错误")
        ));
        persist_account(&account);
        return account;
    }
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    let Some(new_at) = get_str(&data, "accessToken").or_else(|| get_str(&data, "access_token")) else {
        account["needs_relogin"] = json!(true);
        account["needs_relogin_reason"] = json!("刷新响应缺少 accessToken");
        persist_account(&account);
        return account;
    };
    account["access_token"] = json!(new_at);
    if let Some(new_rt) = get_str(&data, "refreshToken").or_else(|| get_str(&data, "refresh_token")) {
        account["refresh_token"] = json!(new_rt);
    }
    if let Some(exp) = data.get("expiresIn").and_then(|v| v.as_i64()) {
        account["expiresAt"] = json!(super::now_ms() + exp * 1000);
    }
    if let Some(exp) = data.get("refreshExpiresIn").and_then(|v| v.as_i64()) {
        account["refreshExpiresAt"] = json!(super::now_ms() + exp * 1000);
    }
    account["refreshedAt"] = json!(super::now_ms());
    if let Some(map) = account.as_object_mut() {
        map.remove("needs_relogin");
        map.remove("needs_relogin_reason");
    }
    persist_account(&account);
    account
}

/// 惰性刷新：过期或临期（< LAZY_REFRESH_HOURS）且有 refresh token 时刷新。
pub fn ensure_fresh_token(mut account: Value) -> Value {
    let exp = account.get("expiresAt").and_then(|v| v.as_i64());
    let stale = match exp {
        Some(e) => super::now_ms() >= e || e - super::now_ms() < LAZY_REFRESH_HOURS * 3600 * 1000,
        None => true,
    };
    let has_rt = !get_str(&account, "refresh_token").unwrap_or_default().is_empty();
    if stale && has_rt {
        account = refresh_account_token(account);
    }
    account
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_rt_marks_relogin() {
        // 无 rt：不联网路径，标记后落盘到 store_path（测试环境为真实 APPDATA，
        // 用无意义 id 避免污染；断言仅看返回值语义）
        let acc = refresh_account_token(json!({"id": "test-missing-rt", "access_token": "a"}));
        assert_eq!(acc["needs_relogin"], true);
        // 清理测试写入的账号，避免污染真实数据目录
        let list = load_accounts();
        let filtered: Vec<Value> = list.into_iter().filter(|a| a.get("id") != Some(&json!("test-missing-rt"))).collect();
        let _ = save_accounts_to_path(&accounts_file(), &filtered);
    }

    #[test]
    fn ensure_fresh_skips_when_valid() {
        let exp = super::super::now_ms() + 7 * 24 * 3600 * 1000;
        let acc = ensure_fresh_token(json!({"id": "t2", "expiresAt": exp, "access_token": "a"}));
        assert!(acc.get("needs_relogin").is_none());
    }

    #[test]
    fn ensure_fresh_noop_when_expired_no_rt() {
        // 过期但无 rt：不触发刷新（刷新会标 needs_relogin），原样返回
        let acc = ensure_fresh_token(json!({"id": "t3", "expiresAt": 1, "access_token": "a"}));
        assert!(acc.get("needs_relogin").is_none());
    }
}
