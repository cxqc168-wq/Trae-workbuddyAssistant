//! 读取 WorkBuddy 客户端官方认证文件（只读，不写入）。

use serde_json::{json, Value};
use std::path::PathBuf;

use super::accounts::{generate_id, get_str};

/// 官方认证文件路径。
pub fn auth_file_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    return dirs_home().join("Library/Application Support/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info");
    #[cfg(target_os = "windows")]
    return local_appdata()
        .join("CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return dirs_home().join(".local/share/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info");
}

#[cfg(not(target_os = "windows"))]
fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn local_appdata() -> PathBuf {
    std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default()
}

fn read_auth_file() -> Option<Value> {
    let path = auth_file_path();
    if !path.exists() {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// 字符串/数字时间戳转 i64（不换算单位）。
fn parse_ts(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

/// 从认证文件根对象提取账号（字段链与官方客户端一致）。无 access_token 返回 None。
pub fn imported_account_from_root(root: &Value) -> Option<Value> {
    let account_obj = root.get("account").cloned().unwrap_or_else(|| json!({}));
    let auth_obj = root.get("auth").cloned().unwrap_or_else(|| json!({}));

    let uid = get_str(root, "uid")
        .or_else(|| get_str(&account_obj, "uid"))
        .or_else(|| get_str(&account_obj, "id"));
    let nickname = get_str(root, "nickname")
        .or_else(|| get_str(root, "name"))
        .or_else(|| get_str(&account_obj, "nickname"))
        .or_else(|| get_str(&account_obj, "label"));
    let email = get_str(root, "email")
        .or_else(|| get_str(&account_obj, "email"))
        .or_else(|| get_str(&auth_obj, "email"));
    let access_token = get_str(&auth_obj, "accessToken")
        .or_else(|| get_str(&auth_obj, "access_token"))
        .or_else(|| get_str(root, "accessToken"))
        .or_else(|| get_str(root, "access_token"))?;
    let refresh_token = get_str(&auth_obj, "refreshToken")
        .or_else(|| get_str(&auth_obj, "refresh_token"))
        .or_else(|| get_str(root, "refreshToken"))
        .or_else(|| get_str(root, "refresh_token"));
    let token_type = get_str(&auth_obj, "tokenType")
        .or_else(|| get_str(&auth_obj, "token_type"))
        .unwrap_or_else(|| "Bearer".to_string());
    let domain = get_str(root, "domain").or_else(|| get_str(&auth_obj, "domain"));
    let expires_at = parse_ts(root.get("expiresAt").or_else(|| auth_obj.get("expiresAt")));
    let refresh_expires_at = parse_ts(
        root.get("refreshExpiresAt")
            .or_else(|| auth_obj.get("refreshExpiresAt")),
    );

    Some(json!({
        "id": generate_id(&access_token),
        "uid": uid,
        "nickname": nickname,
        "email": email,
        "enterpriseName": get_str(root, "enterpriseName")
            .or_else(|| get_str(&account_obj, "enterpriseName")),
        "enterpriseId": get_str(root, "enterpriseId")
            .or_else(|| get_str(&account_obj, "enterpriseId")),
        "access_token": access_token,
        "refresh_token": refresh_token,
        "token_type": token_type,
        "domain": domain,
        "expiresAt": expires_at,
        "refreshExpiresAt": refresh_expires_at,
        "createdAt": super::now_ms(),
    }))
}

/// 从本机当前登录态导入。
pub fn import_from_auth_file() -> Option<Value> {
    imported_account_from_root(&read_auth_file()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fields_from_root() {
        let root = json!({
            "account": {"uid": "u-1", "nickname": "小明", "email": "a@b.c"},
            "auth": {"accessToken": "AT", "refreshToken": "RT", "expiresAt": "1791912333558"},
        });
        let acc = imported_account_from_root(&root).unwrap();
        assert_eq!(acc["uid"], "u-1");
        assert_eq!(acc["access_token"], "AT");
        assert_eq!(acc["refresh_token"], "RT");
        assert_eq!(acc["expiresAt"], 1791912333558_i64);
        assert!(acc["id"].as_str().unwrap().starts_with("wb-"));
    }

    #[test]
    fn missing_token_returns_none() {
        assert!(imported_account_from_root(&json!({"account": {"uid": "u"}})).is_none());
    }

    #[test]
    fn string_ts_parse() {
        assert_eq!(parse_ts(Some(&json!("1786728333"))), Some(1786728333));
        assert_eq!(parse_ts(Some(&json!(123_i64))), Some(123));
        assert_eq!(parse_ts(Some(&json!("xx"))), None);
    }
}
