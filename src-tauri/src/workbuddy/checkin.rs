//! 签到：状态查询 / 执行 / 单账号流程 / 一键全签。

use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use super::accounts::{account_display_name, get_str};
use super::http::{build_auth_headers, http_post_json, is_unauthorized, CHECKIN_API_PREFIX, WORKBUDDY_API_ENDPOINT};
use super::refresh::{ensure_fresh_token, load_accounts, persist_account, refresh_account_token};

fn accounts_running() -> &'static Mutex<HashSet<String>> {
    static M: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashSet::new()))
}

fn log_write_lock() -> &'static Mutex<()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
}

/// 账号运行守卫：离开作用域自动从 running 集合移除，防止 panic 泄漏。
struct AccountRunGuard {
    key: String,
}

impl AccountRunGuard {
    fn acquire(key: String) -> Option<Self> {
        let mut running = accounts_running().lock().unwrap();
        if running.insert(key.clone()) {
            Some(Self { key })
        } else {
            None
        }
    }
}

impl Drop for AccountRunGuard {
    fn drop(&mut self) {
        accounts_running().lock().unwrap().remove(&self.key);
    }
}

/// 发签到请求；未授权且有 refresh token 时刷新重试一次。
fn checkin_request(path: &str, account: &Value) -> Value {
    let url = format!("{WORKBUDDY_API_ENDPOINT}{path}");
    let mut resp = http_post_json(&url, &json!({}), &build_auth_headers(account));
    if is_unauthorized(&resp) && !get_str(account, "refresh_token").unwrap_or_default().is_empty() {
        let refreshed = refresh_account_token(account.clone());
        resp = http_post_json(&url, &json!({}), &build_auth_headers(&refreshed));
    }
    resp
}

/// 查签到状态（新接口失败回退旧接口）。
pub fn get_checkin_status(account: &Value) -> Value {
    let resp = checkin_request(&format!("{CHECKIN_API_PREFIX}/checkin-activity-status"), account);
    if let Some(v) = status_from_response(&resp) {
        return v;
    }
    let resp2 = checkin_request(&format!("{CHECKIN_API_PREFIX}/checkin-status"), account);
    if let Some(v) = status_from_response(&resp2) {
        return v;
    }
    json!({
        "ok": false,
        "error": resp2.get("message").or_else(|| resp2.get("msg"))
            .and_then(|v| v.as_str()).unwrap_or("查询签到状态失败").to_string(),
    })
}

fn status_from_response(resp: &Value) -> Option<Value> {
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        return None;
    }
    let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
    Some(json!({
        "ok": true,
        "todayCheckedIn": data.get("today_checked_in")
            .or_else(|| data.get("todayCheckedIn"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }))
}

/// 判定签到响应：Ok(已签到按 already 成功) / Err(错误消息)。
fn checkin_result_from_response(resp: &Value) -> Result<bool, String> {
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code == 0 || code == 200 {
        return Ok(false);
    }
    let msg = resp.get("message").or_else(|| resp.get("msg"))
        .and_then(|v| v.as_str()).unwrap_or(&format!("code={code}")).to_string();
    if msg.contains("已签到") || msg.to_lowercase().contains("repeat") {
        return Ok(true);
    }
    Err(msg)
}

/// 执行签到；"已签到"按成功。
pub fn perform_checkin(account: &Value) -> Value {
    let resp = checkin_request(&format!("{CHECKIN_API_PREFIX}/daily-checkin"), account);
    match checkin_result_from_response(&resp) {
        Ok(false) => json!({"ok": true}),
        Ok(true) => json!({"ok": true, "already": true, "message": "今日已签到"}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// 状态三态判定。
#[derive(Debug, PartialEq)]
pub enum StatusDecision {
    Already,
    Submit,
    Error(String),
}

pub fn decide_from_status(status: &Value) -> StatusDecision {
    if status.get("ok").and_then(Value::as_bool) != Some(true) {
        return StatusDecision::Error(status.get("error").and_then(Value::as_str).unwrap_or("查询签到状态失败").to_string());
    }
    if status.get("todayCheckedIn").and_then(Value::as_bool) == Some(true) {
        StatusDecision::Already
    } else {
        StatusDecision::Submit
    }
}

/// 追加签到日志（JSON 数组文件 + 文本日志）。
fn add_checkin_log(entry: &Value) {
    let _lock = log_write_lock().lock().unwrap();
    let dir = super::store_path();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("workbuddy_checkin_logs.json");
    let mut logs = super::accounts::load_accounts_from_path(&path);
    logs.push(entry.clone());
    let _ = super::accounts::save_accounts_to_path(&path, &logs);
    let text = dir.join("logs").join("workbuddy_checkin.log");
    let _ = std::fs::create_dir_all(dir.join("logs"));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&text) {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), entry);
    }
}

/// 单账号完整签到：刷新 → 查状态 → 未签则提交 → 记日志。
pub fn checkin_account(account: &Value) -> Value {
    let key = get_str(account, "id").unwrap_or_else(|| account_display_name(account));
    let Some(_guard) = AccountRunGuard::acquire(key) else {
        return json!({"result": "error", "error": "该账号正在签到，请稍后再试"});
    };
    checkin_account_inner(account)
}

fn checkin_account_inner(account: &Value) -> Value {
    let acc = ensure_fresh_token(account.clone());
    persist_account(&acc);
    let status = get_checkin_status(&acc);
    match decide_from_status(&status) {
        StatusDecision::Already => return json!({"result": "already"}),
        StatusDecision::Error(e) => return json!({"result": "error", "error": e}),
        StatusDecision::Submit => {}
    }
    let res = perform_checkin(&acc);
    let result = if res.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        if res.get("already").and_then(|v| v.as_bool()) == Some(true) { "already" } else { "success" }
    } else { "error" };
    let mut entry = json!({
        "result": result,
        "ts": super::now_ms(),
        "accountId": acc.get("id"),
        "email": account_display_name(&acc),
    });
    if result == "error" {
        entry["error"] = res.get("error").cloned().unwrap_or(Value::Null);
    }
    add_checkin_log(&entry);
    json!({"result": result, "error": entry.get("error").cloned().unwrap_or(Value::Null)})
}

