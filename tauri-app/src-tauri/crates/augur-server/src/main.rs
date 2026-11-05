//! The Augur Git web server: the shared application service layer served to
//! browsers over HTTP.
//!
//! One process owns the application state exactly as the desktop process does.
//! Browsers invoke the same named commands the webview does, receive the same
//! events over a websocket, and share one set of open repositories.

mod browse;
mod dispatch;
mod sink;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::Value;

use augur_service::state::{AppState, ServiceStartup};
use augur_service::store::JsonFileStore;

use crate::sink::BroadcastSink;

/// Where the server looks for the built frontend, in order.
const ASSET_SEARCH_PATHS: &[&str] = &["../dist", "dist", "/usr/share/augur-git/web"];

#[derive(Clone)]
struct ServerContext {
    state: Arc<AppState>,
    sink: Arc<BroadcastSink>,
    token: Option<String>,
    /// Optional sandbox for the directory-browsing endpoint.
    repo_root: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    // The keychain client logs its D-Bus handshake at INFO; keep the server log
    // about this application.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,zbus=warn,secret_service=warn,tracing=warn"),
    )
    .init();

    let options = Options::parse(std::env::args().skip(1));
    let host = options.host.clone();
    let port = options.port;

    let config_dir = options.config_dir.clone().unwrap_or_else(default_config_dir);
    let settings_store = JsonFileStore::open_in(&config_dir, "settings.json")
        .expect("the settings document opens");
    let workspace_store = JsonFileStore::open_in(&config_dir, "workspace.json")
        .expect("the workspace document opens");

    let secrets = augur_service::credentials::SecretStores::with_plaintext(
        Arc::new(augur_service::credentials::KeychainSecretStore::new()),
        Some(config_dir.join("credential-secrets.json")),
    );
    let sink = Arc::new(BroadcastSink::new(1024));
    let state = Arc::new(AppState::new(
        sink.clone(),
        ServiceStartup {
            settings_store: Arc::new(settings_store),
            workspace_store: Arc::new(workspace_store),
            secrets,
            warnings: Vec::new(),
        },
    ));
    let credentials = state.credentials();

    let token = options.resolve_token(&config_dir);
    if let Some(token) = &token {
        log::info!("[server] access token required on every request");
        println!("Augur Git web token: {token}");
    } else {
        log::warn!("[server] running without an access token; anyone on the network can read and write the repositories this process can reach");
    }

    // HTTPS credential prompts surface in every connected browser, exactly as
    // they surface in the desktop's webview.
    match augur_service::askpass::AskpassServer::start(sink.clone(), credentials) {
        Ok(_server) => {}
        Err(error) => {
            log::warn!("[git_askpass] HTTPS credential prompts are unavailable: {error}")
        }
    }

    let assets = options
        .assets
        .clone()
        .or_else(find_assets)
        .unwrap_or_else(|| {
            log::warn!("[server] no built frontend found; only the API is served");
            PathBuf::from(".")
        });
    log::info!("[server] serving the frontend from {}", assets.display());

    let context = ServerContext {
        state: state.clone(),
        sink: sink.clone(),
        token,
        repo_root: options.repo_root.clone(),
    };

    let api = Router::new()
        .route("/invoke/{command}", post(invoke))
        .route("/events", get(events))
        .route("/browse", post(browse::browse))
        .layer(axum::middleware::from_fn_with_state(
            context.clone(),
            authenticate,
        ));

    let index = assets.join("index.html");
    let static_files = tower_http::services::ServeDir::new(&assets)
        .not_found_service(tower_http::services::ServeFile::new(index));

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(static_files)
        .with_state(context);

    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .unwrap_or_else(|error| panic!("failed to bind {host}:{port}: {error}"));
    log::info!("[server] listening on http://{host}:{port}");
    println!("Augur Git web server: http://{host}:{port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("the server runs to completion");

    state.shutdown();
    log::info!("[server] stopped");
}

