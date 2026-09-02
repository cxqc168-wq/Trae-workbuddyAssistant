# WorkBuddy 自动签到集成 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 workbuddy-switch 样例的签到/token 刷新/本地导入/积分查询能力移植为本项目原生 Rust 模块，并新增带左右滑动切换的 WorkBuddy 前端页面。

**Architecture:** Rust 端新增 `src-tauri/src/workbuddy/` 模块（accounts/auth\_file/refresh/checkin/credits），用项目现有 `ureq` 同步 HTTP + `AppState` 数据目录；命令层 `commands/workbuddy.rs` 暴露 Tauri commands 并以事件推送签到进度；前端新增独立页面（账号/签到/积分三板块按钮 + translateX 滑动）。账号数据存 `data/workbuddy_accounts.json`，与 Trae 账号完全隔离。

**Tech Stack:** Rust (ureq/serde/chrono/sha2)、Tauri 2 commands/events、React + TypeScript + Tailwind。

**关键约定（全局适用）：**

- HTTP 请求统一用 `ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build()`，JSON POST 空体 `{}`，响应一律 `serde_json::from_str` 为 `Value`（非 2xx 状态码也解析为 JSON，失败则返回 `{"code": status}` 形状）。

- 时间戳统一毫秒；`now_ms()` 用 `chrono::Utc::now().timestamp_millis()`。

- 账号 id 生成：`format!("wb-{:x}-{:x}", now_ms, sha256(token)[..8])`（无需新依赖 uuid）。

- 样例源码参考路径：`D:\My_Codeproject\trae-daily\20260902-093039\workbuddy-switch-main\workbuddy-switch-main\crates\wb-switch-core\src\modules\`（下称 `SAMPLE/modules/`）。

- 每个任务完成后运行 `cargo test`（在 `src-tauri/` 下）验证。

***

### Task 1: workbuddy 账号存储模块

**Files:**

- Create: `src-tauri/src/workbuddy/mod.rs`

- Create: `src-tauri/src/workbuddy/accounts.rs`

- Modify: `src-tauri/src/main.rs`（顶部加 `mod workbuddy;`）

- Test: `src-tauri/src/workbuddy/accounts.rs` 内 `#[cfg(test)]`

- [ ] **Step 1: 创建 mod.rs 并挂载模块**

`src-tauri/src/workbuddy/mod.rs`：

```rust
pub mod accounts;
pub mod auth_file;
pub mod refresh;
pub mod checkin;
pub mod credits;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
```

（其余子模块后续任务创建；本任务先只写 `pub mod accounts;`，后续任务再逐行追加 `pub mod`。）

main.rs 顶部（`mod state;` 之后）加：

```rust
mod workbuddy;
```

- [ ] **Step 2: 编写 accounts.rs 失败测试**

```rust
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
```

- [ ] **Step 3: 运行测试**

Run: `cargo test workbuddy::accounts`（cwd `src-tauri/`）
Expected: PASS（5 个测试）

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/workbuddy/ src-tauri/src/main.rs
git commit -m "feat(workbuddy): 账号存储模块（去重合并/脱敏视图）"
```

***

### Task 2: 本地认证文件导入

**Files:**

- Create: `src-tauri/src/workbuddy/auth_file.rs`

- Modify: `src-tauri/src/workbuddy/mod.rs`（追加 `pub mod auth_file;`）

- Test: 同文件 `#[cfg(test)]`

- [ ] **Step 1: 编写 auth\_file.rs（含失败测试）**

```rust
//! 读取 WorkBuddy 客户端官方认证文件（只读，不写入）。
//! 对照 SAMPLE/modules/auth_file.rs 的 import_from_auth_file。

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

fn dirs_home() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

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

/// 从认证文件根对象提取账号（字段链与样例一致）。无 access_token 返回 None。
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
```

- [ ] **Step 2: mod.rs 追加并运行测试**

mod.rs 中追加 `pub mod auth_file;`
Run: `cargo test workbuddy::auth_file`
Expected: PASS（3 个测试）

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/workbuddy/
git commit -m "feat(workbuddy): 本地认证文件导入"
```

***

### Task 3: 共用 HTTP 请求与 token 刷新

**Files:**

- Create: `src-tauri/src/workbuddy/http.rs`

- Create: `src-tauri/src/workbuddy/refresh.rs`

- Modify: `src-tauri/src/workbuddy/mod.rs`（追加两个 `pub mod`）

- Test: refresh.rs 内 `#[cfg(test)]`

