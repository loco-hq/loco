//! Declared actions: one handler registry and the run that sits in front of it.
//!
//! A handler is registered in code. An ordinary action's key is the project
//! that owns the declaration and the action's bare name ([`ActionKey::Package`]).
//! A type action's key adds the integration type ([`ActionKey::Type`]). A
//! same-named declaration on another project, or the other kind of key, does
//! not bind it. The production binary registers nothing
//! (`Extensions::default`); the Hurl fixture handlers live in the test runner.
//!
//! Both kinds receive an [`ActionContext`]. [`ActionContext::secret`] and
//! [`ActionContext::variable`] forward to its [`Connection`]. An ordinary
//! action's connection is the owning package's loose declarations. A type
//! action's connection is the one integration the address named. The
//! undeclared-name messages stay different: a package says it does not
//! declare the name, and an integration names its type.
//!
//! Handlers are async. Outbound HTTP uses [`reqwest`]'s async client, so the
//! call awaits on the request task. [`DataAdapter`] stays synchronous and
//! runs on that same task: a call finishes without awaiting, so no lock is
//! held across an `.await`, the same as a `/data` handler. A blocking client
//! is not used. One call can take the whole 30s timeout, and that would hold
//! a blocking-pool thread for that long. Each server builds its own client
//! in [`crate::server::build_app`] and stores it on `AppState`. [`http_client`]
//! is that constructor.
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
use crate::http::version_schema::{
    ActionAddress, ActionAddressKind, ConnectionDeclarations, VersionSchema,
};
use crate::validation::{
    kind, validate_action_input, validate_type_action_input, Diagnostic, ValidationReport,
};
use crate::values::{with_default, SecretError, SecretStore, VariableMeta, VariableStore};

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

/// A secret or variable read failed, or the call's declarations do not
/// include `name`.
///
/// [`ActionFailure`] implements [`From<ConfigReadError>`], so a handler can
/// propagate with `?`. [`ConfigReadError::Undeclared`] becomes 500: asking
/// for a name those declarations omit is a bug in the handler. An ordinary
/// action says the package does not declare it. A connection names the
/// integration type.
#[derive(Debug)]
pub enum ConfigReadError {
    /// The call's declarations do not include `name`.
    ///
    /// `type_ref` is set when a [`Connection`] reports the miss. It is the
    /// integration type's reference from the view that built the connection.
    /// An ordinary action leaves it unset.
    Undeclared {
        kind: &'static str,
        name: String,
        type_ref: Option<String>,
    },
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it.
    Unavailable(String),
    Failed(String),
}

impl std::fmt::Display for ConfigReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Undeclared {
                kind,
                name,
                type_ref,
            } => match type_ref {
                Some(type_ref) => write!(
                    f,
                    "{kind} '{name}' is not declared by integration type '{type_ref}'"
                ),
                None => write!(f, "{kind} '{name}' is not declared by this package"),
            },
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

/// One declaration this connection is allowed to read, with the lake row id
/// already chosen.
#[derive(Clone)]
struct BoundSecret {
    name: String,
    required: bool,
    row_id: String,
}

/// One variable declaration this connection is allowed to read.
#[derive(Clone)]
struct BoundVariable {
    name: String,
    required: bool,
    default_value: String,
    row_id: String,
}

/// Which declarations a connection is allowed to read.
///
/// One fact, stored once. The undeclared-name message matches on it, and the
/// action runner prepares the same enum.
#[derive(Clone, Debug)]
pub enum ConnectionScope {
    /// The owning package's loose declarations. `owner` is the action's
    /// project, the registry key. Row ids are canonical declaration
    /// references: bare when `owner` is the running project, `{owner}.{name}`
    /// when it is a dependency. An undeclared name says this package does
    /// not declare it.
    Package { owner: String },
    /// One integration. `integration` is the canonical qualified name from
    /// the asking view: bare for that project's own (`sf_east`),
    /// `{account}/{project}.{name}` for a dependency's (`alice/pkg.store`).
    /// `type_ref` is the type's reference from that same view. Row ids are
    /// `{integration}:{name}`. An undeclared name names `type_ref`.
    Integration {
        integration: String,
        type_ref: String,
    },
}