/// Command-line options, parsed by hand: the surface is small and stays that
/// way without a dependency.
struct Options {
    host: String,
    port: u16,
    token: Option<String>,
    no_token: bool,
    assets: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    repo_root: Option<PathBuf>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut parsed = Self {
            host: "0.0.0.0".to_string(),
            port: 8321,
            token: None,
            no_token: false,
            assets: None,
            config_dir: None,
            repo_root: None,
        };
        let mut args = args;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--host" => parsed.host = args.next().unwrap_or_else(|| missing("--host")),
                "--port" => {
                    parsed.port = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or_else(|| missing("--port"));
                }
                "--token" => parsed.token = Some(args.next().unwrap_or_else(|| missing("--token"))),
                "--no-token" => parsed.no_token = true,
                "--assets" => {
                    parsed.assets = Some(PathBuf::from(args.next().unwrap_or_else(|| missing("--assets"))))
                }
                "--config-dir" => {
                    parsed.config_dir =
                        Some(PathBuf::from(args.next().unwrap_or_else(|| missing("--config-dir"))))
                }
                "--repo-root" => {
                    parsed.repo_root =
                        Some(PathBuf::from(args.next().unwrap_or_else(|| missing("--repo-root"))))
                }
                other => {
                    eprintln!("Unknown argument: {other}");
                    eprintln!(
                        "Usage: augur-git-server [--host ADDR] [--port N] [--token T | --no-token] \
                         [--assets DIR] [--config-dir DIR] [--repo-root DIR]"
                    );
                    std::process::exit(2);
                }
            }
        }
        parsed
    }

    /// The access token: explicit, generated, or none on request. A generated
    /// token is printed once and kept in the configuration directory so a
    /// restart does not silently invalidate everyone's bookmark.
    fn resolve_token(&self, config_dir: &std::path::Path) -> Option<String> {
        if self.no_token {
            return None;
        }
        if let Some(token) = &self.token {
            return Some(token.clone());
        }
        let token_file = config_dir.join("token");
        if let Ok(existing) = std::fs::read_to_string(&token_file) {
            let token = existing.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        if let Some(parent) = token_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&token_file, format!("{token}\n")) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &token_file,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
            }
            Err(error) => log::warn!("[server] could not persist the token file: {error}"),
        }
        Some(token)
    }
}

fn missing(flag: &str) -> ! {
    eprintln!("Missing value for {flag}");
    std::process::exit(2);
}

fn default_config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("augur-git/web");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/augur-git/web");
    }
    PathBuf::from(".augur-git-web")
}

fn find_assets() -> Option<PathBuf> {
    ASSET_SEARCH_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.join("index.html").is_file())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    log::info!("[server] shutdown requested");
}

// --- HTTP surface ---

/// Every API request carries the token, either as a bearer header (fetch) or
/// as a query parameter (websocket, where browsers cannot set headers).
fn token_matches(context: &ServerContext, headers: &HeaderMap, query: &str) -> bool {
    let Some(expected) = &context.token else {
        return true;
    };
    if let Some(value) = headers.get("authorization").and_then(|value| value.to_str().ok()) {
        let provided = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
            .unwrap_or(value);
        if constant_time_eq(provided, expected) {
            return true;
        }
    }
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=")
            && constant_time_eq(value, expected)
        {
            return true;
        }
    }
    false
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

async fn authenticate(
    State(context): State<ServerContext>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let query = request.uri().query().unwrap_or_default().to_string();
    if !token_matches(&context, request.headers(), &query) {
        return (StatusCode::UNAUTHORIZED, "invalid access token").into_response();
    }
    next.run(request).await
}

async fn invoke(
    State(context): State<ServerContext>,
    Path(command): Path<String>,
    Json(args): Json<Value>,
) -> Response {
    match dispatch::dispatch(&context.state, &command, args).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    token: Option<String>,
}

async fn events(
    State(context): State<ServerContext>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Re-check the token here: the middleware only sees what the upgrade
    // request carried, and this doubles as the explicit documentation point
    // that the websocket is authenticated like every other route.
    let query_text = query
        .token
        .as_deref()
        .map(|token| format!("token={token}"))
        .unwrap_or_default();
    if !token_matches(&context, &headers, &query_text) {
        return (StatusCode::UNAUTHORIZED, "invalid access token").into_response();
    }
    upgrade.on_upgrade(move |socket| forward_events(context, socket))
}

async fn forward_events(context: ServerContext, mut socket: WebSocket) {
    let mut receiver = context.sink.subscribe();
    loop {
        match receiver.recv().await {
            Ok(event) => {
                let frame = serde_json::json!({ "event": event.name, "payload": event.payload });
                match serde_json::to_string(&frame) {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => log::error!("[events] failed to encode {frame:?}: {error}"),
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                log::warn!("[events] a client lagged behind by {skipped} events");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_ignores_timing_but_not_content() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "secrets"));
        assert!(!constant_time_eq("secret", "secretx"));
        assert!(!constant_time_eq("", "secret"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn an_explicit_token_short_circuits_the_token_file() {
        let directory = std::env::temp_dir().join(format!(
            "augur-token-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let options = Options {
            host: String::new(),
            port: 0,
            token: Some("given".into()),
            no_token: false,
            assets: None,
            config_dir: Some(directory.clone()),
            repo_root: None,
        };
        assert_eq!(options.resolve_token(&directory).as_deref(), Some("given"));
        assert!(!directory.join("token").exists());
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn opting_out_disables_authentication() {
        let directory = std::env::temp_dir().join(format!(
            "augur-token-off-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let options = Options {
            host: String::new(),
            port: 0,
            token: None,
            no_token: true,
            assets: None,
            config_dir: Some(directory.clone()),
            repo_root: None,
        };
        assert_eq!(options.resolve_token(&directory), None);
        std::fs::remove_dir_all(directory).ok();
    }
}
