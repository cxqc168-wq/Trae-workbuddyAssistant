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

/// 发签到请求；未授权且有 refresh token 时刷新重试一次。
fn checkin_request(path: &str, account: &Value) -> Value {
    let url = format!("{WORKBUDDY_API_ENDPOINT}{path}");
    let mut resp = http_post_json(&url, &json!({}), &build_auth_headers(account));
    if is_unauthorized(&resp) && !get_str(account, "refresh_token").unwrap_or_default().is_empty() {
        let refreshed = refresh_account_token(account.clone());
        persist_account(&refreshed);
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

/// 执行签到；"已签到"按成功。
pub fn perform_checkin(account: &Value) -> Value {
    let resp = checkin_request(&format!("{CHECKIN_API_PREFIX}/daily-checkin"), account);
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code == 0 || code == 200 {
        return json!({"ok": true});
    }
    let msg = resp.get("message").or_else(|| resp.get("msg"))
        .and_then(|v| v.as_str()).unwrap_or(&format!("code={code}")).to_string();
    if msg.contains("已签到") || msg.to_lowercase().contains("repeat") {
        return json!({"ok": true, "already": true, "message": msg});
    }
    json!({"ok": false, "error": msg})
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
    {
        let mut running = accounts_running().lock().unwrap();
        if !running.insert(key.clone()) {
            return json!({"result": "error", "error": "该账号正在签到，请稍后再试"});
        }
    }
    let result = checkin_account_inner(account);
    accounts_running().lock().unwrap().remove(&key);
    result
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

/// 一键全签（或指定 ids）。返回逐账号结果数组。
pub fn checkin_all(ids: Option<&[String]>) -> Vec<Value> {
    let accounts = load_accounts();
    let mut out = Vec::new();
    for acc in accounts {
        if let Some(ids) = ids {
            let id = acc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !ids.iter().any(|x| x == id) {
                continue;
            }
        }
        let r = checkin_account(&acc);
        out.push(json!({
            "accountId": acc.get("id"),
            "email": account_display_name(&acc),
            "result": r.get("result"),
            "error": r.get("error"),
        }));
    }
    out
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
    fn perform_checkin_already_message() {
        // 不联网路径无法直接测 perform_checkin（会发请求）；此测试仅验证 decide 层语义完整性
        let status = json!({"ok": true, "todayCheckedIn": true});
        assert_eq!(decide_from_status(&status), StatusDecision::Already);
    }
}