/// Values and the server's HTTP client for one call.
///
/// An integration connection reads that integration's rows on
/// [`Self::dataset_id`] and nothing else. A loose declaration of the same
/// bare name, and another integration's row, are not returned. A package
/// connection reads the owning project's loose declarations. The secret
/// store and the variable store stay private: a caller receives this
/// connection, not every value on the dataset.
#[derive(Clone)]
pub struct Connection {
    pub dataset_id: String,
    pub scope: ConnectionScope,
    secrets: Arc<dyn SecretStore>,
    variables: Arc<dyn VariableStore>,
    secret_decls: Vec<BoundSecret>,
    variable_decls: Vec<BoundVariable>,
    http: reqwest::Client,
}

impl Connection {
    /// Build a connection for one integration.
    ///
    /// `spec` is that integration's declarations. `secrets`, `variables`, and
    /// `http` are what the reads use. A source builds one the same way a
    /// type action does. Each row id is `{integration}:{name}`. An undeclared
    /// name names this connection's integration type.
    pub(crate) fn new(
        dataset_id: String,
        spec: ConnectionDeclarations,
        secrets: Arc<dyn SecretStore>,
        variables: Arc<dyn VariableStore>,
        http: reqwest::Client,
    ) -> Self {
        let integration = spec.integration;
        let secret_decls = spec
            .secrets
            .into_iter()
            .map(|secret| BoundSecret {
                row_id: format!("{integration}:{}", secret.name),
                name: secret.name,
                required: secret.required,
            })
            .collect();
        let variable_decls = spec
            .variables
            .into_iter()
            .map(|variable| BoundVariable {
                row_id: format!("{integration}:{}", variable.name),
                name: variable.name,
                required: variable.required,
                default_value: variable.default_value,
            })
            .collect();
        Self {
            dataset_id,
            scope: ConnectionScope::Integration {
                integration,
                type_ref: spec.type_ref,
            },
            secrets,
            variables,
            secret_decls,
            variable_decls,
            http,
        }
    }

    /// Build a connection for one package's loose declarations.
    ///
    /// `owner` is the action's project, the registry key. Each row id is the
    /// canonical declaration reference from `schema`: bare when `owner` is
    /// the running project, `{owner}.{name}` when it is a dependency. An
    /// undeclared name says this package does not declare it. An integration
    /// row of the same bare name is not one of these rows.
    pub(crate) fn for_package(
        dataset_id: String,
        schema: &VersionSchema,
        owner: &str,
        secrets: Arc<dyn SecretStore>,
        variables: Arc<dyn VariableStore>,
        http: reqwest::Client,
    ) -> Self {
        let secret_decls = schema
            .secrets_of(owner)
            .into_iter()
            .map(|secret| BoundSecret {
                row_id: schema.reference(owner, secret.name()),
                name: secret.name().to_string(),
                required: secret.required(),
            })
            .collect();
        let variable_decls = schema
            .variables_of(owner)
            .into_iter()
            .map(|variable| BoundVariable {
                row_id: schema.reference(owner, variable.name()),
                name: variable.name().to_string(),
                required: variable.required(),
                default_value: variable.default().to_string(),
            })
            .collect();
        Self {
            dataset_id,
            scope: ConnectionScope::Package {
                owner: owner.to_string(),
            },
            secrets,
            variables,
            secret_decls,
            variable_decls,
            http,
        }
    }