- [ ] **Step 1: http.rs（统一请求入口）**

```rust
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
```

- [ ] **Step 2: refresh.rs（含测试）**

```rust
//! token 刷新与惰性刷新。对照 SAMPLE/modules/refresh.rs（去掉 Windows env 同步）。

use serde_json::{json, Value};
use std::path::PathBuf;

use super::accounts::{get_str, load_accounts_from_path, save_accounts_to_path, upsert_collected_account};
use super::http::{build_auth_headers, http_post_json, is_unauthorized, WORKBUDDY_API_ENDPOINT, WORKBUDDY_API_PREFIX};

/// 测试注入：非 None 时 load/save 走该目录（tests 用）。
pub(crate) fn store_path() -> PathBuf {
    super::store_path()
}

pub(crate) fn load_accounts() -> Vec<Value> {
    load_accounts_from_path(&store_path().join("workbuddy_accounts.json"))
}

pub(crate) fn persist_account(acc: &Value) {
    let mut list = load_accounts();
    upsert_collected_account(&mut list, acc.clone());
    let _ = save_accounts_to_path(&store_path().join("workbuddy_accounts.json"), &list);
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
        let acc = refresh_account_token(json!({"id": "t1", "access_token": "a"}));
        assert_eq!(acc["needs_relogin"], true);
    }

    #[test]
    fn ensure_fresh_skips_when_valid() {
        let exp = super::super::now_ms() + 7 * 24 * 3600 * 1000;
        let acc = ensure_fresh_token(json!({"id": "t2", "expiresAt": exp, "access_token": "a"}));
        assert!(acc.get("needs_relogin").is_none());
    }

    #[test]
    fn ensure_fresh_flags_when_expired_no_rt() {
        let acc = ensure_fresh_token(json!({"id": "t3", "expiresAt": 1, "access_token": "a"}));
        // 无 refresh_token 不会调用刷新（刷新会标 needs_relogin），原样返回
        assert!(acc.get("needs_relogin").is_none());
    }
}
```

- [ ] **Step 3: mod.rs 增加 store\_path**

mod.rs 更新为：

```rust
pub mod accounts;
pub mod auth_file;
pub mod http;
pub mod refresh;

use std::path::PathBuf;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 数据目录注入点：默认 %APPDATA%\TraeWorkAssistant\data；
/// 测试通过 TEST_STORE 覆盖。
pub fn store_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TRAEBUDDY_TEST_STORE") {
        return PathBuf::from(dir);
    }
    std::env::var("APPDATA")
        .map(|a| PathBuf::from(a).join("TraeWorkAssistant").join("data"))
        .unwrap_or_default()
}
```

> 注意：`is_unauthorized` 在 http.rs 暂未被使用会有 warning，Task 4 会用到；如需消除可在 http.rs 加 `#[allow(dead_code)]`。

- [ ] **Step 4: 运行测试并提交**

Run: `cargo test workbuddy::`
Expected: PASS

```bash
git add src-tauri/src/workbuddy/
git commit -m "feat(workbuddy): HTTP 入口与 token 刷新"
```

***

### Task 4: 签到模块

**Files:**

- Create: `src-tauri/src/workbuddy/checkin.rs`

- Modify: `src-tauri/src/workbuddy/mod.rs`（追加 `pub mod checkin;`）

- Test: checkin.rs 内 `#[cfg(test)]`

- [ ] **Step 1: 编写 checkin.rs（含测试）**

