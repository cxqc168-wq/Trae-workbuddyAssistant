//! WorkBuddy Tauri 命令层。

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::workbuddy::accounts::{account_meta, get_str};
use crate::workbuddy::auth_file;
use crate::workbuddy::checkin;
use crate::workbuddy::credits;
use crate::workbuddy::oauth;
use crate::workbuddy::refresh;

fn load_all() -> Vec<Value> {
    refresh::load_accounts()
}

#[tauri::command]
pub fn workbuddy_list_accounts() -> Vec<Value> {
    load_all().iter().map(|a| {
        let mut m = account_meta(a);
        m["checkedToday"] = json!(checkin::checked_in_today(a));
        m
    }).collect()
}

/// 检测本机 WorkBuddy 客户端登录态（基于官方认证文件，只读，不导入）。
#[tauri::command]
pub fn workbuddy_client_status() -> Value {
    json!({ "loggedIn": auth_file::import_from_auth_file().is_some() })
}

#[tauri::command(async)]
pub fn workbuddy_import_local() -> Result<Value, String> {
    let acc = auth_file::import_from_auth_file()
        .ok_or("未读取到本地 WorkBuddy 登录信息（需已安装并登录 WorkBuddy 客户端）")?;
    refresh::persist_account(&acc);
    Ok(account_meta(&acc))
}

#[derive(serde::Deserialize)]
pub struct ManualAccountArgs {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[tauri::command]
pub fn workbuddy_add_manual(args: ManualAccountArgs) -> Result<Value, String> {
    let at = args.access_token.trim().to_string();
    if at.is_empty() {
        return Err("access_token 不能为空".into());
    }
    let acc = json!({
        "id": crate::workbuddy::accounts::generate_id(&at),
        "uid": args.uid,
        "nickname": args.nickname,
        "access_token": at,
        "refresh_token": args.refresh_token,
        "token_type": "Bearer",
        "createdAt": crate::workbuddy::now_ms(),
    });
    refresh::persist_account(&acc);
    Ok(account_meta(&acc))
}

#[tauri::command]
pub fn workbuddy_delete_account(account_id: String) -> Result<(), String> {
    refresh::delete_account(&account_id)
}

#[tauri::command(async)]
pub fn workbuddy_checkin_status(account_id: String) -> Result<Value, String> {
    let acc = load_all().into_iter()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id.as_str()))
        .ok_or("账号不存在")?;
    let acc = refresh::ensure_fresh_token(acc);
    refresh::persist_account(&acc);
    Ok(checkin::get_checkin_status(&acc))
}

#[tauri::command(async)]
pub fn workbuddy_checkin_all(app: AppHandle, account_ids: Option<Vec<String>>) -> Result<Vec<Value>, String> {
    let results = checkin::checkin_all_with(account_ids.as_deref(), |index, total, entry| {
        let _ = app.emit("workbuddy-checkin-progress", json!({
            "index": index,
            "total": total,
            "accountId": entry.get("accountId"),
            "email": entry.get("email"),
            "result": entry.get("result"),
            "error": entry.get("error"),
        }));
    });
    let ok = results.iter().filter(|r| r["result"] == "success").count();
    let already = results.iter().filter(|r| r["result"] == "already").count();
    let failed = results.len() - ok - already;
    let _ = app.emit("workbuddy-checkin-done", json!({"ok": ok, "already": already, "failed": failed, "total": results.len()}));
    Ok(results)
}

/// 积分查询内存缓存：key(accountId 或 "*") → (查询时间戳, 结果)。
/// 60 秒内复用，避免切页/重复点击反复打积分接口；force=true 时绕过缓存。
fn credits_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, (i64, Vec<Value>)>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (i64, Vec<Value>)>>> =
        std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

const CREDITS_CACHE_TTL_MS: i64 = 60_000;

#[tauri::command(async)]
pub fn workbuddy_credits(account_id: Option<String>, force: Option<bool>) -> Result<Vec<Value>, String> {
    let accounts: Vec<Value> = load_all().into_iter()
        .filter(|a| account_id.as_deref().map_or(true, |id| a.get("id").and_then(|v| v.as_str()) == Some(id)))
        .collect();
    if accounts.is_empty() {
        return Err("没有匹配的账号".into());
    }
    let key = account_id.unwrap_or_else(|| "*".into());
    let now = crate::workbuddy::now_ms();
    if !force.unwrap_or(false) {
        if let Some((ts, cached)) = credits_cache().lock().unwrap().get(&key).cloned() {
            if now - ts < CREDITS_CACHE_TTL_MS {
                return Ok(cached);
            }
        }
    }
    // 账号间并行查询（每个账号内部三接口已并行）：3 账号从串行 3RTT 降到 1RTT
    let results: Vec<Value> = std::thread::scope(|s| {
        let handles: Vec<_> = accounts
            .iter()
            .map(|a| s.spawn(|| credits::get_credit_expiry(a)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| json!({"ok": false, "error": "积分查询线程异常"})))
            .collect()
    });
    credits_cache().lock().unwrap().insert(key, (now, results.clone()));
    Ok(results)
}

#[tauri::command(async)]
pub fn workbuddy_refresh_token(account_id: String) -> Result<Value, String> {
    let acc = load_all().into_iter()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id.as_str()))
        .ok_or("账号不存在")?;
    if get_str(&acc, "refresh_token").unwrap_or_default().is_empty() {
        return Err("该账号没有 refresh_token".into());
    }
    Ok(account_meta(&refresh::refresh_account_token(acc)))
}

/// 发起 OAuth 扫码登录：返回 loginId + 浏览器打开的验证页地址。
#[tauri::command(async)]
pub fn workbuddy_oauth_start() -> Result<Value, String> {
    oauth::oauth_start()
}

/// 轮询扫码登录结果；授权完成后自动采集账号入库。
#[tauri::command(async)]
pub fn workbuddy_oauth_poll(login_id: String) -> Result<Value, String> {
    Ok(oauth::oauth_poll(&login_id))
}
