use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

use crate::auth::local::LocalAuthAdapter;
use crate::auth::AuthAdapter;
use crate::handlers;
use crate::SchemaStore;

pub struct AppState {
    pub data_adapter: Box<dyn DataAdapter>,
    pub auth_adapter: Box<dyn AuthAdapter>,
    pub schema: Arc<SchemaStore>,
}

fn build_data_adapter() -> Box<dyn DataAdapter> {
    let adapter_type = std::env::var("LOCO_ADAPTER").unwrap_or_else(|_| "sqlite".to_string());
    match adapter_type.as_str() {
        "memory" => {
            println!("Using in-memory adapter");
            Box::new(InMemoryAdapter::new())
        }
        "sqlite" => {
            let path = std::env::var("LOCO_DB_PATH").unwrap_or_else(|_| "loco.db".to_string());
            println!("Using SQLite adapter ({path})");
            Box::new(
                SqliteAdapter::new(std::path::Path::new(&path))
                    .expect("failed to open SQLite database"),
            )
        }
        other => panic!("unknown LOCO_ADAPTER: {other} (expected \"sqlite\" or \"memory\")"),
    }
}

/// Overrides a caller can pin instead of reading the environment. Tests use
/// this so one process can host servers that disagree about a flag.
#[derive(Default)]
pub struct AppOptions {
    /// `None` → `LOCO_AUTH_AUTO_CREATE` decides (off unless set).
    pub auth_auto_create: Option<bool>,
}

fn build_auth_adapter(root: &std::path::Path, options: &AppOptions) -> Box<dyn AuthAdapter> {
    let adapter_type = std::env::var("LOCO_AUTH_ADAPTER").unwrap_or_else(|_| "local".to_string());
    match adapter_type.as_str() {
        "local" => {
            let path = root.join("auth");
            println!("Using local filesystem auth adapter ({})", path.display());
            Box::new(match options.auth_auto_create {
                Some(auto_create) => LocalAuthAdapter::with_auto_create(&path, auto_create),
                None => LocalAuthAdapter::new(&path),
            })
        }
        other => panic!("unknown LOCO_AUTH_ADAPTER: {other} (expected \"local\")"),
    }
}

pub fn build_app() -> Router {
    build_app_with_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
}

pub fn build_app_with_root(root: &std::path::Path) -> Router {
    build_app_with_options(root, AppOptions::default())
}

pub fn build_app_with_options(root: &std::path::Path, options: AppOptions) -> Router {
    // Load schema from disk into a fresh store
    let instances_dir = root.join("schemas/instances");
    let schema = Arc::new(SchemaStore::load(&instances_dir).expect("failed to load schema"));

    let data_adapter = build_data_adapter();
    let auth_adapter = build_auth_adapter(root, &options);

    let state = Arc::new(AppState {
        data_adapter,
        auth_adapter,
        schema,
    });

    Router::new()
        .nest("/data", handlers::data::router())
        .nest("/schema", handlers::schema::router())
        .nest("/config", handlers::config::router())
        .nest("/auth", handlers::auth::router())
        .with_state(state)
        // Outermost so OPTIONS preflight never hits auth extractors, and so
        // 404s still carry CORS headers (a missing header looks like a CORS
        // failure in the browser). Any origin / method / header; no cookies.
        // Studio's Vite proxy is unchanged.
        .layer(cors_layer())
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}