```rust
//! 签到：状态查询 / 执行 / 单账号流程 / 一键全签。对照 SAMPLE/modules/checkin.rs。

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
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code == 0 || code == 200 {
        let data = resp.get("data").cloned().unwrap_or_else(|| json!({}));
        return json!({
            "ok": true,
            "todayCheckedIn": data.get("today_checked_in")
                .or_else(|| data.get("todayCheckedIn"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        });
    }
    let resp2 = checkin_request(&format!("{CHECKIN_API_PREFIX}/checkin-status"), account);
    let code2 = resp2.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code2 == 0 || code2 == 200 {
        let data = resp2.get("data").cloned().unwrap_or_else(|| json!({}));
        return json!({
            "ok": true,
            "todayCheckedIn": data.get("today_checked_in")
                .or_else(|| data.get("todayCheckedIn"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        });
    }
    json!({
        "ok": false,
        "error": resp2.get("message").or_else(|| resp2.get("msg"))
            .and_then(|v| v.as_str()).unwrap_or(&format!("code={code2}")).to_string(),
    })
}

/// 执行签到；“已签到”按成功。
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
    // 文本日志
    let text = dir.join("logs").join("workbuddy_checkin.log");
    let _ = std::fs::create_dir_all(dir.join("logs"));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&text) {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), entry);
    }
}

/// 今日该账号最近一次签到结果。
fn latest_today_result(today: &str) -> Option<String> {
    let logs = super::accounts::load_accounts_from_path(&super::store_path().join("workbuddy_checkin_logs.json"));
    let mut found: Option<String> = None;
    for e in logs.iter().rev() {
        let ts = e.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let date = chrono::DateTime::from_timestamp_millis(ts)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        if date == today {
            if let Some(r) = e.get("result").and_then(|v| v.as_str()) {
                found = Some(r.to_string());
                break;
            }
        }
    }
    found
}

/// 单账号完整签到：刷新 → 查状态 → 未签则提交 → 记日志。
/// 返回 {result: success|already|error, error?}
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

/// 今日是否已签（供账号列表展示）。
pub fn checked_in_today(account: &Value) -> bool {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let id = match account.get("id").and_then(|v| v.as_str()) {
        Some(i) => i.to_string(),
        None => return false,
    };
    let logs = super::accounts::load_accounts_from_path(&super::store_path().join("workbuddy_checkin_logs.json"));
    logs.iter()
        .filter(|e| e.get("accountId").and_then(|v| v.as_str()) == Some(id.as_str()))
        .filter_map(|e| {
            let ts = e.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
            chrono::DateTime::from_timestamp_millis(ts).map(|d| {
                d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()
            })
        })
        .any(|d| d == today && matches!(latest_today_result(&today).as_deref(), Some("success") | Some("already")))
        && matches!(latest_today_result(&today).as_deref(), Some("success") | Some("already"))
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
}
```

> 注：`latest_today_result` 简化为全局最近一条（本项目单进程串行签到，够用）；`checked_in_today` 判定该账号当天存在 success/already 日志。实现时若发现逻辑冗余可直接简化为“该账号当天最新日志为 success/already”。

- [ ] **Step 2: 运行测试并提交**

Run: `cargo test workbuddy::`
Expected: PASS

```bash
git add src-tauri/src/workbuddy/
git commit -m "feat(workbuddy): 签到流程与日志"
```

***

### Task 5: 积分查询模块

**Files:**

- Create: `src-tauri/src/workbuddy/credits.rs`

- Modify: `src-tauri/src/workbuddy/mod.rs`（追加 `pub mod credits;`）

- Test: credits.rs 内 `#[cfg(test)]`

- [ ] **Step 1: 编写 credits.rs（含测试）**

完整移植 `SAMPLE/modules/credits.rs` 的解析逻辑，改动点：

- `http_request` → 本项目 `http_post_json`（同步）；三路 `tokio::join!` 改为顺序调用（结果一致，串行即可）。

- `load_checkin_config`/`credit_usage::record_snapshot` 依赖删除（YAGNI）。

- `ensure_fresh_token`/`refresh_account_token` 调用签名对齐 Task 3（无 cfg 参数）。

```rust
//! WorkBuddy 积分资源查询。对照 SAMPLE/modules/credits.rs（串行化 + 去配置依赖）。

use serde_json::{json, Value};

use super::accounts::{account_display_name, build_auth_headers_placeholder as _};
```

> 上行 import 为示意——实际直接 `use super::http::{http_post_json, is_unauthorized, WORKBUDDY_API_ENDPOINT};`。以下为核心结构（完整代码以样例 L1-L675 为底本逐函数移植，本计划列出需保留的全部公开/私有函数清单与签名，实现时从样例复制并做列出的改动）：

保留函数（名称与语义与样例一致）：

- `first_value`, `parse_number`, `first_number`, `parse_timestamp_ms`, `value_at_path`

- `resource_accounts`, `resource_packages`, `has_resource_accounts`, `has_resource_packages`

