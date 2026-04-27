use std::sync::Arc;

use axum::Router;

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

use crate::auth::local::LocalAuthAdapter;
use crate::auth::AuthAdapter;
use crate::handlers;
use crate::SchemaStore;

pub struct AppState {
    pub data_adapter: Box<dyn DataAdapter>,
    pub auth_adapter: Box<dyn AuthAdapter>,
    pub schema: SchemaStore,
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

fn build_auth_adapter(root: &std::path::Path) -> Box<dyn AuthAdapter> {
    let adapter_type = std::env::var("LOCO_AUTH_ADAPTER").unwrap_or_else(|_| "local".to_string());
    match adapter_type.as_str() {
        "local" => {
            let path = root.join("auth");
            println!("Using local filesystem auth adapter ({})", path.display());
            Box::new(LocalAuthAdapter::new(&path))
        }
        other => panic!("unknown LOCO_AUTH_ADAPTER: {other} (expected \"local\")"),
    }
}

pub fn build_app() -> Router {
    build_app_with_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
}

pub fn build_app_with_root(root: &std::path::Path) -> Router {
    // Load schema from disk into a fresh store
    let instances_dir = root.join("schemas/instances");
    let schema = SchemaStore::load(&instances_dir).expect("failed to load schema");

    let data_adapter = build_data_adapter();
    let auth_adapter = build_auth_adapter(root);
    println!("Sites are managed in schemas/instances/");

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
}
