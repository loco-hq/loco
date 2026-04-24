use std::sync::Arc;

use axum::Router;

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

use crate::auth::local::LocalAuthAdapter;
use crate::auth::AuthAdapter;
use crate::handlers;
use crate::SchemaStore;

pub struct AppState {
    pub adapter: Box<dyn DataAdapter>,
    pub auth: Box<dyn AuthAdapter>,
    pub schema: SchemaStore,
}

fn build_adapter() -> Box<dyn DataAdapter> {
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

pub fn build_app() -> Router {
    build_app_with_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
}

pub fn build_app_with_root(root: &std::path::Path) -> Router {
    // Load schema from disk into a fresh store
    let instances_dir = root.join("schemas/instances");
    let schema = SchemaStore::load(&instances_dir).expect("failed to load schema");

    crate::manifest::validate_manifests(&schema).expect("manifest validation failed");

    let adapter = build_adapter();

    let auth_adapter: Box<dyn AuthAdapter> = Box::new(LocalAuthAdapter::new(&root.join("auth")));
    println!("Using local filesystem auth adapter (auth/)");
    println!("Sites are managed in schemas/instances/");

    let state = Arc::new(AppState {
        adapter,
        auth: auth_adapter,
        schema,
    });

    Router::new()
        .nest("/data", handlers::data::router())
        .nest("/schema", handlers::schema::router())
        .nest("/config", handlers::config::router())
        .nest("/auth", handlers::auth::router())
        .with_state(state)
}
