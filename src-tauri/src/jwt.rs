//! JWT 解析（不校验签名，仅本地展示用途）。
//! 支持 `Cloud-IDE-JWT <token>` / `Bearer <token>` 前缀；payload 取 data.id 与 exp。

use base64::Engine;

pub struct JwtInfo {
    pub user_id: Option<String>,
    pub exp_hours: Option<f64>,
    pub exp_timestamp: Option<i64>,
}

pub fn parse(jwt_full: &str) -> JwtInfo {
    // 去除首尾空白（用户粘贴时可能带换行/空格）
    let trimmed = jwt_full.trim();
    // 兼容多种前缀：Cloud-IDE-JWT / Bearer / 直接 token
    let token = trimmed
        .strip_prefix("Cloud-IDE-JWT ")
        .or_else(|| trimmed.strip_prefix("Bearer "))
        .unwrap_or(trimmed)
        .trim();
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return JwtInfo {
            user_id: None,
            exp_hours: None,
            exp_timestamp: None,
        };
    }
    // JWT payload 使用 base64url 无填充编码；strip 残余 '=' 后用 URL_SAFE_NO_PAD 解码
    let payload_b64 = parts[1].trim_end_matches('=');
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64) else {
        return JwtInfo {
            user_id: None,
            exp_hours: None,
            exp_timestamp: None,
        };
    };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return JwtInfo {
            user_id: None,
            exp_hours: None,
            exp_timestamp: None,
        };
    };
    let user_id = payload
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| {
            v.as_str().map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .or_else(|| payload.get("auth_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .or_else(|| payload.get("sub").and_then(|v| v.as_str()).map(|s| s.to_string()));

    // exp 可能是整数或浮点数，依次尝试 as_i64 / as_f64 / as_str(数字字符串)
    let exp_timestamp = payload
        .get("exp")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_f64().map(|f| f as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        });

    let exp_hours = exp_timestamp.map(|exp| {
        let now = chrono::Utc::now().timestamp();
        (exp - now) as f64 / 3600.0
    });

    JwtInfo {
        user_id,
        exp_hours,
        exp_timestamp,
    }
}

/// 由 exp 剩余小时数推导状态：>24 ok / <=24 && >0 warn / <=0 expired
pub fn status_of(exp_hours: Option<f64>) -> &'static str {
    match exp_hours {
        Some(h) if h > 24.0 => "ok",
        Some(h) if h > 0.0 => "warn",
        Some(_) => "expired",
        None => "unknown",
    }
}
