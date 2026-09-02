use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::ApiSharedState;

/// Bearer Token 鉴权中间件：
/// - /health 跳过鉴权
/// - api_key 为空时跳过鉴权
/// - 否则校验 Authorization: Bearer <key>
pub async fn bearer_auth(
    State(state): State<Arc<ApiSharedState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == "/health" {
        return next.run(request).await;
    }
    if state.api_key.is_empty() {
        return next.run(request).await;
    }
    let authz = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    match authz {
        Some(s) if s.len() > 7 && s[..7].eq_ignore_ascii_case("Bearer ") => {
            let key = &s[7..];
            if key == state.api_key {
                next.run(request).await
            } else {
                (StatusCode::UNAUTHORIZED, "invalid api key").into_response()
            }
        }
        _ => (StatusCode::UNAUTHORIZED, "missing api key").into_response(),
    }
}
