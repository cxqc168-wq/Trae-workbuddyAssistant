use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Clone, Default)]
pub struct AccountView {
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

#[derive(Serialize, Deserialize, Clone)]
pub struct RawAccount {
    pub name: String,
    #[serde(rename = "UserID", default)]
    pub user_id: Option<String>,
    pub jwt: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub added_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct AccountsFile {
    pub accounts: Vec<RawAccount>,
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
    #[serde(default = "default_true")]
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
