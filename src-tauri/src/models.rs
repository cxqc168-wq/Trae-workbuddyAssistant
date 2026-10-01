use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Clone, Default)]
pub struct AccountView {
    #[serde(default)]
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub group_id: Option<String>,
    pub jwt: String,
    pub jwt_exp_hours: Option<f64>,
    pub jwt_exp_timestamp: Option<i64>,
    pub checked_today: Option<bool>,
    pub credits: Option<i64>,
    pub remaining_credits: Option<f64>,
    pub device_id_masked: Option<String>,
    pub cooldown_type: Option<String>,
    pub cooldown_until: Option<i64>,
    pub cooldown_reason: Option<String>,
    pub has_refresh_token: bool,
    pub jwt_auto_refresh: bool,
    pub credits_expire_at: Option<i64>,
}

pub fn deserialize_flexible_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexibleValue {
        String(String),
        Int(i64),
        Float(f64),
        Null,
    }

    match Option::<FlexibleValue>::deserialize(deserializer)? {
        Some(FlexibleValue::String(s)) => {
            let t = s.trim().to_string();
            if t.is_empty() { Ok(None) } else { Ok(Some(t)) }
        }
        Some(FlexibleValue::Int(n)) => Ok(Some(n.to_string())),
        Some(FlexibleValue::Float(f)) => Ok(Some((f as i64).to_string())),
        Some(FlexibleValue::Null) | None => Ok(None),
    }
}

pub fn build_stable_account_id(a: &RawAccount) -> String {
    if let Some(ref id) = a.id {
        let t = id.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(a.name.as_bytes());
    h.update(a.user_id.as_deref().unwrap_or("").as_bytes());
    h.update(a.jwt.as_bytes());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("acc-{}", &hex[..12])
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RawAccount {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(
        rename = "UserID",
        alias = "user_id",
        alias = "userId",
        default,
        deserialize_with = "deserialize_flexible_string"
    )]
    pub user_id: Option<String>,
    #[serde(default)]
    pub jwt: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub added_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Serialize, Clone, Default)]
pub struct AccountsFile {
    #[serde(default)]
    pub accounts: Vec<RawAccount>,
}

impl<'de> serde::Deserialize<'de> for AccountsFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ObjectFormat {
            #[serde(default)]
            accounts: Vec<RawAccount>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Object(ObjectFormat),
            Array(Vec<RawAccount>),
        }

        match Helper::deserialize(deserializer)? {
            Helper::Object(o) => Ok(AccountsFile { accounts: o.accounts }),
            Helper::Array(a) => Ok(AccountsFile { accounts: a }),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub order: i32,
}

#[derive(Serialize, Deserialize, Default)]
pub struct GroupsFile {
    pub groups: Vec<Group>,
    #[serde(default)]
    pub membership: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DeviceEntry {
    pub device_id: String,
    #[serde(default)]
    pub market_user_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

pub type DeviceMap = HashMap<String, DeviceEntry>;

#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default = "default_port")]
    pub proxy_port: u16,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub launch_minimized: bool,
    #[serde(default = "default_false")]
    pub auto_start_proxy: bool,
    #[serde(default = "default_true")]
    pub tray: bool,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_true")]
    pub checkin_skip_checked: bool,
    #[serde(default = "default_true")]
    pub checkin_skip_expired: bool,
    #[serde(default = "default_retry")]
    pub retry: i32,
    #[serde(default = "default_notify")]
    pub notify: String,
    #[serde(default)]
    pub trae_path: Option<String>,
    #[serde(default)]
    pub browser_path: Option<String>,
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default = "default_retention")]
    pub log_retention_days: i32,
    #[serde(default = "default_proxy_domains")]
    pub proxy_domains: String,
    #[serde(default)]
    pub proxy_log_path: Option<String>,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_api_model")]
    pub api_default_model: String,
}

fn default_api_port() -> u16 {
    7864
}
fn default_api_model() -> String {
    "deepseek-v4-flash".into()
}

fn default_port() -> u16 {
    8899
}
fn default_theme() -> String {
    "system".into()
}
fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}
fn default_lang() -> String {
    "zh-CN".into()
}
fn default_retry() -> i32 {
    1
}
fn default_notify() -> String {
    "toast".into()
}
fn default_retention() -> i32 {
    30
}
pub fn default_proxy_domains() -> String {
    "trae.cn,trae.com.cn,mchost.guru,zijieapi.com,bytedance.com,volcengine.com,volces.com,treecode.com".into()
}