- `resource_summary(raw, now) -> Value`

- `response_error`, `response_code`, `is_success`（`is_unauthorized` 用 `super::http::is_unauthorized`）

- `resource_auth_headers`（`build_auth_headers` + `X-Client-Platform: web`）

- `paid_packages_body`, `free_packages_body`（含 PAID\_PACKAGE\_CODES / FREE\_PACKAGE\_CODES 常量原样复制）

- `new_resource_endpoint`, `new_resource_url`

- `fetch_new_resource_responses`：串行执行三路（summary/paid/free），任一未授权且有 rt → 刷新一次，仅重试未授权分支

- `fetch_legacy_user_resource`（回退旧接口）

- `merge_resources`, `normalized_new_resources`, `credit_result`（去掉 `credit_usage::record_snapshot` 调用）

- 公开入口 `pub fn get_credit_expiry(account: &Value) -> Value`

改动清单：

1. 所有 `async fn` → `fn`（ureq 同步）。
2. `tokio::join!(...)` 三路并行 → 顺序调用三次 `post_with_account`。
3. `ensure_fresh_token(account, &cfg)` → `ensure_fresh_token(account)`。
4. 删除 `credit_usage` 相关 import 与调用。
5. `chrono::Local` 用法保持（Cargo 已有 chrono）。

测试（原样移植样例 tests 中不依赖网络/配置的用例）：

- `parses_cockpit_resource_shape_and_marks_expiry`

- `parses_second_millisecond_and_datetime_timestamps`

- `extracts_nested_accounts`

- `extracts_new_top_level_accounts_and_packages`

- `parses_summary_capacity_fields_and_explicit_used_value`

- `keeps_detail_batches_and_only_fills_missing_summary_packages`

- `accepts_empty_detail_accounts_as_a_valid_success`

- `partial_new_success_returns_available_resources`

- `valid_empty_new_arrays_do_not_trigger_legacy_fallback`

- `all_invalid_new_responses_require_legacy_fallback`

- `new_request_bodies_match_workbuddy_filters`

- `selects_endpoint_from_known_account_domain_and_keeps_headers_aligned`

- `accepts_object_response_without_code`

- `sums_only_resources_that_are_expiring_soon`

> **注意（给执行者）：** 样例文件路径 `SAMPLE/modules/credits.rs`，测试代码在该文件 L677-L959。`normalized_new_resources` 测试需要把该函数改为 `pub(crate)` 或在 tests 内通过公开入口不可达时保留同文件测试可见性（同文件 `#[cfg(test)]` 可访问私有函数，无需改可见性）。

- [ ] **Step 2: 运行测试并提交**

Run: `cargo test workbuddy::credits`
Expected: PASS（14 个测试）

```bash
git add src-tauri/src/workbuddy/
git commit -m "feat(workbuddy): 积分查询（新接口+旧接口回退）"
```

***

### Task 6: Tauri 命令层

**Files:**

- Create: `src-tauri/src/commands/workbuddy.rs`

- Modify: `src-tauri/src/commands/mod.rs`（追加 `pub mod workbuddy;`）

- Modify: `src-tauri/src/main.rs`（invoke\_handler 注册 8 个命令）

- [ ] **Step 1: 编写 commands/workbuddy.rs**

```rust
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

#[tauri::command]
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

#[tauri::command]
pub fn workbuddy_checkin_status(account_id: String) -> Result<Value, String> {
    let acc = load_all().into_iter()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id.as_str()))
        .ok_or("账号不存在")?;
    let acc = refresh::ensure_fresh_token(acc);
    refresh::persist_account(&acc);
    Ok(checkin::get_checkin_status(&acc))
}

#[tauri::command]
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

#[tauri::command]
pub fn workbuddy_credits(account_id: Option<String>) -> Result<Vec<Value>, String> {
    let accounts: Vec<Value> = load_all().into_iter()
        .filter(|a| account_id.as_deref().map_or(true, |id| a.get("id").and_then(|v| v.as_str()) == Some(id)))
        .collect();
    if accounts.is_empty() {
        return Err("没有匹配的账号".into());
    }
    Ok(accounts.iter().map(credits::get_credit_expiry).collect())
}

#[tauri::command]
pub fn workbuddy_refresh_token(account_id: String) -> Result<Value, String> {
    let acc = load_all().into_iter()
        .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(account_id.as_str()))
        .ok_or("账号不存在")?;
    if get_str(&acc, "refresh_token").unwrap_or_default().is_empty() {
        return Err("该账号没有 refresh_token".into());
    }
    let fresh = refresh::refresh_account_token(acc);
    Ok(account_meta(&fresh))
}
```