/// 一键全签（或指定 ids）。on_result 在每账号完成时回调（index, total, 结果对象）。
pub fn checkin_all_with<F: FnMut(usize, usize, &Value)>(ids: Option<&[String]>, mut on_result: F) -> Vec<Value> {
    let accounts = load_accounts();
    let selected: Vec<Value> = accounts
        .into_iter()
        .filter(|acc| match ids {
            Some(ids) => ids.iter().any(|x| x == acc.get("id").and_then(|v| v.as_str()).unwrap_or("")),
            None => true,
        })
        .collect();
    let total = selected.len();
    let mut out = Vec::new();
    for (i, acc) in selected.into_iter().enumerate() {
        let r = checkin_account(&acc);
        let entry = json!({
            "accountId": acc.get("id"),
            "email": account_display_name(&acc),
            "result": r.get("result"),
            "error": r.get("error"),
        });
        on_result(i, total, &entry);
        out.push(entry);
    }
    out
}

/// 无回调包装：命令层已改用 checkin_all_with 实时推送，保留给潜在调用方/测试。
#[allow(dead_code)]
pub fn checkin_all(ids: Option<&[String]>) -> Vec<Value> {
    checkin_all_with(ids, |_, _, _| {})
}

/// 今日是否已签（供账号列表展示）：该账号当天最新日志为 success/already。
pub fn checked_in_today(account: &Value) -> bool {
    let id = match account.get("id").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return false,
    };
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let logs = super::accounts::load_accounts_from_path(&super::store_path().join("workbuddy_checkin_logs.json"));
    logs.iter()
        .rev()
        .filter(|e| e.get("accountId").and_then(|v| v.as_str()) == Some(id.as_str()))
        .find_map(|e| {
            let ts = e.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
            let date = chrono::DateTime::from_timestamp_millis(ts)
                .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            (date == today).then(|| e.get("result").and_then(|v| v.as_str()).map(|s| s.to_string()))
        })
        .flatten()
        .is_some_and(|r| r == "success" || r == "already")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_already() {
        assert_eq!(decide_from_status(&json!({"ok": true, "todayCheckedIn": true})), StatusDecision::Already);
    }
    #[test]
    fn decide_error() {
        assert_eq!(
            decide_from_status(&json!({"ok": false, "error": "offline"})),
            StatusDecision::Error("offline".into())
        );
    }
    #[test]
    fn decide_submit() {
        assert_eq!(decide_from_status(&json!({"ok": true, "todayCheckedIn": false})), StatusDecision::Submit);
    }
    #[test]
    fn checkin_result_success() {
        assert_eq!(checkin_result_from_response(&json!({"code": 0})), Ok(false));
        assert_eq!(checkin_result_from_response(&json!({"code": 200, "data": {}})), Ok(false));
    }

    #[test]
    fn checkin_result_already() {
        assert_eq!(checkin_result_from_response(&json!({"code": 500, "message": "今日已签到"})), Ok(true));
        assert_eq!(checkin_result_from_response(&json!({"code": 10001, "msg": "repeat checkin"})), Ok(true));
    }

    #[test]
    fn checkin_result_error() {
        assert_eq!(checkin_result_from_response(&json!({"code": 500, "message": "内部错误"})), Err("内部错误".to_string()));
        assert_eq!(checkin_result_from_response(&json!({})), Err("code=-1".to_string()));
    }

    #[test]
    fn status_response_fallback_fields() {
        let resp = json!({"code": 0, "data": {"todayCheckedIn": true}});
        assert_eq!(status_from_response(&resp).unwrap()["todayCheckedIn"], true);
        let resp2 = json!({"code": 0, "data": {"today_checked_in": true}});
        assert_eq!(status_from_response(&resp2).unwrap()["todayCheckedIn"], true);
    }
}
