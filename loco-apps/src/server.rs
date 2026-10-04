use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

use crate::actions::HandlerRegistry;
use crate::auth::local::LocalAuthAdapter;
use crate::auth::AuthAdapter;
use crate::handlers;
use crate::http::host;
use crate::integrations::{SourceRegistry, TypeActionRegistry};
use crate::seed;
use crate::values::{KeyStatus, LakeSecretStore, SecretStore};
use crate::{Bundle, Project, SchemaStore, Site};

pub struct AppState {
    /// Shared with [`crate::values::LakeSecretStore`]. Secret and variable
    /// rows live in this lake, under `$secrets` and `$variables`.
    pub data_adapter: Arc<dyn DataAdapter>,
    pub auth_adapter: Box<dyn AuthAdapter>,
    pub schema: Arc<SchemaStore>,
    /// Plaintext trait. The lake impl encrypts. Handlers clone this `Arc`.
    /// See `crate::values`.
    pub secrets: Arc<dyn SecretStore>,
    /// Shared by action handlers. No proxy. A redirect that changes scheme,
    /// host, or port is not followed. See [`crate::actions::http_client`].
    pub http: reqwest::Client,
    /// The site the apex serves at `/`, as `({account}/{project}, {site})`.
    /// `None` is the API-only process. A host that names a site of its own
    /// always wins over this.
    pub default_site: Option<(String, String)>,
    /// Declared-action handlers, keyed by owning project and bare name.
    /// Empty unless a caller of [`build_app_with_options`] registers some.
    /// The server binary does not.
    pub actions: HandlerRegistry,
    /// Integration-type sources, keyed by owning project and type name.
    /// Empty in the server binary. The value is a placeholder until a source
    /// trait exists.
    pub sources: SourceRegistry,
    /// Type-action handlers, keyed by owning project, type name, and action
    /// name. Empty in the server binary, so a resolved type action is 501.
    /// A registered handler runs with the connection the address named.
    /// Sources are not dispatched.
    pub type_actions: TypeActionRegistry,
}