- [ ] **Step 2: 注册命令**

commands/mod.rs 追加：

```rust
pub mod workbuddy;
```

main.rs `invoke_handler` 追加（`commands::oauth::oauth_callback_stop,` 之后）：

```rust
            commands::workbuddy::workbuddy_list_accounts,
            commands::workbuddy::workbuddy_import_local,
            commands::workbuddy::workbuddy_add_manual,
            commands::workbuddy::workbuddy_delete_account,
            commands::workbuddy::workbuddy_checkin_status,
            commands::workbuddy::workbuddy_checkin_all,
            commands::workbuddy::workbuddy_credits,
            commands::workbuddy::workbuddy_refresh_token,
```

- [ ] **Step 3: 编译验证并提交**

Run: `cargo check`（cwd `src-tauri/`）
Expected: 无 error（unused warning 允许，可加 `#[allow(dead_code)]` 于 http.rs 的 `is_unauthorized` 若 Task5 前报）

```bash
git add src-tauri/src/commands/ src-tauri/src/main.rs
git commit -m "feat(workbuddy): Tauri 命令层（8 个命令 + 签到进度事件）"
```

***

### Task 7: 前端类型与 API 封装

**Files:**

- Modify: `src/types.ts`（追加 WorkBuddy 类型）

- Modify: `src/lib/tauri.ts`（追加 workbuddy 命名空间）

- [ ] **Step 1: types.ts 末尾追加**

```typescript
// ---- WorkBuddy ----
export type WorkBuddyViewKey = 'accounts' | 'checkin' | 'credits';

export interface WorkBuddyAccountMeta {
  id: string;
  uid: string | null;
  email: string | null;
  nickname: string | null;
  enterpriseName: string | null;
  expiresAt: number | null;
  refreshExpiresAt: number | null;
  refreshedAt: number | null;
  createdAt: number | null;
  hasRefreshToken: boolean;
  needsRelogin: boolean;
  needsReloginReason: string | null;
  checkedToday?: boolean;
}

export type WorkBuddyCheckinResult = 'success' | 'already' | 'error';

export interface WorkBuddyCheckinEntry {
  accountId: string | null;
  email: string;
  result: WorkBuddyCheckinResult | null;
  error: string | null;
}

export interface WorkBuddyCheckinProgress {
  index: number;
  total: number;
  accountId: string | null;
  email: string;
  result: WorkBuddyCheckinResult | null;
  error: string | null;
}

export interface WorkBuddyCheckinDone {
  ok: number;
  already: number;
  failed: number;
  total: number;
}

export interface WorkBuddyCreditResource {
  packageCode: string | null;
  packageName: string | null;
  total: number;
  remaining: number;
  used: number;
  expireAt: number | null;
  expired: boolean;
  expiringSoon: boolean;
}

export interface WorkBuddyCreditSummary {
  ok: boolean;
  accountId: string | null;
  accountName: string;
  error?: string;
  totalCapacity?: number;
  totalRemaining?: number;
  expiringSoonRemaining?: number;
  expiredRemaining?: number;
  soonestExpireAt?: number | null;
  expiringSoon?: boolean;
  expired?: boolean;
  resources?: WorkBuddyCreditResource[];
}
```

同时把 `ViewKey` 联合类型追加 `| 'workbuddy'`。

- [ ] **Step 2: tauri.ts 追加 workbuddy 命名空间（api 对象内）**

