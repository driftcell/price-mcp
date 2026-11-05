mod db;
mod models;
mod server;
mod store;
mod write_models;
mod write_store;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::{Json, Router, routing::get};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::json;
use tower_service::Service;
use worker::*;

use crate::server::PriceBookServer;
use crate::store::PriceBookStore;

// Emscripten links a binary; workers-rs supplies the actual Worker entrypoint.
fn main() {}

fn router(env: &Env) -> Result<Router> {
    let store = PriceBookStore::new(Arc::new(env.d1("DB")?));
    let allowed_hosts = env
        .var("MCP_ALLOWED_HOSTS")
        .map(|value| {
            value
                .to_string()
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| vec!["localhost".to_owned(), "127.0.0.1".to_owned()]);

    let service: StreamableHttpService<PriceBookServer, NeverSessionManager> =
        StreamableHttpService::new(
            {
                let store = store.clone();
                move || Ok(PriceBookServer::new(store.clone()))
            },
            Arc::new(NeverSessionManager::default()),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(false)
                .with_json_response(true)
                .with_sse_keep_alive(None)
                .with_sse_retry(None)
                .with_allowed_hosts(allowed_hosts),
        );

    Ok(Router::new()
        .route("/health", get(health))
        .nest_service("/mcp", service))
}

#[cfg_attr(target_arch = "wasm32", event(fetch))]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<axum::http::Response<Body>> {
    if req.uri().path().starts_with("/mcp") && !is_authorized(&req, &env)? {
        return Ok(unauthorized());
    }
    Ok(router(&env)?.call(req).await?)
}

fn is_authorized(req: &HttpRequest, env: &Env) -> Result<bool> {
    let token = env.secret("MCP_BEARER_TOKEN")?.to_string();
    let expected = format!("Bearer {token}");
    Ok(req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected))
}

fn unauthorized() -> Response<Body> {
    let mut response = Response::new(Body::from("missing or invalid bearer token"));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "pricebook-mcp",
        "database": "d1",
        "transport": "streamable-http"
    }))
}