fn build_data_adapter(sqlite_path: Option<&Path>) -> Box<dyn DataAdapter> {
    let adapter_type = std::env::var("LOCO_ADAPTER").unwrap_or_else(|_| "sqlite".to_string());
    match adapter_type.as_str() {
        "memory" => {
            println!("Using in-memory adapter");
            Box::new(InMemoryAdapter::new())
        }
        "sqlite" => {
            // No root here. Callers that resolved against `LOCO_ROOT` pass
            // the path in. Everyone else, including tests, keeps the env
            // string relative to the working directory.
            let path = sqlite_path.map(Path::to_path_buf).unwrap_or_else(|| {
                resolve_sqlite_path(None, std::env::var("LOCO_DB_PATH").ok().as_deref())
            });
            println!("Using SQLite adapter ({})", path.display());
            Box::new(SqliteAdapter::new(&path).expect("failed to open SQLite database"))
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
    /// Handlers for declared actions. [`Default`] registers none, which is
    /// what [`build_app`] ships. The Hurl fixture handler is registered by
    /// the test runner, so it is not in the server binary.
    pub actions: HandlerRegistry,
    /// Sources for integration types. [`Default`] registers none.
    pub sources: SourceRegistry,
    /// Handlers for type actions. [`Default`] registers none, so a resolved
    /// type action is 501. A registered handler receives the connection the
    /// address named.
    pub type_actions: TypeActionRegistry,
    /// SQLite file to open when the adapter is `sqlite`.
    ///
    /// `None` reads `LOCO_DB_PATH` (default `loco.db`) and opens that path
    /// as given, so a relative path stays relative to the working directory.
    /// [`build_app`] leaves this unset. `main` sets it after
    /// [`resolve_sqlite_path`]: joined onto `LOCO_ROOT` only when that
    /// variable is set, and left relative when it is not.
    pub sqlite_path: Option<PathBuf>,
}

/// Default SQLite file name. A relative path. See [`resolve_sqlite_path`].
const DEFAULT_SQLITE_FILE: &str = "loco.db";

/// Crate directory (`loco-apps/`). The schema and auth root when `LOCO_ROOT`
/// is unset.
pub fn default_data_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `LOCO_ROOT` when it names a directory. Blank is unset: the process keeps
/// [`default_data_root`] and does not move the SQLite file.
pub fn parse_loco_root(value: Option<&str>) -> Option<PathBuf> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

/// SQLite path the process opens.
///
/// `root` is `Some` only when `LOCO_ROOT` is set. The default file
/// (`loco.db`) and a relative `LOCO_DB_PATH` are then joined onto that
/// root. When `root` is `None`, the path is `LOCO_DB_PATH` or `loco.db`
/// and a relative path stays relative to the working directory.
///
/// `cargo run -p loco-apps` from the repo root, with `LOCO_ROOT` unset,
/// opens `./loco.db` there. The schema root in that case is still
/// `loco-apps/`. Joining the database onto that directory would open a
/// different file.
///
/// An absolute `LOCO_DB_PATH` is used as given in both cases. An empty
/// string is a relative path, the same as an empty `LOCO_DB_PATH` was
/// before `LOCO_ROOT` existed.
pub fn resolve_sqlite_path(root: Option<&Path>, db_path: Option<&str>) -> PathBuf {
    let raw = db_path.unwrap_or(DEFAULT_SQLITE_FILE);
    let path = Path::new(raw);
    match root {
        Some(root) if path.is_relative() => root.join(path),
        _ => PathBuf::from(raw),
    }
}

/// Absolute form of `path` for the startup log. The file does not have to
/// exist, and symlinks are left as written. This does not change the path
/// [`resolve_sqlite_path`] returns: with `LOCO_ROOT` unset that path stays
/// relative so the open follows the working directory.
pub fn absolute_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `PORT`, or 3000 when the variable is unset. Blank and non-numeric values
/// are errors. `0` and `65535` are in range.
pub fn resolve_port(value: Option<&str>) -> Result<u16, String> {
    let Some(raw) = value else {
        return Ok(3000);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("PORT is empty; set a number from 0 to 65535".to_string());
    }
    raw.parse::<u16>()
        .map_err(|_| format!("PORT must be a number from 0 to 65535, got {raw}"))
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
    build_app_with_root(default_data_root())
}

pub fn build_app_with_root(root: &std::path::Path) -> Router {
    build_app_with_options(root, AppOptions::default())
}

pub fn build_app_with_options(root: &std::path::Path, options: AppOptions) -> Router {
    // Seed committed projects the store lacks, then load the store. Writes
    // go only to `schemas/instances/`; `schemas/seed/` is read, never written.
    let instances_dir = root.join("schemas/instances");
    let seeded = seed::seed_instances(&root.join("schemas/seed"), &instances_dir)
        .expect("failed to seed schema instances");
    for project in &seeded {
        println!("Seeded {project} from schemas/seed");
    }
    let schema = Arc::new(SchemaStore::load(&instances_dir).expect("failed to load schema"));
    let secret_key = KeyStatus::from_env();
    match &secret_key {
        KeyStatus::Missing => {
            eprintln!("LOCO_SECRET_KEY is not set; PUT /config/secret will return 503")
        }
        KeyStatus::Invalid(msg) => eprintln!("{msg}; PUT /config/secret will return 503"),
        KeyStatus::Ready(_) => {}
    }

    let data_adapter: Arc<dyn DataAdapter> =
        Arc::from(build_data_adapter(options.sqlite_path.as_deref()));
    let secrets: Arc<dyn SecretStore> =
        Arc::new(LakeSecretStore::new(data_adapter.clone(), secret_key));
    let http = crate::actions::http_client();
    let auth_adapter = build_auth_adapter(root, &options);
    warn_projects_without_account(&schema, auth_adapter.as_ref());
    let default_site = resolve_default_site(&schema, &options);

    let state = Arc::new(AppState {
        data_adapter,
        auth_adapter,
        schema,
        secrets,
        http,
        default_site,
        actions: options.actions,
        sources: options.sources,
        type_actions: options.type_actions,
    });

    Router::new()
        .nest("/data", handlers::data::router())
        .nest("/schema", handlers::schema::router())
        .nest("/config", handlers::config::router())
        .nest("/auth", handlers::auth::router())
        .nest("/actions", handlers::actions::router())
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

/// A project loads whether or not its account exists, and is unusable until
/// it does: nobody is its developer. Say so once per project at boot.
fn warn_projects_without_account(schema: &SchemaStore, auth: &dyn AuthAdapter) {
    for (key, _) in schema.projects().list_all() {
        let Some(project_id) = Project::from_path(&key).and_then(|v| v.get("project").cloned())
        else {
            continue;
        };
        let account = project_id.split('/').next().unwrap_or_default();
        if let Ok(None) = auth.get_account(account) {
            eprintln!(
                "Project {project_id} is loaded but account {account} does not exist; \
                 create it (POST /config/org) before using the project"
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_path_stays_cwd_relative_when_root_is_unset() {
        let path = resolve_sqlite_path(None, None);
        assert_eq!(path, Path::new("loco.db"));
        assert!(path.is_relative());
        // The schema root is the crate directory. The database is not under it.
        assert!(default_data_root().join("loco.db").is_absolute());
        assert_ne!(path, default_data_root().join("loco.db"));
    }

    #[test]
    fn sqlite_path_keeps_a_relative_override_when_root_is_unset() {
        assert_eq!(
            resolve_sqlite_path(None, Some("data/app.db")),
            Path::new("data/app.db")
        );
        assert_eq!(resolve_sqlite_path(None, Some("")), Path::new(""));
    }

    #[test]
    fn sqlite_path_joins_relative_paths_when_root_is_set() {
        let root = Path::new("/tmp/x");
        assert_eq!(
            resolve_sqlite_path(Some(root), None),
            Path::new("/tmp/x/loco.db")
        );
        assert_eq!(
            resolve_sqlite_path(Some(root), Some("data/app.db")),
            Path::new("/tmp/x/data/app.db")
        );
        assert_eq!(resolve_sqlite_path(Some(root), Some("")), root);
    }

    #[test]
    fn sqlite_path_keeps_an_absolute_override() {
        let absolute = "/var/loco/app.db";
        assert_eq!(
            resolve_sqlite_path(None, Some(absolute)),
            Path::new(absolute)
        );
        assert_eq!(
            resolve_sqlite_path(Some(Path::new("/tmp/x")), Some(absolute)),
            Path::new(absolute)
        );
    }

    #[test]
    fn blank_loco_root_is_unset() {
        assert!(parse_loco_root(None).is_none());
        assert!(parse_loco_root(Some("")).is_none());
        assert!(parse_loco_root(Some("   ")).is_none());
        assert_eq!(
            parse_loco_root(Some(" /tmp/x ")).as_deref(),
            Some(Path::new("/tmp/x"))
        );
    }

    #[test]
    fn absolute_path_logs_a_relative_database_under_the_working_directory() {
        let logged = absolute_path(Path::new("loco.db"));
        assert!(logged.is_absolute());
        assert_eq!(logged.file_name().unwrap(), "loco.db");
        assert_eq!(logged, std::env::current_dir().unwrap().join("loco.db"));
        assert_eq!(
            absolute_path(Path::new("/tmp/x/loco.db")),
            Path::new("/tmp/x/loco.db")
        );
    }

    #[test]
    fn port_defaults_and_parses() {
        assert_eq!(resolve_port(None).unwrap(), 3000);
        assert_eq!(resolve_port(Some("3100")).unwrap(), 3100);
        assert_eq!(resolve_port(Some(" 3100 ")).unwrap(), 3100);
        assert_eq!(resolve_port(Some("0")).unwrap(), 0);
        assert_eq!(resolve_port(Some("65535")).unwrap(), 65535);
    }

    #[test]
    fn port_rejects_blank_and_non_numeric() {
        assert!(resolve_port(Some("")).is_err());
        assert!(resolve_port(Some("   ")).is_err());
        assert!(resolve_port(Some("http")).is_err());
        assert!(resolve_port(Some("65536")).is_err());
        assert!(resolve_port(Some("-1")).is_err());
    }
}