```typescript
  workbuddy: {
    listAccounts: () => invoke<WorkBuddyAccountMeta[]>('workbuddy_list_accounts'),
    importLocal: () => invoke<WorkBuddyAccountMeta>('workbuddy_import_local'),
    addManual: (args: { access_token: string; refresh_token?: string; uid?: string; nickname?: string }) =>
      invoke<WorkBuddyAccountMeta>('workbuddy_add_manual', { args }),
    deleteAccount: (accountId: string) => invoke('workbuddy_delete_account', { accountId }),
    checkinStatus: (accountId: string) => invoke<{ ok: boolean; todayCheckedIn?: boolean; error?: string }>('workbuddy_checkin_status', { accountId }),
    checkinAll: (accountIds?: string[]) => invoke<WorkBuddyCheckinEntry[]>('workbuddy_checkin_all', { accountIds }),
    credits: (accountId?: string) => invoke<WorkBuddyCreditSummary[]>('workbuddy_credits', { accountId }),
    refreshToken: (accountId: string) => invoke<WorkBuddyAccountMeta>('workbuddy_refresh_token', { accountId }),
  },
```

并在 tauri.ts 顶部 import 处追加类型导入（按文件现有风格，若已是 `import type { ... } from '../types'` 则并入）。

- [ ] **Step 3: 类型检查并提交**

Run: `npx tsc --noEmit`
Expected: 无错误

```bash
git add src/types.ts src/lib/tauri.ts
git commit -m "feat(workbuddy): 前端类型与 API 封装"
```

***

### Task 8: WorkBuddy 页面（三板块滑动切换）

**Files:**

- Create: `src/pages/WorkBuddy.tsx`

- Modify: `src/components/Sidebar.tsx`（NAV 追加 workbuddy 项）

- Modify: `src/App.tsx`（import + renderView case）

- [ ] **Step 1: 编写 WorkBuddy.tsx**

结构要点（完整实现，风格对齐现有页面：Card/按钮 className 复用项目 `input`/`label` 类名与 zinc/slate 双主题）：

```tsx
import { useCallback, useEffect, useRef, useState } from 'react';
import { Bot, Coins, Import, Loader2, PlayCircle, Plus, RefreshCw, Trash2, Users } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import { api } from '../lib/tauri';
import { useAppStore } from '../store';
import { cn } from '../lib/cn';
import type {
  WorkBuddyAccountMeta,
  WorkBuddyCheckinDone,
  WorkBuddyCheckinProgress,
  WorkBuddyCreditSummary,
  WorkBuddyViewKey,
} from '../types';

const PANELS: { key: WorkBuddyViewKey; label: string; icon: typeof Users }[] = [
  { key: 'accounts', label: '账号列表', icon: Users },
  { key: 'checkin', label: '一键签到', icon: PlayCircle },
  { key: 'credits', label: '积分概览', icon: Coins },
];
```

页面骨架：

```tsx
export default function WorkBuddy() {
  const [panel, setPanel] = useState<WorkBuddyViewKey>('accounts');
  const [accounts, setAccounts] = useState<WorkBuddyAccountMeta[]>([]);
  const [loading, setLoading] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualForm, setManualForm] = useState({ access_token: '', refresh_token: '', uid: '', nickname: '' });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [skipChecked, setSkipChecked] = useState(true);
  const [checkinRunning, setCheckinRunning] = useState(false);
  const [checkinResults, setCheckinResults] = useState<WorkBuddyCheckinProgress[]>([]);
  const [creditsData, setCreditsData] = useState<WorkBuddyCreditSummary[] | null>(null);
  const [creditsLoading, setCreditsLoading] = useState(false);
  const toast = useAppStore((s) => s.pushToast);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(await api.workbuddy.listAccounts());
    } catch (e) {
      toast('error', `加载 WorkBuddy 账号失败：${String(e)}`);
    } finally {
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => { void reload(); }, [reload]);

  // 签到进度事件
  useEffect(() => {
    const un1 = listen<WorkBuddyCheckinProgress>('workbuddy-checkin-progress', (e) => {
      setCheckinResults((prev) => [...prev, e.payload]);
    });
    const un2 = listen<WorkBuddyCheckinDone>('workbuddy-checkin-done', (e) => {
      setCheckinRunning(false);
      toast('success', `签到完成：成功 ${e.payload.ok}，已签 ${e.payload.already}，失败 ${e.payload.failed}`);
      void reload();
    });
    return () => { void un1.then((f) => f()); void un2.then((f) => f()); };
  }, [reload, toast]);
```

（后续：导入/手动添加/删除/签到/积分的处理函数与各板块 JSX。）

**滑动容器实现（核心）：**

