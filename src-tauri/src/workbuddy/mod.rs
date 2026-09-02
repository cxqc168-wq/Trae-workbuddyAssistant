pub mod accounts;
pub mod auth_file;
pub mod checkin;
pub mod credits;
pub mod http;
pub mod refresh;

use std::path::PathBuf;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 数据目录：默认 %APPDATA%\TraeWorkAssistant\data；
/// 测试通过 TRAEWB_TEST_STORE 注入临时目录。
pub fn store_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TRAEWB_TEST_STORE") {
        return PathBuf::from(dir);
    }
    std::env::var("APPDATA")
        .map(|a| PathBuf::from(a).join("TraeWorkAssistant").join("data"))
        .unwrap_or_default()
}
