//! Declared actions: a handler registry and the run that sits in front of it.
//!
//! A handler is registered in code against the project that owns the
//! declaration and the action's bare name. A same-named declaration in
//! another project does not bind it. The production binary registers nothing
//! (`AppOptions::default`); the Hurl fixture handlers live in the test runner.
//!
//! Type actions are a different registry, keyed `(project, type, name)`.
//! See [`crate::integrations::TypeActionRegistry`]. It is not dispatched.
//! This one stays `(project, name)`.
//!
//! Handlers are async. Outbound HTTP uses [`reqwest`]'s async client, so the
//! call awaits on the request task. [`DataAdapter`] stays synchronous and
//! runs on that same task: a call finishes without awaiting, so no lock is
//! held across an `.await`, the same as a `/data` handler. A blocking client
//! is not used. One call can take the whole 30s timeout, and that would hold
//! a blocking-pool thread for that long.
//!
//! The lake has no transactions. A handler that fails after it has written
//! leaves those writes in place. Handlers must be safe to re-run, and a
//! handler error should say what was already written. [`ActionFailure`] is
//! how that error becomes 400, 409, 502, 503, or 500.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use loco_lake::{DataAdapter, Value};
use serde_json::Map;

use crate::auth::AuthUser;
use crate::http::version_schema::VersionSchema;
use crate::validation::{kind, validate_action_input, Diagnostic, ValidationReport};
use crate::values::{get_variable, list_variables, SecretError, SecretStore};

/// TCP connect budget for one outbound call.
pub const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Budget for the whole call, including the body.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_MAX_REDIRECTS: usize = 10;

