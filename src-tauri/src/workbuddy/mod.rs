pub mod accounts;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
