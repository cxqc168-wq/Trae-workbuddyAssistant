//! WorkBuddy Tauri 命令层。

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::workbuddy::accounts::{account_meta, get_str};
use crate::workbuddy::auth_file;
use crate::workbuddy::checkin;
use crate::workbuddy::credits;
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
    let path = crate::workbuddy::store_path().join("workbuddy_accounts.json");
    let mut list = crate::workbuddy::accounts::load_accounts_from_path(&path);
    let before = list.len();
    list.retain(|a| a.get("id").and_then(|v| v.as_str()) != Some(account_id.as_str()));
    if list.len() == before {
        return Err("账号不存在".into());
    }
    crate::workbuddy::accounts::save_accounts_to_path(&path, &list).map_err(|e| e.to_string())
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
    let results = checkin::checkin_all(account_ids.as_deref());
    for (i, r) in results.iter().enumerate() {
        let _ = app.emit("workbuddy-checkin-progress", json!({
            "index": i,
            "total": results.len(),
            "accountId": r.get("accountId"),
            "email": r.get("email"),
            "result": r.get("result"),
            "error": r.get("error"),
        }));
    }
    let ok = results.iter().filter(|r| r["result"] == "success").count();
    let already = results.iter().filter(|r| r["result"] == "already").count();
    let failed = results.len() - ok - already;
    let _ = app.emit("workbuddy-checkin-done", json!({"ok": ok, "already": already, "failed": failed, "total": results.len()}));
    Ok(results)
}

#[tauri::command(async)]
pub fn workbuddy_credits(account_id: Option<String>) -> Result<Vec<Value>, String> {
    let accounts: Vec<Value> = load_all().into_iter()
        .filter(|a| account_id.as_deref().map_or(true, |id| a.get("id").and_then(|v| v.as_str()) == Some(id)))
        .collect();
    if accounts.is_empty() {
        return Err("没有匹配的账号".into());
    }
    Ok(accounts.iter().map(credits::get_credit_expiry).collect())
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
