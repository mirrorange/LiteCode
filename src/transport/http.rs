use std::net::SocketAddr;

use axum::Router;
use rmcp::transport::{
    StreamableHttpServerConfig,
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
};
use tokio_util::sync::CancellationToken;

use crate::{error::Result, server::LiteCodeServer};

pub async fn serve(server: LiteCodeServer, bind: SocketAddr) -> Result<()> {
    let cancellation_token = CancellationToken::new();
    let shutdown_token = cancellation_token.clone();
    let service: StreamableHttpService<LiteCodeServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            Default::default(),
            StreamableHttpServerConfig {
                stateful_mode: true,
                sse_keep_alive: Some(std::time::Duration::from_secs(15)),
                cancellation_token,
                ..Default::default()
            },
        );

    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(bind).await?;

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_token.cancel();
        })
        .await?;

    Ok(())
}
