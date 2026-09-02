pub mod accounts;
#[allow(dead_code)] // command 接线在后续任务，先允许未引用告警
pub mod auth_file;
#[allow(dead_code)] // command 接线在后续任务，先允许未引用告警
pub mod checkin;
#[allow(dead_code)] // command 接线在后续任务，先允许未引用告警
pub mod http;
#[allow(dead_code)] // command 接线在后续任务，先允许未引用告警
pub mod refresh;

use std::path::PathBuf;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 数据目录：默认 %APPDATA%\TraeWorkAssistant\data。
pub fn store_path() -> PathBuf {
    std::env::var("APPDATA")
        .map(|a| PathBuf::from(a).join("TraeWorkAssistant").join("data"))
        .unwrap_or_default()
}