#[derive(Serialize, Deserialize, Default)]
pub struct CreditRecord {
    pub date: String,
    pub user_id: String,
    pub credits: i64,
    #[serde(default)]
    pub delta: i64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct CreditsFile {
    pub records: Vec<CreditRecord>,
}

/// 每日积分快照：记录当天所有账号的积分总数、获得数、消耗数
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct CreditsDailySnapshot {
    pub date: String,
    pub total: f64,
    pub earned: f64,
    pub consumed: f64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct CreditsDailyFile {
    #[serde(default)]
    pub snapshots: Vec<CreditsDailySnapshot>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct CheckinSummary {
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
    #[serde(default)]
    pub total_ok: i32,
    #[serde(default)]
    pub already: i32,
    #[serde(default)]
    pub failed: i32,
}

/// 剩余积分缓存文件：user_id -> 剩余积分
#[derive(Serialize, Deserialize, Default)]
pub struct RemainingCreditsFile {
    #[serde(default)]
    pub credits: HashMap<String, f64>,
    #[serde(default)]
    pub expire_times: HashMap<String, i64>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// 单个账号的冷却状态
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CooldownEntry {
    #[serde(rename = "type", default)]
    pub error_type: String,
    #[serde(default)]
    pub until: i64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub error_count: i32,
}

/// 冷却状态文件：account_cooldowns.json
#[derive(Serialize, Deserialize, Default)]
pub struct AccountCooldownsFile {
    #[serde(default)]
    pub cooldowns: HashMap<String, CooldownEntry>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// API 池配置文件：api_pool.json
#[derive(Serialize, Deserialize, Default)]
pub struct ApiPoolFile {
    #[serde(default)]
    pub enabled_uids: Vec<String>,
}

/// 池中单个账号的运行时状态（给 /status 和前端用）
#[derive(Serialize, Clone)]
pub struct PoolStatus {
    pub uid: String,
    pub name: String,
    pub credits: Option<f64>,
    pub credits_expire_at: Option<i64>,
    pub cooling: bool,
    pub cooldown_until: Option<i64>,
    pub cooldown_reason: Option<String>,
    pub disabled: bool,
    pub err_count: i32,
}

/// API 服务整体状态（给前端用）
#[derive(Serialize, Clone)]
pub struct ApiServiceStatus {
    pub running: bool,
    pub port: u16,
    pub total_requests: u64,
    pub active_uid: Option<String>,
    pub last_error: Option<String>,
    pub started_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_account_deserialization_flexible() {
        // 1. UserID 作为数字
        let json_num = r#"{"name":"test1","UserID":3031813081815523,"jwt":"eyJ..."}"#;
        let acc1: RawAccount = serde_json::from_str(json_num).unwrap();
        assert_eq!(acc1.user_id.as_deref(), Some("3031813081815523"));

        // 2. user_id 小写别名
        let json_lower = r#"{"name":"test2","user_id":"987654321","jwt":"eyJ..."}"#;
        let acc2: RawAccount = serde_json::from_str(json_lower).unwrap();
        assert_eq!(acc2.user_id.as_deref(), Some("987654321"));

        // 3. userId 驼峰别名
        let json_camel = r#"{"name":"test3","userId":"55555","jwt":"eyJ..."}"#;
        let acc3: RawAccount = serde_json::from_str(json_camel).unwrap();
        assert_eq!(acc3.user_id.as_deref(), Some("55555"));

        // 4. UserID 为空或 null
        let json_null = r#"{"name":"test4","UserID":null,"jwt":"eyJ..."}"#;
        let acc4: RawAccount = serde_json::from_str(json_null).unwrap();
        assert_eq!(acc4.user_id, None);
    }

    #[test]
    fn test_accounts_file_formats() {
        // 标准对象格式
        let obj_json = r#"{"accounts":[{"name":"a1","UserID":"111","jwt":"j1"}]}"#;
        let file1: AccountsFile = serde_json::from_str(obj_json).unwrap();
        assert_eq!(file1.accounts.len(), 1);
        assert_eq!(file1.accounts[0].name, "a1");

        // 数组格式
        let arr_json = r#"[{"name":"a2","UserID":"222","jwt":"j2"},{"name":"a3","UserID":333,"jwt":"j3"}]"#;
        let file2: AccountsFile = serde_json::from_str(arr_json).unwrap();
        assert_eq!(file2.accounts.len(), 2);
        assert_eq!(file2.accounts[0].name, "a2");
        assert_eq!(file2.accounts[1].user_id.as_deref(), Some("333"));
    }

    #[test]
    fn test_stable_account_id() {
        let mut a = RawAccount {
            id: Some("custom-id-1".into()),
            name: "acc1".into(),
            user_id: Some("123".into()),
            jwt: "token1".into(),
            ..Default::default()
        };
        assert_eq!(build_stable_account_id(&a), "custom-id-1");

        a.id = None;
        let id1 = build_stable_account_id(&a);
        assert!(id1.starts_with("acc-"));
        // 幂等：再次计算相同
        assert_eq!(build_stable_account_id(&a), id1);

        // 不同账号生成不同 id
        let a2 = RawAccount {
            id: None,
            name: "acc2".into(),
            user_id: Some("123".into()), // 即使 user_id 相同，name 不同其 id 也不同
            jwt: "token2".into(),
            ..Default::default()
        };
        let id2 = build_stable_account_id(&a2);
        assert_ne!(id1, id2);
    }
}