/// Why a handler refused to finish. The HTTP layer maps these to 400, 409,
/// 502, 503, and 500. `diagnostics` uses the same kinds as `/data` when the
/// handler has them; an empty list is omitted from the body.
#[derive(Debug)]
pub enum ActionFailure {
    BadInput {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
    Conflict {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
    /// The handler's upstream HTTP call failed. `status` is the upstream
    /// status, or `0` when the call never got a response. The HTTP layer
    /// answers 502 `upstream {status}: {message}`, and that string is the
    /// caller's response as written. A handler must not put secret material
    /// in `message`, or an upstream body that echoes the request (the URL,
    /// a query string, a token). [`From<reqwest::Error>`] drops the URL
    /// reqwest would otherwise append. This is not [`Self::Failed`]: an
    /// upstream failure is never a 500.
    Upstream { status: u16, message: String },
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it. The
    /// HTTP layer answers 503, the same as `PUT /config/secret`.
    Unavailable { message: String },
    Failed {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
}

impl ActionFailure {
    pub fn message(&self) -> &str {
        match self {
            Self::BadInput { message, .. }
            | Self::Conflict { message, .. }
            | Self::Upstream { message, .. }
            | Self::Unavailable { message }
            | Self::Failed { message, .. } => message,
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::BadInput { diagnostics, .. }
            | Self::Conflict { diagnostics, .. }
            | Self::Failed { diagnostics, .. } => diagnostics,
            Self::Upstream { .. } | Self::Unavailable { .. } => &[],
        }
    }
}

/// A secret or variable read failed, or the package does not declare `name`.
///
/// [`ActionFailure`] implements [`From<ConfigReadError>`], so a handler can
/// propagate with `?`. [`ConfigReadError::Undeclared`] becomes 500: asking
/// for a name the package did not declare is a bug in the handler.
#[derive(Debug)]
pub enum ConfigReadError {
    /// `owner` does not declare `name` on the version this request sees.
    Undeclared {
        kind: &'static str,
        name: String,
    },
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it.
    Unavailable(String),
    Failed(String),
}

impl std::fmt::Display for ConfigReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Undeclared { kind, name } => {
                write!(f, "{kind} '{name}' is not declared by this package")
            }
            Self::Unavailable(message) | Self::Failed(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ConfigReadError {}

impl From<reqwest::Error> for ActionFailure {
    fn from(err: reqwest::Error) -> Self {
        let status = err.status().map(|status| status.as_u16()).unwrap_or(0);
        // `Display` appends ` for url ({url})`, query string included.
        Self::Upstream {
            status,
            message: err.without_url().to_string(),
        }
    }
}

impl From<ConfigReadError> for ActionFailure {
    fn from(err: ConfigReadError) -> Self {
        match err {
            ConfigReadError::Unavailable(message) => Self::Unavailable { message },
            ConfigReadError::Undeclared { .. } | ConfigReadError::Failed(_) => Self::Failed {
                message: err.to_string(),
                diagnostics: Vec::new(),
            },
        }
    }
}

/// What a handler is allowed to see.
///
/// `secret` and `variable` read the owning project's declarations only (the
/// registry key), and the values are always [`Self::dataset_id`] — the
/// installing project's dataset. The package's own datasets are never opened.
pub struct ActionContext {
    pub dataset_id: String,
    pub data: Arc<dyn DataAdapter>,
    pub schema: VersionSchema,
    pub caller: AuthUser,
    pub input: HashMap<String, Value>,
    secrets: Arc<dyn SecretStore>,
    /// Owning project of the action being run. The registry key.
    owner: String,
    http: reqwest::Client,
}

impl ActionContext {
    /// Build the context a handler receives. `owner` is the registry project:
    /// the action's project for an ordinary action, the type's project for a
    /// type action.
    pub(crate) fn new(
        dataset_id: String,
        deps: HandlerDeps,
        schema: VersionSchema,
        caller: AuthUser,
        input: HashMap<String, Value>,
        owner: String,
    ) -> Self {
        Self {
            dataset_id,
            data: deps.data,
            schema,
            caller,
            input,
            secrets: deps.secrets,
            owner,
            http: deps.http,
        }
    }

    /// The installing dataset's plaintext for the secret `name` this package
    /// declares.
    ///
    /// `name` is the package's bare declaration name. It is not resolved
    /// through [`VersionSchema::split`], so a secret the installer declares
    /// under the same name is a different row and is not returned. `Ok(None)`
    /// means the package declares it and this dataset has not set it. The
    /// plaintext is returned as stored, including `""`.
    pub fn secret(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        let Some(decl) = self.schema.secret_of(&self.owner, name) else {
            return Err(ConfigReadError::Undeclared {
                kind: "secret",
                name: name.to_string(),
            });
        };
        let id = self.schema.reference(&self.owner, decl.name());
        match self.secrets.get(&self.dataset_id, &id) {
            Ok(value) => Ok(value),
            Err(SecretError::NotFound) => Ok(None),
            Err(SecretError::Unavailable(message)) => Err(ConfigReadError::Unavailable(message)),
            Err(err) => Err(ConfigReadError::Failed(err.to_string())),
        }
    }

    /// The installing dataset's value for the variable `name` this package
    /// declares, or the declaration's non-empty default when nothing is stored.
    ///
    /// A stored value wins, including `""`. `Ok(None)` means the package
    /// declares it, nothing is stored, and the default is empty. The same
    /// owner rule as [`Self::secret`] applies.
    pub fn variable(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        let Some(decl) = self.schema.variable_of(&self.owner, name) else {
            return Err(ConfigReadError::Undeclared {
                kind: "variable",
                name: name.to_string(),
            });
        };
        let id = self.schema.reference(&self.owner, decl.name());
        match get_variable(self.data.as_ref(), &self.dataset_id, &id) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => Ok(non_empty(decl.default())),
            Err(err) => Err(ConfigReadError::Failed(err.to_string())),
        }
    }

    /// Shared client. Connect and total timeouts are set. No proxy. A
    /// redirect is followed only when the next URL keeps the same scheme,
    /// host, and port.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

type ActionFuture = Pin<Box<dyn Future<Output = Result<serde_json::Value, ActionFailure>> + Send>>;

trait ActionHandler: Send + Sync {
    fn call(&self, ctx: ActionContext) -> ActionFuture;
}

impl<F, Fut> ActionHandler for F
where
    F: Fn(ActionContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
{
    fn call(&self, ctx: ActionContext) -> ActionFuture {
        Box::pin(self(ctx))
    }
}

/// Handlers keyed by `(owning project, bare action name)`.
#[derive(Clone, Default)]
pub struct HandlerRegistry {
    handlers: HashMap<(String, String), Arc<dyn ActionHandler>>,
}

impl HandlerRegistry {
    /// Register `handler` for `project`'s bare action `name`.
    ///
    /// The closure returns a future and runs on the request's task. It can
    /// `.await` [`ActionContext::http`]. A same-named action on another
    /// project does not call it.
    pub fn register<F, Fut>(
        &mut self,
        project: impl Into<String>,
        name: impl Into<String>,
        handler: F,
    ) where
        F: Fn(ActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
    {
        let handler: Arc<dyn ActionHandler> = Arc::new(handler);
        self.handlers.insert((project.into(), name.into()), handler);
    }

    fn get(&self, project: &str, name: &str) -> Option<Arc<dyn ActionHandler>> {
        self.handlers
            .get(&(project.to_string(), name.to_string()))
            .cloned()
    }
}

/// Lake, secret store, and HTTP client one run needs. The `Arc`s and the
/// client are cloned onto the handler so it can hold them across `.await`.
pub struct HandlerDeps {
    pub data: Arc<dyn DataAdapter>,
    pub secrets: Arc<dyn SecretStore>,
    pub http: reqwest::Client,
}

/// The result of resolving, validating, and running one action. Auth stays in
/// the HTTP layer: this runs only after the caller is allowed to write.
#[derive(Debug)]
pub enum Dispatch {
    NotFound,
    Invalid(ValidationReport),
    NoHandler {
        project: String,
        name: String,
    },
    /// A required secret or variable of the owning project is unset on this
    /// dataset. The handler was not called.
    MissingConfig(Vec<Diagnostic>),
    Done(serde_json::Value),
    Failed(ActionFailure),
}

/// Resolve `name` in `schema`, validate `input`, refuse a run whose owning
/// project is missing a required value, and call the handler registered for
/// that project and the action's bare name.
///
/// An invalid input does not call the handler. A missing required value does
/// not either, and it is reported only once a handler exists: an action with
/// no handler is still [`Dispatch::NoHandler`].
pub async fn dispatch(
    schema: &VersionSchema,
    registry: &HandlerRegistry,
    dataset_id: &str,
    deps: HandlerDeps,
    caller: &AuthUser,
    name: &str,
    input: &Map<String, serde_json::Value>,
) -> Dispatch {
    let Some(action) = schema.action(name) else {
        return Dispatch::NotFound;
    };
    let input = match validate_action_input(schema, &action, input) {
        Ok(input) => input,
        Err(report) => return Dispatch::Invalid(report),
    };
    let owner = action.project().to_string();
    let bare = action.name().to_string();
    let Some(handler) = registry.get(&owner, &bare) else {
        return Dispatch::NoHandler {
            project: owner,
            name: bare,
        };
    };
    let missing = match missing_configuration(schema, &owner, dataset_id, &deps) {
        Ok(missing) => missing,
        Err(failure) => return Dispatch::Failed(failure),
    };
    if !missing.is_empty() {
        return Dispatch::MissingConfig(missing);
    }
    let ctx = ActionContext::new(
        dataset_id.to_string(),
        deps,
        schema.clone(),
        caller.clone(),
        input,
        owner,
    );
    match handler.call(ctx).await {
        Ok(value) => Dispatch::Done(value),
        Err(failure) => Dispatch::Failed(failure),
    }
}

/// Required secrets and variables `owner` declares on the version `schema`
/// sees, that `dataset_id` has not set.
///
/// A secret counts as set when its row exists. The plaintext is not read, so
/// this cannot tell an empty secret from any other stored secret. A variable
/// counts as set when its row exists, or when the declaration's default is
/// non-empty. Diagnostics are secrets by name, then variables by name.
fn missing_configuration(
    schema: &VersionSchema,
    owner: &str,
    dataset_id: &str,
    deps: &HandlerDeps,
) -> Result<Vec<Diagnostic>, ActionFailure> {
    let mut missing = Vec::new();

    let mut secrets = schema.secrets_of(owner);
    secrets.retain(|secret| secret.required());
    secrets.sort_by(|a, b| a.name().cmp(b.name()));
    if !secrets.is_empty() {
        let stored = deps.secrets.list(dataset_id).map_err(secret_failure)?;
        for secret in secrets {
            let id = schema.reference(owner, secret.name());
            if stored.iter().any(|row| row.name == id) {
                continue;
            }
            missing.push(missing_diag("secret", secret.name()));
        }
    }

    let mut variables = schema.variables_of(owner);
    variables.retain(|variable| variable.required());
    variables.sort_by(|a, b| a.name().cmp(b.name()));
    if !variables.is_empty() {
        let stored = list_variables(deps.data.as_ref(), dataset_id).map_err(lake_failure)?;
        for variable in variables {
            let id = schema.reference(owner, variable.name());
            if stored.iter().any(|row| row.name == id) || !variable.default().is_empty() {
                continue;
            }
            missing.push(missing_diag("variable", variable.name()));
        }
    }

    Ok(missing)
}

fn missing_diag(noun: &str, name: &str) -> Diagnostic {
    Diagnostic::error(
        kind::REQUIRED,
        Some(name.to_string()),
        format!("{noun} '{name}' is not set"),
    )
}

fn secret_failure(err: SecretError) -> ActionFailure {
    match err {
        SecretError::Unavailable(message) => ActionFailure::Unavailable { message },
        other => ActionFailure::Failed {
            message: other.to_string(),
            diagnostics: Vec::new(),
        },
    }
}

fn lake_failure(err: loco_lake::Error) -> ActionFailure {
    ActionFailure::Failed {
        message: err.to_string(),
        diagnostics: Vec::new(),
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// The client handlers share. Built once at boot.
///
/// No proxy: not `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`, and not the OS
/// proxy settings. The call goes to the URL the handler built. A redirect is
/// followed only when the next URL keeps the same scheme, host, and port.
/// Ten such redirects are followed; the eleventh fails the call.
/// `previous()` already includes the original URL, so the limit is `len > 10`,
/// the same comparison reqwest's own `limited` uses.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(HTTP_TIMEOUT)
        .redirect(redirect_policy())
        .no_proxy()
        .build()
        .expect("build the action HTTP client")
}

fn same_origin(prev: &reqwest::Url, next: &reqwest::Url) -> bool {
    let hosts = match (prev.host_str(), next.host_str()) {
        (Some(prev), Some(next)) => prev.eq_ignore_ascii_case(next),
        _ => false,
    };
    prev.scheme() == next.scheme()
        && hosts
        && prev.port_or_known_default() == next.port_or_known_default()
}

fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > HTTP_MAX_REDIRECTS {
            return attempt.error(RedirectLimited);
        }
        let Some(prev) = attempt.previous().last() else {
            return attempt.follow();
        };
        if same_origin(prev, attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

#[derive(Debug)]
struct RedirectLimited;

impl std::fmt::Display for RedirectLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "too many redirects")
    }
}

impl std::error::Error for RedirectLimited {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use loco_lake::InMemoryAdapter;
    use serde_json::json;

    use super::*;
    use crate::validation::kind;
    use crate::values::{put_variable, KeyStatus, LakeSecretStore, SecretMeta};
    use crate::{Action, ActionParam, ActionParamOption, Manifest, SchemaStore, Secret, Variable};

    const PROJECT: &str = "ben/crm";
    const VERSION: &str = "0.0.1-dev";
    const PKG: &str = "alice/pkg";
    const STORE: &str = "alice/store";
    const STORE_DATASET: &str = "alice/store/dev";
    const PKG_DATASET: &str = "alice/pkg/dev";

    fn schema() -> (tempfile::TempDir, VersionSchema) {
        let dir = tempfile::tempdir().unwrap();
        let store = SchemaStore::load(dir.path()).unwrap();
        store
            .manifests()
            .create(Manifest::new(
                PROJECT.into(),
                VERSION.into(),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        store
            .actions()
            .create(Action::new(
                PROJECT.into(),
                VERSION.into(),
                "echo".into(),
                "Echo".into(),
                String::new(),
                vec![
                    ActionParam {
                        name: "qty".into(),
                        r#type: "integer".into(),
                        required: true,
                        ..ActionParam::default()
                    },
                    ActionParam {
                        name: "size".into(),
                        r#type: "string".into(),
                        required: true,
                        options: vec![
                            ActionParamOption {
                                value: "s".into(),
                                label: "Small".into(),
                            },
                            ActionParamOption {
                                value: "m".into(),
                                label: "Medium".into(),
                            },
                        ],
                        ..ActionParam::default()
                    },
                ],
            ))
            .unwrap();
        store
            .actions()
            .create(Action::new(
                PROJECT.into(),
                VERSION.into(),
                "noop".into(),
                "Noop".into(),
                String::new(),
                Vec::new(),
            ))
            .unwrap();
        let schema = VersionSchema::new_read_only(Arc::new(store), PROJECT, VERSION);
        (dir, schema)
    }

    fn caller() -> AuthUser {
        AuthUser {
            id: "1".into(),
            username: "alice".into(),
            name: "Alice".into(),
            account_type: "person".into(),
            created_at: String::new(),
            last_login_at: None,
        }
    }

    fn test_http() -> reqwest::Client {
        static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
        CLIENT.get_or_init(http_client).clone()
    }

    fn deps(data: Arc<dyn DataAdapter>, secrets: Arc<dyn SecretStore>) -> HandlerDeps {
        HandlerDeps {
            data,
            secrets,
            http: test_http(),
        }
    }

    struct Ran {
        echo: Arc<AtomicBool>,
        decoy: Arc<AtomicBool>,
        registry: HandlerRegistry,
    }

    fn registry() -> Ran {
        let echo = Arc::new(AtomicBool::new(false));
        let decoy = Arc::new(AtomicBool::new(false));
        let mut registry = HandlerRegistry::default();
        let echo_flag = Arc::clone(&echo);
        registry.register("ben/crm", "echo", move |_| {
            let echo_flag = Arc::clone(&echo_flag);
            async move {
                echo_flag.store(true, Ordering::SeqCst);
                Ok(json!({ "ran": true }))
            }
        });
        let decoy_flag = Arc::clone(&decoy);
        registry.register("alice/other", "echo", move |_| {
            let decoy_flag = Arc::clone(&decoy_flag);
            async move {
                decoy_flag.store(true, Ordering::SeqCst);
                Ok(json!({ "ran": true }))
            }
        });
        Ran {
            echo,
            decoy,
            registry,
        }
    }

    async fn run(
        schema: &VersionSchema,
        ran: &Ran,
        name: &str,
        input: serde_json::Value,
    ) -> Dispatch {
        ran.echo.store(false, Ordering::SeqCst);
        ran.decoy.store(false, Ordering::SeqCst);
        let data: Arc<dyn DataAdapter> = Arc::new(InMemoryAdapter::new());
        let secrets: Arc<dyn SecretStore> =
            Arc::new(LakeSecretStore::new(data.clone(), KeyStatus::Missing));
        let input = input.as_object().cloned().unwrap_or_default();
        dispatch(
            schema,
            &ran.registry,
            "ben/crm/dev",
            deps(data, secrets),
            &caller(),
            name,
            &input,
        )
        .await
    }

    fn kinds(outcome: &Dispatch) -> Vec<&str> {
        let Dispatch::Invalid(report) = outcome else {
            panic!("expected a validation error");
        };
        report.diagnostics.iter().map(|d| d.kind.as_str()).collect()
    }

    #[tokio::test]
    async fn invalid_input_does_not_call_the_handler() {
        let (_dir, schema) = schema();
        let ran = registry();

        let missing = run(&schema, &ran, "echo", json!({"qty": 1})).await;
        assert_eq!(kinds(&missing), vec![kind::REQUIRED]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let wrong = run(&schema, &ran, "echo", json!({"qty": "two", "size": "m"})).await;
        assert_eq!(kinds(&wrong), vec![kind::TYPE_MISMATCH]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let option = run(&schema, &ran, "echo", json!({"qty": 1, "size": "xl"})).await;
        assert_eq!(kinds(&option), vec![kind::INVALID_OPTION]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let unknown = run(
            &schema,
            &ran,
            "echo",
            json!({"qty": 1, "size": "m", "extra": true}),
        )
        .await;
        assert_eq!(kinds(&unknown), vec![kind::UNKNOWN_FIELD]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let nested = run(&schema, &ran, "echo", json!({"qty": {"n": 1}, "size": "m"})).await;
        assert_eq!(kinds(&nested), vec![kind::TYPE_MISMATCH]);
        assert!(!ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn lookup_is_owning_project_and_bare_name() {
        let (_dir, schema) = schema();
        let ran = registry();

        let outcome = run(&schema, &ran, "echo", json!({"qty": 2, "size": "m"})).await;
        assert!(matches!(outcome, Dispatch::Done(_)));
        assert!(ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));

        let outcome = run(&schema, &ran, "noop", json!({})).await;
        match outcome {
            Dispatch::NoHandler { project, name } => {
                assert_eq!(project, "ben/crm");
                assert_eq!(name, "noop");
            }
            other => panic!("expected no handler, got a different outcome: {other:?}"),
        }
        assert!(!ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));

        let outcome = run(&schema, &ran, "alice/other.echo", json!({})).await;
        assert!(matches!(outcome, Dispatch::NotFound));
        assert!(!ran.decoy.load(Ordering::SeqCst));
    }

    /// `list` reports the name and nothing else. `get` panics, so a required
    /// check that decrypted would fail the test.
    struct ListOnly {
        names: Vec<String>,
    }

    impl SecretStore for ListOnly {
        fn put(&self, _: &str, _: &str, _: &str) -> Result<SecretMeta, SecretError> {
            panic!("not used");
        }

        fn get(&self, _: &str, _: &str) -> Result<Option<String>, SecretError> {
            panic!("required check must not read secret plaintext");
        }

        fn delete(&self, _: &str, _: &str) -> Result<(), SecretError> {
            panic!("not used");
        }

        fn list(&self, _: &str) -> Result<Vec<SecretMeta>, SecretError> {
            Ok(self
                .names
                .iter()
                .map(|name| SecretMeta {
                    name: name.clone(),
                    updated_at: "t".into(),
                })
                .collect())
        }

        fn delete_dataset(&self, _: &str) -> Result<(), SecretError> {
            panic!("not used");
        }
    }

    struct World {
        _dir: tempfile::TempDir,
        store: Arc<SchemaStore>,
        data: Arc<dyn DataAdapter>,
        secrets: Arc<dyn SecretStore>,
    }

    impl World {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(SchemaStore::load(dir.path()).unwrap());
            store
                .manifests()
                .create(Manifest::new(
                    PKG.into(),
                    VERSION.into(),
                    Vec::new(),
                    Vec::new(),
                ))
                .unwrap();
            store
                .secrets()
                .create(Secret::new(
                    PKG.into(),
                    VERSION.into(),
                    "token".into(),
                    "Token".into(),
                    String::new(),
                    true,
                ))
                .unwrap();
            store
                .variables()
                .create(Variable::new(
                    PKG.into(),
                    VERSION.into(),
                    "upstream".into(),
                    "Upstream".into(),
                    String::new(),
                    true,
                    "http://127.0.0.1:9".into(),
                ))
                .unwrap();
            store
                .variables()
                .create(Variable::new(
                    PKG.into(),
                    VERSION.into(),
                    "label".into(),
                    "Label".into(),
                    String::new(),
                    false,
                    "from-the-package-default".into(),
                ))
                .unwrap();
            store
                .variables()
                .create(Variable::new(
                    PKG.into(),
                    VERSION.into(),
                    "region".into(),
                    "Region".into(),
                    String::new(),
                    true,
                    String::new(),
                ))
                .unwrap();
            store
                .actions()
                .create(Action::new(
                    PKG.into(),
                    VERSION.into(),
                    "pull".into(),
                    "Pull".into(),
                    String::new(),
                    Vec::new(),
                ))
                .unwrap();
            store
                .manifests()
                .create(Manifest::new(
                    STORE.into(),
                    VERSION.into(),
                    vec![format!("{PKG}@{VERSION}")],
                    Vec::new(),
                ))
                .unwrap();
            store
                .secrets()
                .create(Secret::new(
                    STORE.into(),
                    VERSION.into(),
                    "token".into(),
                    "Own token".into(),
                    String::new(),
                    true,
                ))
                .unwrap();
            store
                .secrets()
                .create(Secret::new(
                    STORE.into(),
                    VERSION.into(),
                    "store_only".into(),
                    "Store only".into(),
                    String::new(),
                    false,
                ))
                .unwrap();
            let data: Arc<dyn DataAdapter> = Arc::new(InMemoryAdapter::new());
            let secrets: Arc<dyn SecretStore> = Arc::new(LakeSecretStore::new(
                data.clone(),
                KeyStatus::Ready([7u8; 32]),
            ));
            Self {
                _dir: dir,
                store,
                data,
                secrets,
            }
        }

        fn store_schema(&self) -> VersionSchema {
            VersionSchema::new_read_only(self.store.clone(), STORE, VERSION)
        }

        fn package_schema(&self) -> VersionSchema {
            VersionSchema::new_read_only(self.store.clone(), PKG, VERSION)
        }
    }

    fn messages(outcome: &Dispatch) -> Vec<&str> {
        let Dispatch::MissingConfig(diagnostics) = outcome else {
            panic!("expected missing configuration, got {outcome:?}");
        };
        diagnostics.iter().map(|d| d.message.as_str()).collect()
    }

    async fn dispatch_pull(
        schema: &VersionSchema,
        world: &World,
        secrets: Arc<dyn SecretStore>,
        dataset_id: &str,
        registry: &HandlerRegistry,
    ) -> Dispatch {
        dispatch(
            schema,
            registry,
            dataset_id,
            deps(world.data.clone(), secrets),
            &caller(),
            if schema.project_id() == STORE {
                "alice/pkg.pull"
            } else {
                "pull"
            },
            &Map::new(),
        )
        .await
    }

    #[tokio::test]
    async fn missing_required_does_not_run_or_read_plaintext() {
        let world = World::new();
        let schema = world.store_schema();
        let ran = Arc::new(AtomicBool::new(false));
        let mut registry = HandlerRegistry::default();
        let flag = Arc::clone(&ran);
        registry.register(PKG, "pull", move |_| {
            let flag = Arc::clone(&flag);
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(json!({ "ran": true }))
            }
        });

        let absent: Arc<dyn SecretStore> = Arc::new(ListOnly { names: Vec::new() });
        let outcome = dispatch_pull(&schema, &world, absent, STORE_DATASET, &registry).await;
        assert_eq!(
            messages(&outcome),
            vec!["secret 'token' is not set", "variable 'region' is not set",]
        );
        assert!(!ran.load(Ordering::SeqCst));
        let Dispatch::MissingConfig(diagnostics) = &outcome else {
            unreachable!();
        };
        assert!(diagnostics.iter().all(|d| d.kind == kind::REQUIRED));

        // The row's presence is enough. The plaintext is not read, so an
        // empty secret is set. `upstream`'s default covers that variable;
        // `label` is not required. `region` still is.
        let present: Arc<dyn SecretStore> = Arc::new(ListOnly {
            names: vec!["alice/pkg.token".into()],
        });
        let outcome = dispatch_pull(&schema, &world, present, STORE_DATASET, &registry).await;
        assert_eq!(messages(&outcome), vec!["variable 'region' is not set"]);
        assert!(!ran.load(Ordering::SeqCst));

        put_variable(world.data.as_ref(), STORE_DATASET, "alice/pkg.region", "").unwrap();
        let present: Arc<dyn SecretStore> = Arc::new(ListOnly {
            names: vec!["alice/pkg.token".into()],
        });
        let outcome = dispatch_pull(&schema, &world, present, STORE_DATASET, &registry).await;
        assert!(matches!(outcome, Dispatch::Done(_)), "{outcome:?}");
        assert!(ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn handler_reads_the_installers_values_for_its_own_package() {
        let world = World::new();
        let mut registry = HandlerRegistry::default();
        registry.register(PKG, "pull", |ctx| async move {
            let token = ctx.secret("token")?;
            let store_only = match ctx.secret("store_only") {
                Ok(value) => json!(value),
                Err(err) => json!({ "error": err.to_string() }),
            };
            Ok(json!({
                "ran": true,
                "dataset_id": ctx.dataset_id,
                "token": token,
                "store_only": store_only,
                "upstream": ctx.variable("upstream")?,
                "label": ctx.variable("label")?,
                "region": ctx.variable("region")?,
            }))
        });

        world
            .secrets
            .put(PKG_DATASET, "token", "from-the-package-dataset")
            .unwrap();
        world
            .secrets
            .put(STORE_DATASET, "token", "store-own-token")
            .unwrap();
        world
            .secrets
            .put(STORE_DATASET, "store_only", "store-only-token")
            .unwrap();
        // Empty plaintext still counts as set, and the handler receives it.
        world
            .secrets
            .put(STORE_DATASET, "alice/pkg.token", "")
            .unwrap();
        put_variable(world.data.as_ref(), STORE_DATASET, "alice/pkg.region", "eu").unwrap();

        let store_schema = world.store_schema();
        let outcome = dispatch_pull(
            &store_schema,
            &world,
            world.secrets.clone(),
            STORE_DATASET,
            &registry,
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the handler to run, got {outcome:?}");
        };
        assert_eq!(body["token"], "");
        assert_eq!(body["dataset_id"], STORE_DATASET);

        world
            .secrets
            .put(STORE_DATASET, "alice/pkg.token", "installer-token")
            .unwrap();
        put_variable(
            world.data.as_ref(),
            STORE_DATASET,
            "alice/pkg.upstream",
            "http://127.0.0.1:9/replaced",
        )
        .unwrap();
        let outcome = dispatch_pull(
            &store_schema,
            &world,
            world.secrets.clone(),
            STORE_DATASET,
            &registry,
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the handler to run, got {outcome:?}");
        };
        assert_eq!(body["ran"], true);
        assert_eq!(body["dataset_id"], STORE_DATASET);
        assert_eq!(body["token"], "installer-token");
        assert_eq!(body["region"], "eu");
        assert_eq!(body["label"], "from-the-package-default");
        assert_eq!(body["upstream"], "http://127.0.0.1:9/replaced");
        assert_eq!(
            body["store_only"]["error"],
            "secret 'store_only' is not declared by this package"
        );
        let text = body.to_string();
        assert!(!text.contains("store-own-token"), "{text}");
        assert!(!text.contains("store-only-token"), "{text}");
        assert!(!text.contains("from-the-package-dataset"), "{text}");

        // The package's own site reads the package dataset. The installer's
        // row is a different dataset and is not visible here.
        put_variable(world.data.as_ref(), PKG_DATASET, "region", "pkg-region").unwrap();
        let package_schema = world.package_schema();
        let outcome = dispatch_pull(
            &package_schema,
            &world,
            world.secrets.clone(),
            PKG_DATASET,
            &registry,
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the package site to run, got {outcome:?}");
        };
        assert_eq!(body["dataset_id"], PKG_DATASET);
        assert_eq!(body["token"], "from-the-package-dataset");
        assert_eq!(body["region"], "pkg-region");
        assert_eq!(body["upstream"], "http://127.0.0.1:9");
        assert_eq!(body["label"], "from-the-package-default");
        let text = body.to_string();
        assert!(!text.contains("installer-token"), "{text}");
        assert!(!text.contains("store-own-token"), "{text}");
    }

    #[tokio::test]
    async fn missing_secret_key_over_a_stored_row_is_unavailable() {
        let world = World::new();
        world
            .secrets
            .put(STORE_DATASET, "alice/pkg.token", "installer-token")
            .unwrap();
        put_variable(world.data.as_ref(), STORE_DATASET, "alice/pkg.region", "eu").unwrap();
        let missing: Arc<dyn SecretStore> =
            Arc::new(LakeSecretStore::new(world.data.clone(), KeyStatus::Missing));
        let mut registry = HandlerRegistry::default();
        registry.register(PKG, "pull", |ctx| async move {
            let _token = ctx.secret("token")?;
            Ok(json!({ "ran": true }))
        });
        let schema = world.store_schema();
        let outcome = dispatch_pull(&schema, &world, missing, STORE_DATASET, &registry).await;
        match outcome {
            Dispatch::Failed(ActionFailure::Unavailable { message }) => {
                assert!(message.contains("LOCO_SECRET_KEY"), "{message}");
            }
            other => panic!("expected an unavailable secret key, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reqwest_error_drops_the_request_url() {
        let err = http_client()
            .get("http://127.0.0.1:9/?k=1")
            .send()
            .await
            .expect_err("port 9 accepts nothing");
        let ActionFailure::Upstream { status, message } = ActionFailure::from(err) else {
            panic!("expected an upstream failure");
        };
        assert_eq!(status, 0);
        assert!(!message.contains("127.0.0.1"), "{message}");
        assert!(!message.contains("k=1"), "{message}");
    }

    #[tokio::test]
    async fn redirect_stays_on_the_same_host() {
        let client = test_http();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_server = Arc::clone(&hits);
        // `localhost` and `127.0.0.1` are different hosts to the policy even
        // though both reach this machine. Following the redirect would hit
        // `other` and increment the counter.
        let other = spawn_server(move |_port, path| {
            hits_for_server.fetch_add(1, Ordering::SeqCst);
            response(200, "OK", &[], &format!("other-host {path}"))
        });
        let same = spawn_server(|port, path| {
            if path == "/start" {
                response(
                    302,
                    "Found",
                    &[("Location", &format!("http://127.0.0.1:{port}/done"))],
                    "go-away",
                )
            } else {
                response(200, "OK", &[], "done")
            }
        });
        let followed = client
            .get(format!("http://127.0.0.1:{same}/start"))
            .send()
            .await
            .unwrap();
        assert_eq!(followed.status(), 200);
        assert_eq!(followed.text().await.unwrap(), "done");

        let away = spawn_server(move |_port, path| {
            if path == "/away" {
                response(
                    302,
                    "Found",
                    &[("Location", &format!("http://localhost:{other}/landed"))],
                    "go-away",
                )
            } else {
                response(404, "Not Found", &[], "no")
            }
        });
        let stopped = client
            .get(format!("http://127.0.0.1:{away}/away"))
            .send()
            .await
            .unwrap();
        assert_eq!(stopped.status(), 302);
        assert_eq!(stopped.text().await.unwrap(), "go-away");
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        // Same host, different port. Following would hit `other`.
        let cross_port = spawn_server(move |_port, path| {
            if path == "/port" {
                response(
                    302,
                    "Found",
                    &[("Location", &format!("http://127.0.0.1:{other}/landed"))],
                    "go-away",
                )
            } else {
                response(404, "Not Found", &[], "no")
            }
        });
        let stopped_port = client
            .get(format!("http://127.0.0.1:{cross_port}/port"))
            .send()
            .await
            .unwrap();
        assert_eq!(stopped_port.status(), 302);
        assert_eq!(stopped_port.text().await.unwrap(), "go-away");
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    fn response(status: u16, reason: &str, headers: &[(&str, &str)], body: &str) -> String {
        let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
        for (name, value) in headers {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
            out.push_str("\r\n");
        }
        out.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        out
    }

    fn spawn_server(handler: impl Fn(u16, &str) -> String + Send + Sync + 'static) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = answer(stream, &|path| handler(port, path));
            }
        });
        port
    }

    fn answer(
        mut stream: std::net::TcpStream,
        handler: &impl Fn(&str) -> String,
    ) -> std::io::Result<()> {
        use std::io::{Read, Write};
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 8192 {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
                Err(err) => return Err(err),
            }
        }
        let req = String::from_utf8_lossy(&buf);
        let path = req.split_whitespace().nth(1).unwrap_or("/");
        stream.write_all(handler(path).as_bytes())?;
        stream.flush()
    }
}
