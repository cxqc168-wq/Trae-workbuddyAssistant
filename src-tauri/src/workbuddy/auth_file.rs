//! 读取 / 备份 / 切换 WorkBuddy 客户端官方认证文件。
//!
//! 官方登录文件（国内版）：%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info
//! 本应用把它按 <uid>.info 备份到 data/wb_auth/，切换时原子写回官方固定文件。

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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

// ==================== 客户端登录态备份与切换（能力 A） ====================

/// 官方登录文件的结构化信息（脱敏，可回传前端）。
#[derive(Serialize, Clone)]
pub struct WbAuthInfo {
    pub uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uin: Option<String>,
}

/// 已备份登录文件的元信息（不含 token）。
#[derive(Serialize, Clone)]
pub struct StoredAuthMeta {
    pub uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub file_name: String,
    pub size_bytes: u64,
    pub last_modified: String,
}

/// 本应用托管的登录备份目录：%APPDATA%\TraeWorkAssistant\data\wb_auth\
fn auth_store_dir() -> PathBuf {
    super::store_path().join("wb_auth")
}

/// 校验 uid 可安全作为文件名（防路径穿越）。
fn safe_uid(uid: &str) -> Result<(), String> {
    if uid.is_empty()
        || uid.len() > 200
        || uid.contains('/')
        || uid.contains('\\')
        || uid.contains('\0')
        || uid == "."
        || uid == ".."
    {
        return Err("非法的 uid".into());
    }
    Ok(())
}

/// 从官方登录文件根对象提取结构化信息（只取身份字段，不取 token）。
pub fn parse_auth_info(root: &Value) -> Option<WbAuthInfo> {
    let account = root.get("account").cloned().unwrap_or_else(|| json!({}));
    let uid = get_str(root, "uid")
        .or_else(|| get_str(&account, "uid"))
        .or_else(|| get_str(&account, "id"))?;
    Some(WbAuthInfo {
        uid,
        nickname: get_str(root, "nickname")
            .or_else(|| get_str(&account, "nickname"))
            .or_else(|| get_str(&account, "label")),
        email: get_str(root, "email").or_else(|| get_str(&account, "email")),
        uin: get_str(&account, "uin"),
    })
}

/// 读取当前官方登录文件的结构化信息。
pub fn read_current_auth_info() -> Option<WbAuthInfo> {
    parse_auth_info(&read_auth_file()?)
}

/// 原子写文件：写临时文件 → rename 替换目标，避免写一半损坏官方登录文件。
fn write_atomic(target: &Path, content: &[u8]) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let tmp = target.with_extension(format!(
        "info.tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    std::fs::write(&tmp, content).map_err(|e| format!("写入临时文件失败: {e}"))?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("原子替换失败: {e}")
    })?;
    Ok(())
}

/// 把当前官方登录文件备份到 data/wb_auth/<uid>.info，返回账号信息。
pub fn backup_current_auth() -> Result<WbAuthInfo, String> {
    let path = auth_file_path();
    if !path.exists() {
        return Err("未检测到 WorkBuddy 客户端登录文件（请先在 WorkBuddy 客户端登录）".into());
    }
    let content = std::fs::read(&path).map_err(|e| format!("读取登录文件失败: {e}"))?;
    let root: Value = serde_json::from_slice(&content).map_err(|e| format!("登录文件解析失败: {e}"))?;
    let info = parse_auth_info(&root).ok_or("登录文件中缺少 uid")?;
    safe_uid(&info.uid)?;

    let dir = auth_store_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let target = dir.join(format!("{}.info", info.uid));
    write_atomic(&target, &content)?;
    Ok(info)
}

/// 列出所有已备份的官方登录文件（脱敏）。
pub fn list_stored_auth() -> Vec<StoredAuthMeta> {
    let dir = auth_store_dir();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            // 只认 <uid>.info，跳过 .tmp 临时文件
            if !name.ends_with(".info") || name.contains(".tmp.") {
                continue;
            }
            let uid_from_name = name.trim_end_matches(".info").to_string();
            let meta = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|v| parse_auth_info(&v));
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let last_modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| {
                    let dt = chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + d);
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_else(|| "-".to_string());
            out.push(StoredAuthMeta {
                uid: meta.as_ref().map(|m| m.uid.clone()).unwrap_or(uid_from_name),
                nickname: meta.as_ref().and_then(|m| m.nickname.clone()),
                email: meta.as_ref().and_then(|m| m.email.clone()),
                file_name: name,
                size_bytes: size,
                last_modified,
            });
        }
    }
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    out
}

/// 把指定 uid 的备份原子写回官方登录文件，完成客户端账号切换。
/// 切换前校验备份文件 uid 与请求一致，防止错号。
pub fn switch_auth_to(uid: &str) -> Result<WbAuthInfo, String> {
    safe_uid(uid)?;
    let src = auth_store_dir().join(format!("{}.info", uid));
    if !src.exists() {
        return Err(format!(
            "未找到账号 {} 的登录备份，请先在该账号登录状态下点「备份当前登录」",
            uid
        ));
    }
    let content = std::fs::read(&src).map_err(|e| format!("读取备份失败: {e}"))?;
    let root: Value = serde_json::from_slice(&content).map_err(|e| format!("备份文件解析失败: {e}"))?;
    let info = parse_auth_info(&root).ok_or("备份文件缺少 uid")?;
    if info.uid != uid {
        return Err("备份文件校验失败：uid 不匹配，已中止切换".into());
    }
    let target = auth_file_path();
    write_atomic(&target, &content)?;
    Ok(info)
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
