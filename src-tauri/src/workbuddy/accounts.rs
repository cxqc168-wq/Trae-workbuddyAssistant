//! WorkBuddy 账号存储：data/workbuddy_accounts.json，与 Trae 账号隔离。

use serde_json::{json, Value};
use std::path::Path;

use super::now_ms;

/// 从指定路径读取账号数组；缺失/损坏返回空。
pub fn load_accounts_from_path(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

/// 原子写账号数组。
pub fn save_accounts_to_path(path: &Path, accounts: &[Value]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(accounts).unwrap_or_default())?;
    std::fs::rename(&tmp, path)
}

/// 非空字符串字段；空/缺失返回 None。
pub fn get_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 展示名：email → nickname → uid → unknown。
pub fn account_display_name(acc: &Value) -> String {
    get_str(acc, "email")
        .or_else(|| get_str(acc, "nickname"))
        .or_else(|| get_str(acc, "uid"))
        .unwrap_or_else(|| "unknown".to_string())
}

/// 脱敏视图（不含 token）。
pub fn account_meta(acc: &Value) -> Value {
    json!({
        "id": acc.get("id"),
        "uid": acc.get("uid"),
        "email": acc.get("email"),
        "nickname": acc.get("nickname"),
        "enterpriseName": acc.get("enterpriseName"),
        "expiresAt": acc.get("expiresAt"),
        "refreshExpiresAt": acc.get("refreshExpiresAt"),
        "refreshedAt": acc.get("refreshedAt"),
        "createdAt": acc.get("createdAt"),
        "hasRefreshToken": !get_str(acc, "refresh_token").unwrap_or_default().is_empty(),
        "needsRelogin": acc.get("needs_relogin").and_then(|v| v.as_bool()) == Some(true),
        "needsReloginReason": acc.get("needs_relogin_reason"),
    })
}

/// 真实邮箱（含 @ 且非占位值），用于 uid 缺失时的身份兜底。
fn identity_email(account: &Value) -> Option<String> {
    let email = get_str(account, "email")?;
    if !email.contains('@')
        || email.eq_ignore_ascii_case("unknown")
        || get_str(account, "nickname").as_deref() == Some(email.as_str())
        || get_str(account, "uid").as_deref() == Some(email.as_str())
    {
        return None;
    }
    Some(email.to_ascii_lowercase())
}

/// 按 uid（优先）/真实邮箱去重合并进列表，命中保留本地 id。
pub fn upsert_collected_account(accounts: &mut Vec<Value>, mut collected: Value) -> Value {
    let collected_uid = get_str(&collected, "uid");
    let collected_email = identity_email(&collected);
    let matches = |existing: &Value| {
        if let Some(uid) = collected_uid.as_deref() {
            return get_str(existing, "uid").as_deref() == Some(uid);
        }
        collected_email
            .as_deref()
            .is_some_and(|e| identity_email(existing).as_deref() == Some(e))
    };
    let idx: Vec<usize> = accounts
        .iter()
        .enumerate()
        .filter_map(|(i, a)| matches(a).then_some(i))
        .collect();
    if let Some(&first) = idx.first() {
        if let Some(id) = accounts[first].get("id").cloned() {
            collected["id"] = id;
        }
        if get_str(&collected, "uid").is_none() {
            if let Some(uid) = accounts[first].get("uid").cloned() {
                collected["uid"] = uid;
            }
        }
        if let Some(created) = accounts[first].get("createdAt").cloned() {
            collected["createdAt"] = created;
        }
        for i in idx.into_iter().rev() {
            accounts.remove(i);
        }
        accounts.insert(first.min(accounts.len()), collected.clone());
    } else {
        accounts.push(collected.clone());
    }
    collected
}

/// 生成账号 id：时间戳 + token 哈希前 8 位。
pub fn generate_id(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("wb-{:x}-{}", now_ms(), &hex[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_strips_tokens() {
        let acc = json!({"id": "a1", "email": "x@y.z", "access_token": "S", "refresh_token": "R"});
        let meta = account_meta(&acc);
        assert!(meta.get("access_token").is_none());
        assert!(meta.get("refresh_token").is_none());
    }

    #[test]
    fn same_uid_merge_preserves_id() {
        let mut list = vec![json!({"id": "old", "uid": "u1", "createdAt": 1})];
        let saved = upsert_collected_account(&mut list, json!({"id": "new", "uid": "u1"}));
        assert_eq!(list.len(), 1);
        assert_eq!(saved["id"], "old");
        assert_eq!(saved["createdAt"], 1);
    }

    #[test]
    fn email_fallback_merges_without_uid() {
        let mut list = vec![json!({"id": "a", "email": "user@e.com"})];
        upsert_collected_account(&mut list, json!({"id": "b", "email": "USER@e.com"}));
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn nickname_placeholder_email_does_not_merge() {
        let mut list = vec![json!({"id": "a", "nickname": "n", "email": "n"})];
        upsert_collected_account(&mut list, json!({"id": "b", "nickname": "n", "email": "n"}));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wb-test-{}", now_ms()));
        let p = dir.join("accounts.json");
        let list = vec![json!({"id": "x"})];
        save_accounts_to_path(&p, &list).unwrap();
        assert_eq!(load_accounts_from_path(&p).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
