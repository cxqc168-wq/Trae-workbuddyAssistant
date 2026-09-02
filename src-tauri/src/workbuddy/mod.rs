pub mod accounts;
#[allow(dead_code)] // command 接线在后续任务，先允许未引用告警
pub mod auth_file;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