    /// This connection's plaintext for the bare declaration `name`.
    ///
    /// `name` is not resolved through [`VersionSchema::split`]. `Ok(None)`
    /// means the connection's declarations include `name` and this dataset
    /// has not set it. The plaintext is returned as stored, including `""`.
    /// A name those declarations omit is [`ConfigReadError::Undeclared`]:
    /// a package says it does not declare the name, and an integration names
    /// its type.
    pub fn secret(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        let Some(decl) = self.secret_decls.iter().find(|decl| decl.name == name) else {
            return Err(self.undeclared("secret", name));
        };
        match self.secrets.get(&self.dataset_id, &decl.row_id) {
            Ok(value) => Ok(value),
            Err(SecretError::NotFound) => Ok(None),
            Err(SecretError::Unavailable(message)) => Err(ConfigReadError::Unavailable(message)),
            Err(err) => Err(ConfigReadError::Failed(err.to_string())),
        }
    }

    /// This connection's value for the bare declaration `name`, or the
    /// declaration's non-empty default when nothing is stored.
    ///
    /// A stored value wins, including `""`. `Ok(None)` means the connection
    /// declares `name`, nothing is stored, and the default is empty. The same
    /// split as [`Self::secret`].
    pub fn variable(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        let Some(decl) = self.variable_decls.iter().find(|decl| decl.name == name) else {
            return Err(self.undeclared("variable", name));
        };
        match self.variables.get(&self.dataset_id, &decl.row_id) {
            Ok(stored) => Ok(with_default(stored, &decl.default_value)),
            Err(err) => Err(ConfigReadError::Failed(err.to_string())),
        }
    }

    /// This server's client. Connect and total timeouts are set. No proxy.
    /// A redirect is followed only when the next URL keeps the same scheme,
    /// host, and port.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn undeclared(&self, kind: &'static str, name: &str) -> ConfigReadError {
        let type_ref = match &self.scope {
            ConnectionScope::Package { .. } => None,
            ConnectionScope::Integration { type_ref, .. } => Some(type_ref.clone()),
        };
        ConfigReadError::Undeclared {
            kind,
            name: name.to_string(),
            type_ref,
        }
    }

    /// Required secrets and variables of this connection that `dataset_id`
    /// has not set. A secret counts as set when its row exists. The plaintext
    /// is not read. A variable counts as set when its row exists, or when its
    /// declaration's default is non-empty. Diagnostics are secrets by name,
    /// then variables by name.
    pub(crate) fn missing_required(&self) -> Result<Vec<Diagnostic>, ActionFailure> {
        let mut missing = Vec::new();
        let mut secrets: Vec<_> = self
            .secret_decls
            .iter()
            .filter(|secret| secret.required)
            .collect();
        secrets.sort_by(|a, b| a.name.cmp(&b.name));
        if !secrets.is_empty() {
            let stored = self
                .secrets
                .list(&self.dataset_id)
                .map_err(secret_failure)?;
            for secret in secrets {
                if stored.iter().any(|row| row.name == secret.row_id) {
                    continue;
                }
                missing.push(missing_diag("secret", &secret.name));
            }
        }

        let mut variables: Vec<_> = self
            .variable_decls
            .iter()
            .filter(|variable| variable.required)
            .collect();
        variables.sort_by(|a, b| a.name.cmp(&b.name));
        if !variables.is_empty() {
            let stored = self
                .variables
                .list(&self.dataset_id)
                .map_err(variable_failure)?;
            for variable in variables {
                if variable_is_set(&stored, &variable.row_id, &variable.default_value) {
                    continue;
                }
                missing.push(missing_diag("variable", &variable.name));
            }
        }
        Ok(missing)
    }
}

/// What an action handler is allowed to see.
///
/// `secret` and `variable` forward to [`Self::connection`]. An ordinary
/// action's connection reads the owning project's loose declarations (the
/// registry key) on [`Self::dataset_id`], the installing project's dataset.
/// The package's own datasets are never opened. A type action's connection
/// reads the one integration the address named. There is no secret store and
/// no variable store on this context.
pub struct ActionContext {
    pub dataset_id: String,
    pub data: Arc<dyn DataAdapter>,
    pub schema: VersionSchema,
    pub caller: AuthUser,
    pub input: HashMap<String, Value>,
    pub connection: Connection,
}

