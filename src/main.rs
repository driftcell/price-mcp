mod db;
mod models;
mod server;
mod store;
mod write_models;
mod write_store;

use std::path::PathBuf;

use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use clap::{Parser, ValueEnum};
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::json;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::server::PriceBookServer;
use crate::store::PriceBookStore;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// Serve Streamable HTTP over a TCP socket.
    Http,
    /// Serve MCP over stdin/stdout, for clients that launch the process themselves.
    Stdio,
}

#[derive(Debug, Parser)]
#[command(version, about = "PriceBook MCP server")]
struct Args {
    /// SQLite database path.
    #[arg(long, default_value = "data/pricebook.db")]
    db: PathBuf,

    /// MCP transport to serve.
    #[arg(long, value_enum, default_value_t = Transport::Http)]
    transport: Transport,

    /// HTTP bind host (ignored with --transport stdio).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// HTTP bind port (ignored with --transport stdio).
    #[arg(long, default_value_t = 8787)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout is the MCP protocol channel in stdio mode, so logs always go to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,rmcp=info".into()),
        )
        .init();

    let args = Args::parse();
    let pool = db::open(&args.db).await?;
    let store = PriceBookStore::new(pool);

    match args.transport {
        Transport::Stdio => serve_stdio(store).await,
        Transport::Http => serve_http(store, &args).await,
    }
}

async fn serve_stdio(store: PriceBookStore) -> anyhow::Result<()> {
    let service = PriceBookServer::new(store)
        .serve(rmcp::transport::stdio())
        .await
        .context("failed to start stdio MCP service")?;

    tracing::info!("pricebook MCP serving over stdio");

    service
        .waiting()
        .await
        .context("stdio MCP service stopped unexpectedly")?;
    Ok(())
}

async fn serve_http(store: PriceBookStore, args: &Args) -> anyhow::Result<()> {
    let service: StreamableHttpService<PriceBookServer, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let store = store.clone();
                move || Ok(PriceBookServer::new(store.clone()))
            },
            Default::default(),
            StreamableHttpServerConfig::default(),
        );

    let app = Router::new()
        .route("/health", get(health))
        .nest_service("/mcp", service);

    let listener = TcpListener::bind((args.host.as_str(), args.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", args.host, args.port))?;
    let address = listener.local_addr()?;

    tracing::info!(
        database = %args.db.display(),
        %address,
        "pricebook MCP listening at http://{address}/mcp"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "pricebook-mcp" }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
