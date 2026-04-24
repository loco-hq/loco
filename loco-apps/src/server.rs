use std::sync::Arc;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

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

    let all_collections = schema.collections().list_all();
    println!("Loaded {} collection(s):", all_collections.len());
    for (ns, _) in &all_collections {
        println!("  - {ns}");
    }

    let all_fields = schema.fields().list_all();
    println!("Loaded {} field(s):", all_fields.len());
    for (ns, _) in &all_fields {
        println!("  - {ns}");
    }

    let all_sites = schema.sites().list_all();
    println!("Loaded {} site(s):", all_sites.len());
    for (id, _) in &all_sites {
        println!("  - {id}");
    }

    let adapter = build_adapter();

    let auth_adapter: Box<dyn AuthAdapter> = Box::new(LocalAuthAdapter::new(&root.join("auth")));
    println!("Using local filesystem auth adapter (auth/)");
    println!("Sites are managed in schemas/instances/");

    let state = Arc::new(AppState {
        adapter,
        auth: auth_adapter,
        schema,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Data endpoints
        .route(
            "/{user}/{project}/collection/{name}/add",
            post(handlers::data::add),
        )
        .route(
            "/{user}/{project}/collection/{name}/list",
            get(handlers::data::list),
        )
        .route(
            "/{user}/{project}/collection/{name}/get/{id}",
            get(handlers::data::get),
        )
        .route(
            "/{user}/{project}/collection/{name}/update/{id}",
            put(handlers::data::update),
        )
        .route(
            "/{user}/{project}/collection/{name}/delete/{id}",
            delete(handlers::data::delete),
        )
        // Meta endpoint
        .route(
            "/meta/{user}/{project}/{type_name}/list",
            get(handlers::schema::meta_list),
        )
        // Schema CRUD endpoints
        .route(
            "/schema/{user}/{project}/{version}/collection",
            post(handlers::schema::create_collection),
        )
        .route(
            "/schema/{user}/{project}/{version}/collection/list",
            get(handlers::schema::list_collections),
        )
        .route(
            "/schema/{user}/{project}/{version}/collection/{name}",
            get(handlers::schema::get_collection)
                .put(handlers::schema::update_collection)
                .delete(handlers::schema::delete_collection),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}",
            post(handlers::schema::create_field),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}/list",
            get(handlers::schema::list_fields),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}/{name}",
            put(handlers::schema::update_field).delete(handlers::schema::delete_field),
        )
        // Schema introspection
        .route("/schema/collections", get(handlers::schema::introspect))
        // Config CRUD endpoints (global-scope types)
        .route("/config/{type_name}/list", get(handlers::config::list))
        .route("/config/get/{*path}", get(handlers::config::get))
        .route("/config/create/{*path}", post(handlers::config::create))
        .route("/config/update/{*path}", put(handlers::config::update))
        .route("/config/delete/{*path}", delete(handlers::config::delete))
        // Auth endpoints
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/logout", post(handlers::auth::logout))
        .route("/auth/me", get(handlers::auth::me))
        .route("/auth/users", post(handlers::auth::create_user))
        .route("/auth/users/list", get(handlers::auth::list_users))
        .route(
            "/auth/users/{id}",
            put(handlers::auth::update_user).delete(handlers::auth::delete_user),
        )
        .route("/auth/api-keys", post(handlers::auth::create_api_key))
        .route("/auth/api-keys/list", get(handlers::auth::list_api_keys))
        .route("/auth/api-keys/{id}", delete(handlers::auth::revoke_api_key))
        .layer(cors)
        .with_state(state)
}
