use std::sync::Arc;

use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::auth;
use super::routes;
use super::ApiSharedState;

/// API 服务器句柄：用于优雅停止
pub struct ApiServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl ApiServerHandle {
    /// 发送 shutdown 信号并 abort 线程
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.join_handle.take() {
            h.abort();
        }
    }
}

impl Drop for ApiServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 启动 axum HTTP 服务器
///
/// 使用 Tauri 内置 tokio runtime，不新建 runtime。
pub async fn start_api_server(
    port: u16,
    state: Arc<ApiSharedState>,
) -> Result<ApiServerHandle, String> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("端口 {} 绑定失败: {}", port, e))?;

    let app = build_router(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });

    let join_handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("API server error: {}", e);
        }
    });

    Ok(ApiServerHandle {
        shutdown_tx: Some(shutdown_tx),
        join_handle: Some(join_handle),
    })
}

fn build_router(state: Arc<ApiSharedState>) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/status", get(routes::status))
        .route("/v1/models", get(routes::models))
        .route("/v1/chat/completions", post(routes::chat_completions))
        .layer(from_fn_with_state(state.clone(), auth::bearer_auth))
        .with_state(state)
}
