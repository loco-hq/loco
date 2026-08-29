use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

use crate::auth::local::LocalAuthAdapter;
use crate::auth::AuthAdapter;
use crate::handlers;
use crate::http::host;
use crate::{Bundle, SchemaStore, Site};

pub struct AppState {
    pub data_adapter: Box<dyn DataAdapter>,
    pub auth_adapter: Box<dyn AuthAdapter>,
    pub schema: Arc<SchemaStore>,
    /// The site the apex serves at `/`, as `({account}/{project}, {site})`.
    /// `None` is the API-only process. A host that names a site of its own
    /// always wins over this.
    pub default_site: Option<(String, String)>,
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
    /// Which site the apex serves at `/`, as `{account}/{project}/{site}`.
    /// `None` → `LOCO_DEFAULT_SITE` decides (unset → the apex is API-only).
    ///
    /// There is no default here and there must not be one: a Loco process is
    /// not a Studio process. Whoever runs it says which app it hosts.
    pub default_site: Option<String>,
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
    let default_site = resolve_default_site(&schema, &options);

    let state = Arc::new(AppState {
        data_adapter,
        auth_adapter,
        schema,
        default_site,
    });

    Router::new()
        .nest("/data", handlers::data::router())
        .nest("/schema", handlers::schema::router())
        .nest("/config", handlers::config::router())
        .nest("/auth", handlers::auth::router())
        // Everything the API does not own is a request for the site's pinned
        // version bundle. Reserved prefixes are re-checked inside, because a
        // nested router with no fallback of its own lands here too and a
        // mistyped `/data/...` must stay a JSON 404.
        .fallback(handlers::hosting::serve_site_files)
        // Under the API and the fallback both, so they agree on which site
        // the URL names. Above `with_state` so it can read the store.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            host::resolve_site,
        ))
        .with_state(state)
        // Outermost so OPTIONS preflight never hits auth extractors, and so
        // 404s still carry CORS headers (a missing header looks like a CORS
        // failure in the browser). Any origin / method / header; no cookies.
        // Studio's Vite proxy is unchanged.
        .layer(cors_layer())
}

/// Read `LOCO_DEFAULT_SITE` (or the pinned option), and say once at boot what
/// the apex will do with it.
///
/// Every problem here is a warning, never a panic. A process whose default
/// site has no bundle yet is a process mid-deploy: it serves its API and 404s
/// `/` until something is uploaded.
fn resolve_default_site(
    schema: &Arc<SchemaStore>,
    options: &AppOptions,
) -> Option<(String, String)> {
    let raw = options
        .default_site
        .clone()
        .or_else(|| std::env::var("LOCO_DEFAULT_SITE").ok())?;
    if raw.trim().is_empty() {
        return None;
    }

    let Some((project_id, site_name)) = host::parse_site_ref(&raw) else {
        eprintln!(
            "LOCO_DEFAULT_SITE={raw} is not {{account}}/{{project}}/{{site}};              the apex stays API-only"
        );
        return None;
    };

    match schema.sites().get(&Site::to_path(&project_id, &site_name)) {
        None => eprintln!("Apex site {project_id}/{site_name} does not exist; / will 404"),
        Some(site)
            if !schema
                .bundles()
                .has(&Bundle::to_path(&project_id, site.version())) =>
        {
            eprintln!(
                "Apex site {project_id}/{site_name} pins version {}, which has no bundle; \
                 / will 404 until one is uploaded",
                site.version()
            );
        }
        Some(site) => println!(
            "Serving {project_id}/{site_name} (version {}) at /",
            site.version()
        ),
    }

    Some((project_id, site_name))
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}