impl ActionContext {
    /// This connection's plaintext for the bare declaration `name`.
    ///
    /// Forwards to [`Connection::secret`]. An ordinary action does not resolve
    /// `name` through [`VersionSchema::split`], so a secret the installer
    /// declares under the same name is a different row and is not returned.
    /// A type action does not read a loose declaration. `Ok(None)` means the
    /// connection's declarations include `name` and this dataset has not set
    /// it. The plaintext is returned as stored, including `""`. A name those
    /// declarations omit is [`ConfigReadError::Undeclared`]: a package says
    /// it does not declare the name, and an integration names its type.
    pub fn secret(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        self.connection.secret(name)
    }

    /// This connection's value for the bare declaration `name`, or the
    /// declaration's non-empty default when nothing is stored.
    ///
    /// Forwards to [`Connection::variable`]. A stored value wins, including
    /// `""`. The same split as [`Self::secret`].
    pub fn variable(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        self.connection.variable(name)
    }

    /// This server's client. Connect and total timeouts are set. No proxy. A
    /// redirect is followed only when the next URL keeps the same scheme,
    /// host, and port.
    pub fn http(&self) -> &reqwest::Client {
        self.connection.http()
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

/// Which declaration a handler is bound to.
///
/// [`ActionKey::Package`] is an ordinary action: the owning project and the
/// bare name. [`ActionKey::Type`] is a type action: the type's project, the
/// type name, and the action name. The two do not collide.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActionKey {
    Package {
        project: String,
        name: String,
    },
    Type {
        project: String,
        type_name: String,
        name: String,
    },
}

/// Handlers keyed by [`ActionKey`].
#[derive(Clone, Default)]
pub struct ActionRegistry {
    handlers: HashMap<ActionKey, Arc<dyn ActionHandler>>,
}

impl ActionRegistry {
    /// Register `handler` for `project`'s bare action `name`.
    ///
    /// The closure returns a future and runs on the request's task. It can
    /// `.await` [`ActionContext::http`]. The context's connection is that
    /// package's loose declarations. A same-named action on another project,
    /// and a type action of the same name, do not call it.
    pub fn register_action<F, Fut>(
        &mut self,
        project: impl Into<String>,
        name: impl Into<String>,
        handler: F,
    ) where
        F: Fn(ActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
    {
        self.insert(
            ActionKey::Package {
                project: project.into(),
                name: name.into(),
            },
            handler,
        );
    }

    /// Register `handler` for `project`'s type `type_name`, action `name`.
    ///
    /// The context's connection is the integration the address named.
    /// `secret` and `variable` read that connection. A same-named action on
    /// another type, or an ordinary action of the same name, does not call it.
    pub fn register_type_action<F, Fut>(
        &mut self,
        project: impl Into<String>,
        type_name: impl Into<String>,
        name: impl Into<String>,
        handler: F,
    ) where
        F: Fn(ActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
    {
        self.insert(
            ActionKey::Type {
                project: project.into(),
                type_name: type_name.into(),
                name: name.into(),
            },
            handler,
        );
    }

    #[cfg(test)]
    pub fn contains(&self, key: &ActionKey) -> bool {
        self.handlers.contains_key(key)
    }

    fn insert<F, Fut>(&mut self, key: ActionKey, handler: F)
    where
        F: Fn(ActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
    {
        let handler: Arc<dyn ActionHandler> = Arc::new(handler);
        self.handlers.insert(key, handler);
    }

    fn get(&self, key: &ActionKey) -> Option<Arc<dyn ActionHandler>> {
        self.handlers.get(key).cloned()
    }
}

/// Record adapter, secret store, variable store, and HTTP client one run needs.
/// The `Arc`s and the client are cloned onto the handler so it can hold them
/// across `.await`.
///
/// `data` is the raw adapter so a handler can patch records, pending #120.
/// `variables` is the store `AppState` holds. A run does not build another
/// store on `data`.
pub struct HandlerDeps {
    pub data: Arc<dyn DataAdapter>,
    pub secrets: Arc<dyn SecretStore>,
    pub variables: Arc<dyn VariableStore>,
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
    /// A required secret or variable of this connection is unset on this
    /// dataset. The handler was not called. For an ordinary action that is
    /// the owning package's loose declarations. For a type action it is this
    /// integration's declarations only.
    MissingConfig(Vec<Diagnostic>),
    Done(serde_json::Value),
    Failed(ActionFailure),
}

struct Prepared {
    key: ActionKey,
    scope: ConnectionScope,
    input: HashMap<String, Value>,
}

/// Validate `address`'s input, refuse a run whose connection is missing a
/// required value, and call the handler registered for that address.
///
/// The caller has already resolved the address and checked access. An invalid
/// input does not call the handler and does not ask for configuration. A
/// missing handler is [`Dispatch::NoHandler`] before the required-value
/// check, so an action with no handler does not demand configuration. Once a
/// handler exists, the check is that connection's required secrets and
/// variables: the owning package's loose declarations, or this integration's
/// declarations and nothing else.
pub async fn dispatch(
    schema: &VersionSchema,
    registry: &ActionRegistry,
    dataset_id: &str,
    deps: HandlerDeps,
    caller: &AuthUser,
    address: &ActionAddress,
    input: &Map<String, serde_json::Value>,
) -> Dispatch {
    let prepared = match prepare(schema, address, input) {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    let Some(handler) = registry.get(&prepared.key) else {
        return no_handler(&prepared.key);
    };
    let connection = match connect(
        schema,
        dataset_id,
        prepared.scope,
        deps.secrets,
        deps.variables,
        deps.http,
    ) {
        Ok(connection) => connection,
        Err(outcome) => return outcome,
    };
    let missing = match connection.missing_required() {
        Ok(missing) => missing,
        Err(failure) => return Dispatch::Failed(failure),
    };
    if !missing.is_empty() {
        return Dispatch::MissingConfig(missing);
    }
    let ctx = ActionContext {
        dataset_id: dataset_id.to_string(),
        // Pending #120: the handler still holds the raw adapter so it can
        // patch records. Variable reads go through `connection`, which was
        // given `deps.variables`.
        data: deps.data,
        schema: schema.clone(),
        caller: caller.clone(),
        input: prepared.input,
        connection,
    };
    match handler.call(ctx).await {
        Ok(value) => Dispatch::Done(value),
        Err(failure) => Dispatch::Failed(failure),
    }
}

fn prepare(
    schema: &VersionSchema,
    address: &ActionAddress,
    input: &Map<String, serde_json::Value>,
) -> Result<Prepared, Dispatch> {
    match &address.kind {
        ActionAddressKind::Ordinary => {
            let name = schema.reference(&address.project, &address.local);
            let Some(action) = schema.action(&name) else {
                return Err(Dispatch::NotFound);
            };
            let input = match validate_action_input(schema, &action, input) {
                Ok(input) => input,
                Err(report) => return Err(Dispatch::Invalid(report)),
            };
            let owner = action.project().to_string();
            Ok(Prepared {
                key: ActionKey::Package {
                    project: owner.clone(),
                    name: action.name().to_string(),
                },
                scope: ConnectionScope::Package { owner },
                input,
            })
        }
        ActionAddressKind::Type {
            type_project,
            type_name,
            action,
        } => {
            let action_ref = schema.reference(&address.project, &address.local);
            let input = match validate_type_action_input(schema, &action_ref, action, input) {
                Ok(input) => input,
                Err(report) => return Err(Dispatch::Invalid(report)),
            };
            let Some(integration_name) = address.integration.as_deref() else {
                return Err(Dispatch::NotFound);
            };
            let Some(spec) = schema.connection_declarations(&address.project, integration_name)
            else {
                return Err(Dispatch::NotFound);
            };
            Ok(Prepared {
                key: ActionKey::Type {
                    project: type_project.clone(),
                    type_name: type_name.clone(),
                    name: action.name().to_string(),
                },
                scope: ConnectionScope::Integration {
                    integration: spec.integration,
                    type_ref: spec.type_ref,
                },
                input,
            })
        }
    }
}

/// Build the connection `prepare` named.
///
/// An integration's declarations are read again from `schema`. The canonical
/// name on the scope is what [`VersionSchema::split`] turns back into the
/// project and the bare name, including a bare name that itself contains `.`.
fn connect(
    schema: &VersionSchema,
    dataset_id: &str,
    scope: ConnectionScope,
    secrets: Arc<dyn SecretStore>,
    variables: Arc<dyn VariableStore>,
    http: reqwest::Client,
) -> Result<Connection, Dispatch> {
    match scope {
        ConnectionScope::Package { owner } => Ok(Connection::for_package(
            dataset_id.to_string(),
            schema,
            &owner,
            secrets,
            variables,
            http,
        )),
        ConnectionScope::Integration { integration, .. } => {
            let (project, name) = schema.split(&integration);
            let Some(spec) = schema.connection_declarations(project, name) else {
                return Err(Dispatch::NotFound);
            };
            Ok(Connection::new(
                dataset_id.to_string(),
                spec,
                secrets,
                variables,
                http,
            ))
        }
    }
}

/// The HTTP line is `no handler for action {project}.{name}`. A type action
/// puts `{type_project}.{type_name}` in `project`, so the line stays
/// `{type_project}.{type_name}.{action}`.
fn no_handler(key: &ActionKey) -> Dispatch {
    match key {
        ActionKey::Package { project, name } => Dispatch::NoHandler {
            project: project.clone(),
            name: name.clone(),
        },
        ActionKey::Type {
            project,
            type_name,
            name,
        } => Dispatch::NoHandler {
            project: format!("{project}.{type_name}"),
            name: name.clone(),
        },
    }
}

/// A variable counts as set when a row named `id` exists, or when `default`
/// is non-empty. An empty stored value is a row, so it counts.
fn variable_is_set(stored: &[VariableMeta], id: &str, default: &str) -> bool {
    stored.iter().any(|row| row.name == id) || !default.is_empty()
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

fn variable_failure(err: crate::values::VariableError) -> ActionFailure {
    ActionFailure::Failed {
        message: err.to_string(),
        diagnostics: Vec::new(),
    }
}

/// Build the client one server stores on `AppState`.
///
/// [`crate::server::build_app`] calls this once.
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
    use crate::http::version_schema::AddressResolution;
    use crate::validation::kind;
    use crate::values::{
        KeyStatus, LakeSecretStore, LakeVariableStore, SecretMeta, VariableError, VariableMeta,
        VariableStore,
    };
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
        http_client()
    }

    fn address_of(schema: &VersionSchema, name: &str) -> Result<ActionAddress, Dispatch> {
        match schema.action_address(name) {
            AddressResolution::Resolved(address) => Ok(address),
            AddressResolution::Missing => Err(Dispatch::NotFound),
        }
    }

    fn deps(data: Arc<dyn DataAdapter>, secrets: Arc<dyn SecretStore>) -> HandlerDeps {
        let variables: Arc<dyn VariableStore> = Arc::new(LakeVariableStore::new(Arc::clone(&data)));
        HandlerDeps {
            data,
            secrets,
            variables,
            http: test_http(),
        }
    }

    struct Ran {
        echo: Arc<AtomicBool>,
        decoy: Arc<AtomicBool>,
        registry: ActionRegistry,
    }

    fn registry() -> Ran {
        let echo = Arc::new(AtomicBool::new(false));
        let decoy = Arc::new(AtomicBool::new(false));
        let mut registry = ActionRegistry::default();
        let echo_flag = Arc::clone(&echo);
        registry.register_action("ben/crm", "echo", move |_| {
            let echo_flag = Arc::clone(&echo_flag);
            async move {
                echo_flag.store(true, Ordering::SeqCst);
                Ok(json!({ "ran": true }))
            }
        });
        let decoy_flag = Arc::clone(&decoy);
        registry.register_action("alice/other", "echo", move |_| {
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

    #[test]
    fn package_and_type_keys_do_not_collide() {
        let mut registry = ActionRegistry::default();
        let package = ActionKey::Package {
            project: "alice/pkg".into(),
            name: "read".into(),
        };
        let typed = ActionKey::Type {
            project: "alice/pkg".into(),
            type_name: "warehouse".into(),
            name: "read".into(),
        };
        assert!(!registry.contains(&package));
        assert!(!registry.contains(&typed));
        registry.register_action("alice/pkg", "read", |_| async { Ok(json!({})) });
        registry.register_type_action("alice/pkg", "warehouse", "read", |_| async {
            Ok(json!({ "which": "warehouse" }))
        });
        registry.register_type_action("alice/pkg", "other", "read", |_| async {
            Ok(json!({ "which": "other" }))
        });
        assert!(registry.contains(&package));
        assert!(registry.contains(&typed));
        assert!(registry.contains(&ActionKey::Type {
            project: "alice/pkg".into(),
            type_name: "other".into(),
            name: "read".into(),
        }));
        assert!(!registry.contains(&ActionKey::Type {
            project: "alice/shop".into(),
            type_name: "warehouse".into(),
            name: "read".into(),
        }));
        assert!(!registry.contains(&ActionKey::Package {
            project: "alice/shop".into(),
            name: "read".into(),
        }));
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
        let Ok(address) = address_of(schema, name) else {
            return Dispatch::NotFound;
        };
        dispatch(
            schema,
            &ran.registry,
            "ben/crm/dev",
            deps(data, secrets),
            &caller(),
            &address,
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
        registry: &ActionRegistry,
    ) -> Dispatch {
        let name = if schema.project_id() == STORE {
            "alice/pkg.pull"
        } else {
            "pull"
        };
        let Ok(address) = address_of(schema, name) else {
            return Dispatch::NotFound;
        };
        dispatch(
            schema,
            registry,
            dataset_id,
            deps(world.data.clone(), secrets),
            &caller(),
            &address,
            &Map::new(),
        )
        .await
    }

    #[tokio::test]
    async fn missing_required_does_not_run_or_read_plaintext() {
        let world = World::new();
        let schema = world.store_schema();
        let ran = Arc::new(AtomicBool::new(false));
        let mut registry = ActionRegistry::default();
        let flag = Arc::clone(&ran);
        registry.register_action(PKG, "pull", move |_| {
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

        LakeVariableStore::new(Arc::clone(&world.data))
            .set(STORE_DATASET, "alice/pkg.region", "")
            .unwrap();
        let present: Arc<dyn SecretStore> = Arc::new(ListOnly {
            names: vec!["alice/pkg.token".into()],
        });
        let outcome = dispatch_pull(&schema, &world, present, STORE_DATASET, &registry).await;
        assert!(matches!(outcome, Dispatch::Done(_)), "{outcome:?}");
        assert!(ran.load(Ordering::SeqCst));
    }

    /// A row stored as `{integration}:{name}` is not the package's loose secret.
    /// The ordinary required check still names that loose secret.
    #[tokio::test]
    async fn connection_row_does_not_satisfy_a_loose_secret() {
        let world = World::new();
        let schema = world.store_schema();
        let ran = Arc::new(AtomicBool::new(false));
        let mut registry = ActionRegistry::default();
        let flag = Arc::clone(&ran);
        registry.register_action(PKG, "pull", move |_| {
            let flag = Arc::clone(&flag);
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(json!({ "ran": true }))
            }
        });
        let present: Arc<dyn SecretStore> = Arc::new(ListOnly {
            names: vec!["sf_east:token".into(), "token".into()],
        });
        let outcome = dispatch_pull(&schema, &world, present, STORE_DATASET, &registry).await;
        assert_eq!(
            messages(&outcome),
            vec!["secret 'token' is not set", "variable 'region' is not set",]
        );
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn handler_reads_the_installers_values_for_its_own_package() {
        let world = World::new();
        let mut registry = ActionRegistry::default();
        registry.register_action(PKG, "pull", |ctx| async move {
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
        LakeVariableStore::new(Arc::clone(&world.data))
            .set(STORE_DATASET, "alice/pkg.region", "eu")
            .unwrap();

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
        // A connection row of the same bare name is not this package's secret.
        world
            .secrets
            .put(STORE_DATASET, "sf_east:token", "connection-token")
            .unwrap();
        LakeVariableStore::new(Arc::clone(&world.data))
            .set(
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
        assert!(!text.contains("connection-token"), "{text}");

        // The package's own site reads the package dataset. The installer's
        // row is a different dataset and is not visible here.
        LakeVariableStore::new(Arc::clone(&world.data))
            .set(PKG_DATASET, "region", "pkg-region")
            .unwrap();
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

    /// The handler reads `HandlerDeps.variables`. A row on the raw adapter is
    /// not that store, so a later non-lake store stays what the handler sees.
    #[tokio::test]
    async fn handler_reads_the_variable_store_not_the_raw_adapter() {
        let world = World::new();
        LakeVariableStore::new(Arc::clone(&world.data))
            .set(STORE_DATASET, "alice/pkg.region", "from-the-lake")
            .unwrap();
        world
            .secrets
            .put(STORE_DATASET, "alice/pkg.token", "installer-token")
            .unwrap();
        let variables: Arc<dyn VariableStore> = Arc::new(FixedVariables {
            rows: vec![VariableMeta {
                name: "alice/pkg.region".into(),
                value: "from-the-store".into(),
                updated_at: "t".into(),
            }],
        });
        let mut registry = ActionRegistry::default();
        registry.register_action(PKG, "pull", |ctx| async move {
            Ok(json!({ "region": ctx.variable("region")? }))
        });
        let schema = world.store_schema();
        let address = address_of(&schema, "alice/pkg.pull").expect("pull resolves");
        let outcome = dispatch(
            &schema,
            &registry,
            STORE_DATASET,
            HandlerDeps {
                data: world.data.clone(),
                secrets: world.secrets.clone(),
                variables,
                http: test_http(),
            },
            &caller(),
            &address,
            &Map::new(),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the handler to run, got {outcome:?}");
        };
        assert_eq!(body["region"], "from-the-store");
    }

    struct FixedVariables {
        rows: Vec<VariableMeta>,
    }

    impl VariableStore for FixedVariables {
        fn set(&self, _: &str, _: &str, _: &str) -> Result<VariableMeta, VariableError> {
            panic!("not used");
        }

        fn get(&self, _: &str, name: &str) -> Result<Option<String>, VariableError> {
            Ok(self
                .rows
                .iter()
                .find(|row| row.name == name)
                .map(|row| row.value.clone()))
        }

        fn delete(&self, _: &str, _: &str) -> Result<(), VariableError> {
            panic!("not used");
        }

        fn list(&self, _: &str) -> Result<Vec<VariableMeta>, VariableError> {
            Ok(self.rows.clone())
        }

        fn delete_dataset(&self, _: &str) -> Result<(), VariableError> {
            panic!("not used");
        }
    }

    #[tokio::test]
    async fn missing_secret_key_over_a_stored_row_is_unavailable() {
        let world = World::new();
        world
            .secrets
            .put(STORE_DATASET, "alice/pkg.token", "installer-token")
            .unwrap();
        LakeVariableStore::new(Arc::clone(&world.data))
            .set(STORE_DATASET, "alice/pkg.region", "eu")
            .unwrap();
        let missing: Arc<dyn SecretStore> =
            Arc::new(LakeSecretStore::new(world.data.clone(), KeyStatus::Missing));
        let mut registry = ActionRegistry::default();
        registry.register_action(PKG, "pull", |ctx| async move {
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
