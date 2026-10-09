use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use loco_lake::{DataAdapter, LakeConfig};

use crate::actions::ActionRegistry;
use crate::auth::AuthAdapter;
use crate::config::Config;
use crate::handlers;
use crate::http::host;
use crate::integrations::SourceRegistry;
use crate::seed;
use crate::source::LakeSource;
use crate::values::{KeyStatus, LakeSecretStore, LakeVariableStore, SecretStore, VariableStore};
use crate::{Bundle, Project, SchemaStore, Site};

pub struct AppState {
    /// Lake collections as a [`crate::source::CollectionSource`]. `/data` and
    /// `/data/query` call this for ordinary collections, and the same trait
    /// for an integration collection's registered source.
    /// [`LakeSource::purge_dataset`] removes a dataset's rows after the
    /// secret and variable stores. [`LakeSource::adapter`] is the raw adapter
    /// action handlers take so they can patch records, pending #120. Nothing
    /// else in the crate calls it. Variable reads use [`Self::variables`].
    pub lake: Arc<LakeSource>,
    pub auth_adapter: Box<dyn AuthAdapter>,
    pub schema: Arc<SchemaStore>,
    /// Plaintext trait. The lake impl encrypts. Handlers clone this `Arc`.
    /// See `crate::values`.
    pub secrets: Arc<dyn SecretStore>,
    /// Plaintext. The lake impl stores `$variables`. Handlers clone this `Arc`.
    /// See `crate::values`.
    pub variables: Arc<dyn VariableStore>,
    /// This server's client, built once in [`build_app`]. No proxy. A redirect
    /// that changes scheme, host, or port is not followed. See
    /// [`crate::actions::http_client`].
    pub http: reqwest::Client,
    /// The site the apex serves at `/`, as `({account}/{project}, {site})`.
    /// `None` is the API-only process. A host that names a site of its own
    /// always wins over this.
    pub default_site: Option<(String, String)>,
    /// The registries [`build_app`] was given. Call sites read
    /// [`Extensions::actions`] and [`Extensions::sources`] here.
    pub extensions: Extensions,
}

/// Handler and source registries. Not configuration: [`Default`] is what the
/// server binary ships, and a test builds its own.
///
/// [`Self::actions`] is empty in [`Default`]. The Hurl fixture handlers are
/// registered by the test runner, so they are not in the server binary. An
/// ordinary handler is [`crate::actions::ActionKey::Package`]. A type action
/// is [`crate::actions::ActionKey::Type`]. Both receive an
/// [`crate::actions::ActionContext`]. An ordinary action's connection is the
/// owning package's loose declarations. A type action's connection is the
/// integration the address named. A resolved action with no handler is 501
/// before the required-value check.
///
/// [`Self::sources`] is [`SourceRegistry::production`] in [`Default`]
/// (BrickLink). A test that registers its own fixture source replaces the
/// whole registry. A registered source handles that type's standard and
/// custom collections. An address with no registration is 501 before the
/// required-value check.
pub struct Extensions {
    pub actions: ActionRegistry,
    pub sources: SourceRegistry,
}

impl Default for Extensions {
    fn default() -> Self {
        Self {
            actions: ActionRegistry::default(),
            sources: SourceRegistry::production(),
        }
    }
}

pub fn build_app(config: &Config, extensions: Extensions) -> Router {
    // Seed committed projects the store lacks, then load the store. Writes
    // go only to `schemas/instances/`; `schemas/seed/` is read, never written.
    let root = &config.root;
    let instances_dir = root.join("schemas/instances");
    let seeded = seed::seed_instances(&root.join("schemas/seed"), &instances_dir)
        .expect("failed to seed schema instances");
    for project in &seeded {
        println!("Seeded {project} from schemas/seed");
    }
    let schema = Arc::new(SchemaStore::load(&instances_dir).expect("failed to load schema"));
    match &config.secret_key {
        KeyStatus::Missing => {
            eprintln!("LOCO_SECRET_KEY is not set; PUT /config/secret will return 503")
        }
        KeyStatus::Invalid(msg) => eprintln!("{msg}; PUT /config/secret will return 503"),
        KeyStatus::Ready(_) => {}
    }

    let adapter: Arc<dyn DataAdapter> =
        Arc::from(config.lake.open().unwrap_or_else(|err| panic!("{err}")));
    match &config.lake {
        LakeConfig::Memory => println!("Using in-memory adapter"),
        LakeConfig::Sqlite { path } => {
            println!("Using SQLite adapter ({})", path.display());
        }
    }
    let lake = Arc::new(LakeSource::new(Arc::clone(&adapter)));
    let secrets: Arc<dyn SecretStore> = Arc::new(LakeSecretStore::new(
        Arc::clone(&adapter),
        config.secret_key.clone(),
    ));
    let variables: Arc<dyn VariableStore> = Arc::new(LakeVariableStore::new(adapter));
    let http = crate::actions::http_client();
    let auth_adapter = config.auth.open();
    warn_projects_without_account(&schema, auth_adapter.as_ref());
    let default_site = resolve_default_site(&schema, config.default_site.as_deref());

    let state = Arc::new(AppState {
        lake,
        auth_adapter,
        schema,
        secrets,
        variables,
        http,
        default_site,
        extensions,
    });

    Router::new()
        // Registered here, not under a nest, so a site bundle cannot shadow
        // them. Every host answers these two GETs, including one that is
        // serving `index.html` at `/`.
        .route(
            crate::discovery::DISCOVERY_PATH,
            axum::routing::get(crate::discovery::document),
        )
        .route(
            crate::discovery::LLMS_PATH,
            axum::routing::get(crate::discovery::llms_txt),
        )
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

/// Read `LOCO_DEFAULT_SITE`, and say once at boot what the apex will do with it.
///
/// Every problem here is a warning, never a panic. A process whose default
/// site has no bundle yet is a process mid-deploy: it serves its API and 404s
/// `/` until something is uploaded. Blank is API-only, with no warning.
fn resolve_default_site(
    schema: &Arc<SchemaStore>,
    default_site: Option<&str>,
) -> Option<(String, String)> {
    let raw = default_site?;
    if raw.trim().is_empty() {
        return None;
    }

    let Some((project_id, site_name)) = host::parse_site_ref(raw) else {
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