```tsx
  return (
    <div className="flex h-full flex-col">
      {/* 顶部按钮组 */}
      <div className="mb-4 flex gap-2">
        {PANELS.map((p) => {
          const Icon = p.icon;
          const active = panel === p.key;
          return (
            <button
              key={p.key}
              onClick={() => setPanel(p.key)}
              className={cn(
                'flex flex-1 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition active:scale-[0.98]',
                active
                  ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
                  : 'bg-white text-slate-600 hover:bg-slate-100 dark:bg-zinc-900 dark:text-zinc-400 dark:hover:bg-zinc-800',
              )}
            >
              <Icon size={16} />
              {p.label}
            </button>
          );
        })}
      </div>
      {/* 左右滑动容器：三面板横排，translateX 切换 */}
      <div className="min-h-0 flex-1 overflow-hidden">
        <div
          className="flex h-full w-full transition-transform duration-300 ease-out"
          style={{ transform: `translateX(-${PANELS.findIndex((p) => p.key === panel) * 100}%)` }}
        >
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">{/* 账号面板 */}</div>
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">{/* 签到面板 */}</div>
          <div className="h-full w-full shrink-0 overflow-y-auto pr-1">{/* 积分面板 */}</div>
        </div>
      </div>
    </div>
  );
```

**账号面板内容**：顶部「导入本机账号」（`api.workbuddy.importLocal`，成功 toast + reload，失败提示需登录客户端）与「手动添加」（切 manualOpen 弹层，表单 4 字段，提交 `api.workbuddy.addManual`）；账号卡片列表（昵称/邮箱/uid、token 状态徽标——`needsRelogin` 红「需重登」、`expiresAt` 剩余 <24h 黄「临期」、否则绿「有效」、`checkedToday` 显示「今日已签」、`hasRefreshToken` 显示「可刷新」；删除按钮确认后 `deleteAccount` + reload）。

**签到面板内容**：账号复选列表（默认全选；`skipChecked` 开关默认开，开启时 `checkedToday` 账号禁用勾选）、「开始签到」按钮（`checkinRunning` 时禁用 + Loader2 旋转；调用 `api.workbuddy.checkinAll(selectedIds)`，事件驱动更新 `checkinResults`）；结果列表（每账号一行：email + result 徽标 + error 文本）。

**积分面板内容**：「刷新积分」按钮（`creditsLoading` 旋转；`api.workbuddy.credits()` → setCreditsData）；每账号卡片：accountName、总剩余/总额度大数字 + 进度条（`totalRemaining/totalCapacity`）、`expiringSoon` 时黄色警示行「即将过期额度 X」、`error` 时红色错误行。

- [ ] **Step 2: Sidebar.tsx 接线**

NAV 数组「一键签到」项之后插入：

```tsx
  { key: 'workbuddy', label: 'WorkBuddy', icon: Bot },
```

lucide import 列表加 `Bot`。

- [ ] **Step 3: App.tsx 接线**

```tsx
import WorkBuddy from './pages/WorkBuddy';
```

renderView 的 switch 加：

```tsx
    case 'workbuddy':
      return <WorkBuddy />;
```

- [ ] **Step 4: 类型检查并提交**

Run: `npx tsc --noEmit`
Expected: 无错误

```bash
git add src/pages/WorkBuddy.tsx src/components/Sidebar.tsx src/App.tsx
git commit -m "feat(workbuddy): 三板块滑动切换页面（账号/签到/积分）"
```

***

### Task 9: 集成构建与真机验证

- [ ] **Step 1: Rust 全量测试**

Run: `cargo test`（cwd `src-tauri/`）
Expected: 全部 PASS（含原有测试）

- [ ] **Step 2: 启动应用**

Run: `npm run tauri dev`
Expected: 编译成功、窗口正常显示、侧边栏出现「WorkBuddy」

- [ ] **Step 3: 手动验收（用户配合）**

1. WorkBuddy 页面：三按钮切换左右滑动动画正常
2. 手动添加（粘贴 token）成功出现在列表
3. （装有 WorkBuddy 客户端时）本地导入成功
4. 一键签到：进度逐账号刷新、结果徽标正确、已签账号跳过
5. 积分刷新：卡片显示总额度/剩余
6. Trae 原有签到/账号页不受影响

- [ ] **Step 4: 提交收尾**

```bash
git add -A
git commit -m "feat(workbuddy): WorkBuddy 自动签到集成完成"
```

