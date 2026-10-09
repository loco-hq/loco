//! Registries for integration types.
//!
//! A source is keyed by the owning project and the type name. A type action
//! is keyed by the owning project, the type name, and the action name.
//! Ordinary actions stay [`crate::actions::HandlerRegistry`], keyed by
//! `(project, name)` only.
//!
//! Both registries are empty in the production binary. A source is a
//! [`crate::source::CollectionSource`]: `/data` dispatches it for that type's
//! standard and custom collections. An address with no registration is 501
//! before the required-value check. A resolved type action with no handler
//! is 501. One with a handler runs with a [`TypeActionContext`] for the
//! integration the address named. That context's connection reads the
//! integration's values. It does not read the type project's loose secrets.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use loco_lake::{DataAdapter, Value};
use serde_json::Map;

use crate::actions::{ActionFailure, ConfigReadError, Connection, Dispatch, HandlerDeps};
use crate::auth::AuthUser;
use crate::http::version_schema::{ActionAddress, ActionAddressKind, VersionSchema};
use crate::validation::validate_type_action_input;

/// What a type-action handler receives.
///
/// `connection` is the integration the address named. It is not optional.
/// This context has no secret store. [`Self::secret`] and [`Self::variable`]
/// forward to the connection. They do not read a loose declaration, and a
/// handler written against [`crate::actions::ActionContext`] does not type-check
/// here. The schema and the lake adapter are the same ones an ordinary action
/// receives, so a handler can patch records, pending #120. Variable reads
/// use the store on [`HandlerDeps`], not a store built on that adapter.
pub struct TypeActionContext {
    pub dataset_id: String,
    pub data: Arc<dyn DataAdapter>,
    pub schema: VersionSchema,
    pub caller: AuthUser,
    pub input: HashMap<String, Value>,
    pub connection: Connection,
}

impl TypeActionContext {
    /// This connection's plaintext for the type's bare declaration `name`.
    ///
    /// Forwards to [`Connection::secret`]. A loose declaration of the same
    /// name is not read. A name the type does not declare names that type.
    pub fn secret(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        self.connection.secret(name)
    }

    /// This connection's value for the type's bare declaration `name`.
    ///
    /// Forwards to [`Connection::variable`]. A loose declaration of the same
    /// name is not read.
    pub fn variable(&self, name: &str) -> Result<Option<String>, ConfigReadError> {
        self.connection.variable(name)
    }

    /// The process-wide client. The same one [`Connection::http`] returns.
    pub fn http(&self) -> &reqwest::Client {
        self.connection.http()
    }
}

/// Sources keyed by `(owning project, type name)`.
///
/// One source serves every integration of that type. The connection on the
/// call says which integration. Empty in the server binary.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    sources: HashMap<(String, String), Arc<dyn crate::source::CollectionSource>>,
}

impl SourceRegistry {
    /// What the server binary registers: the BrickLink store API for
    /// `loco/bricklink`'s `bricklink` type. [`Default`] is empty.
    pub fn production() -> Self {
        let mut sources = Self::default();
        sources.register(
            crate::bricklink::PROJECT,
            crate::bricklink::TYPE,
            crate::bricklink::BrickLinkSource,
        );
        sources
    }

    /// Register `source` for `project`'s integration type `type_name`.
    pub fn register<S>(
        &mut self,
        project: impl Into<String>,
        type_name: impl Into<String>,
        source: S,
    ) where
        S: crate::source::CollectionSource + 'static,
    {
        self.sources
            .insert((project.into(), type_name.into()), Arc::new(source));
    }

    pub fn get(
        &self,
        project: &str,
        type_name: &str,
    ) -> Option<Arc<dyn crate::source::CollectionSource>> {
        self.sources
            .get(&(project.to_string(), type_name.to_string()))
            .cloned()
    }

    pub fn contains(&self, project: &str, type_name: &str) -> bool {
        self.sources
            .contains_key(&(project.to_string(), type_name.to_string()))
    }
}

type TypeActionFuture =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, ActionFailure>> + Send>>;

trait TypeActionHandler: Send + Sync {
    fn call(&self, ctx: TypeActionContext) -> TypeActionFuture;
}

impl<F, Fut> TypeActionHandler for F
where
    F: Fn(TypeActionContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
{
    fn call(&self, ctx: TypeActionContext) -> TypeActionFuture {
        Box::pin(self(ctx))
    }
}

/// Type-action handlers keyed by `(owning project, type name, action name)`.
#[derive(Clone, Default)]
pub struct TypeActionRegistry {
    handlers: HashMap<(String, String, String), Arc<dyn TypeActionHandler>>,
}

impl TypeActionRegistry {
    /// Register `handler` for `project`'s type `type_name`, action `name`.
    ///
    /// The handler receives a [`TypeActionContext`]. `secret` and `variable`
    /// read that context's connection. A same-named action on another type,
    /// or an ordinary action of the same name, does not call it.
    pub fn register<F, Fut>(
        &mut self,
        project: impl Into<String>,
        type_name: impl Into<String>,
        name: impl Into<String>,
        handler: F,
    ) where
        F: Fn(TypeActionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
    {
        let handler: Arc<dyn TypeActionHandler> = Arc::new(handler);
        self.handlers
            .insert((project.into(), type_name.into(), name.into()), handler);
    }

    pub fn contains(&self, project: &str, type_name: &str, name: &str) -> bool {
        self.handlers
            .contains_key(&(project.to_string(), type_name.to_string(), name.to_string()))
    }

    fn get(
        &self,
        project: &str,
        type_name: &str,
        name: &str,
    ) -> Option<Arc<dyn TypeActionHandler>> {
        self.handlers
            .get(&(project.to_string(), type_name.to_string(), name.to_string()))
            .cloned()
    }
}

/// Validate `input`, then run the handler registered for the type action
/// `address` resolved to.
///
/// The caller has already resolved the address and checked access. A missing
/// handler is [`Dispatch::NoHandler`] before the required-value check, the
/// same as [`crate::actions::dispatch`]: an action with no handler does not
/// demand configuration. Once a handler exists, every required secret and
/// variable of **this** integration must be set on `dataset_id`. Loose
/// version-level declarations are not part of that check and are not read.
/// The handler receives a [`TypeActionContext`]. Its connection reads only
/// that integration. The context has no secret store.
pub(crate) async fn dispatch_type_action(
    schema: &VersionSchema,
    registry: &TypeActionRegistry,
    dataset_id: &str,
    deps: HandlerDeps,
    caller: &AuthUser,
    address: &ActionAddress,
    input: &Map<String, serde_json::Value>,
) -> Dispatch {
    let ActionAddressKind::Type {
        type_project,
        type_name,
        action,
    } = &address.kind
    else {
        return Dispatch::NotFound;
    };
    let action_ref = schema.reference(&address.project, &address.local);
    let input = match validate_type_action_input(schema, &action_ref, action, input) {
        Ok(input) => input,
        Err(report) => return Dispatch::Invalid(report),
    };
    let Some(integration_name) = address.integration.as_deref() else {
        return Dispatch::NotFound;
    };
    let Some(spec) = schema.connection_declarations(&address.project, integration_name) else {
        return Dispatch::NotFound;
    };
    let Some(handler) = registry.get(type_project, type_name, action.name()) else {
        return Dispatch::NoHandler {
            // The HTTP line is `no handler for action {project}.{name}`.
            // `{type_project}.{type_name}` keeps the three-part form
            // `{type_project}.{type_name}.{action}`.
            project: format!("{type_project}.{type_name}"),
            name: action.name().to_string(),
        };
    };
    let connection = Connection::new(
        dataset_id.to_string(),
        spec,
        deps.secrets,
        deps.variables,
        deps.http,
    );
    let missing = match connection.missing_required() {
        Ok(missing) => missing,
        Err(failure) => return Dispatch::Failed(failure),
    };
    if !missing.is_empty() {
        return Dispatch::MissingConfig(missing);
    }
    let ctx = TypeActionContext {
        dataset_id: dataset_id.to_string(),
        // Pending #120: the handler still holds the raw adapter so it can
        // patch records. Variable reads go through `connection`, which was
        // given `deps.variables`.
        data: deps.data,
        schema: schema.clone(),
        caller: caller.clone(),
        input,
        connection,
    };
    match handler.call(ctx).await {
        Ok(value) => Dispatch::Done(value),
        Err(failure) => Dispatch::Failed(failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::source::{
        async_trait, Capabilities, CollectionSource, SourceCall, SourceError, SourcePage,
        SourceRecord,
    };

    /// Registry tests only check the key. The methods are never called.
    struct Unused;

    #[async_trait]
    impl CollectionSource for Unused {
        fn capabilities(&self) -> Capabilities {
            Capabilities::none()
        }

        async fn get(
            &self,
            _: SourceCall<'_>,
            _: &str,
            _: &str,
        ) -> Result<Option<SourceRecord>, SourceError> {
            panic!("unused")
        }

        async fn list(&self, _: SourceCall<'_>, _: &str) -> Result<Vec<SourceRecord>, SourceError> {
            panic!("unused")
        }

        async fn insert(
            &self,
            _: SourceCall<'_>,
            _: &str,
            _: loco_lake::InsertRequest,
        ) -> Result<SourceRecord, SourceError> {
            panic!("unused")
        }

        async fn update(
            &self,
            _: SourceCall<'_>,
            _: &str,
            _: &str,
            _: loco_lake::UpdatePatch,
        ) -> Result<SourceRecord, SourceError> {
            panic!("unused")
        }

        async fn delete(&self, _: SourceCall<'_>, _: &str, _: &str) -> Result<(), SourceError> {
            panic!("unused")
        }

        async fn query(
            &self,
            _: SourceCall<'_>,
            _: &[loco_lake::LakeQuery],
        ) -> Result<Vec<SourcePage>, SourceError> {
            panic!("unused")
        }
    }

    #[test]
    fn registries_key_by_type_and_start_empty() {
        let sources = SourceRegistry::default();
        assert!(!sources.contains("alice/pkg", "bricklink"));

        let mut sources = SourceRegistry::default();
        sources.register("alice/pkg", "bricklink", Unused);
        sources.register("alice/pkg", "warehouse", Unused);
        assert!(sources.contains("alice/pkg", "bricklink"));
        assert!(sources.contains("alice/pkg", "warehouse"));
        assert!(!sources.contains("alice/shop", "bricklink"));

        let mut type_actions = TypeActionRegistry::default();
        assert!(!type_actions.contains("alice/pkg", "bricklink", "set_status"));
        type_actions.register("alice/pkg", "bricklink", "set_status", |_| async {
            Ok(serde_json::json!({"type": "bricklink"}))
        });
        type_actions.register("alice/pkg", "warehouse", "set_status", |_| async {
            Ok(serde_json::json!({"type": "warehouse"}))
        });
        assert!(type_actions.contains("alice/pkg", "bricklink", "set_status"));
        assert!(type_actions.contains("alice/pkg", "warehouse", "set_status"));
        assert!(!type_actions.contains("alice/pkg", "bricklink", "other"));
        assert!(!type_actions.contains("alice/shop", "bricklink", "set_status"));
    }

    use std::sync::atomic::{AtomicBool, Ordering};

    use loco_lake::InMemoryAdapter;
    use serde_json::{json, Map};

    use crate::actions::HandlerDeps;
    use crate::auth::AuthUser;
    use crate::http::version_schema::{AddressResolution, VersionSchema};
    use crate::validation::kind;
    use crate::values::{
        KeyStatus, LakeSecretStore, LakeVariableStore, SecretError, SecretMeta, SecretStore,
        VariableStore,
    };
    use crate::{
        Integration, IntegrationAction, IntegrationSecret, IntegrationType, IntegrationVariable,
        Manifest, SchemaStore, Secret,
    };

    const VERSION: &str = "0.0.1-dev";
    const PKG: &str = "alice/pkg";
    const SHOP: &str = "alice/shop";
    const DATASET: &str = "alice/shop/dev";

    /// `list` reports names. `get` panics, so a required check that decrypted
    /// would fail the test. `allow_list` false panics on `list` too, which is
    /// how a missing handler proves it did not ask for configuration.
    struct ListOnly {
        names: Vec<String>,
        allow_list: bool,
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
            assert!(self.allow_list, "this path must not list secrets");
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

    struct Fixture {
        _dir: tempfile::TempDir,
        store: std::sync::Arc<SchemaStore>,
        data: std::sync::Arc<dyn loco_lake::DataAdapter>,
        secrets: std::sync::Arc<dyn SecretStore>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = std::sync::Arc::new(SchemaStore::load(dir.path()).unwrap());
            store
                .manifests()
                .create(Manifest::new(
                    PKG.into(),
                    VERSION.into(),
                    Vec::new(),
                    Vec::new(),
                ))
                .unwrap();
            let pkg = VersionSchema::new(store.clone(), PKG, VERSION);
            pkg.create_integration_type(IntegrationType {
                name: "warehouse".into(),
                label: "Warehouse".into(),
                secrets: vec![
                    IntegrationSecret {
                        name: "token".into(),
                        required: true,
                        ..IntegrationSecret::default()
                    },
                    IntegrationSecret {
                        name: "consumer_key".into(),
                        required: false,
                        ..IntegrationSecret::default()
                    },
                ],
                variables: vec![
                    IntegrationVariable {
                        name: "region".into(),
                        default: "us".into(),
                        required: false,
                        ..Default::default()
                    },
                    IntegrationVariable {
                        name: "lane".into(),
                        required: true,
                        ..Default::default()
                    },
                ],
                actions: vec![
                    IntegrationAction {
                        name: "read".into(),
                        label: "Read".into(),
                        ..IntegrationAction::default()
                    },
                    IntegrationAction {
                        name: "unread".into(),
                        label: "Unread".into(),
                        ..IntegrationAction::default()
                    },
                ],
                ..IntegrationType::default()
            })
            .unwrap();
            pkg.create_integration(Integration {
                name: "store".into(),
                label: "Store".into(),
                r#type: "warehouse".into(),
                ..Integration::default()
            })
            .unwrap();
            store
                .manifests()
                .create(Manifest::new(
                    SHOP.into(),
                    VERSION.into(),
                    vec![format!("{PKG}@{VERSION}")],
                    Vec::new(),
                ))
                .unwrap();
            store
                .secrets()
                .create(Secret::new(
                    SHOP.into(),
                    VERSION.into(),
                    "token".into(),
                    "Loose token".into(),
                    String::new(),
                    true,
                ))
                .unwrap();
            store
                .secrets()
                .create(Secret::new(
                    SHOP.into(),
                    VERSION.into(),
                    "license".into(),
                    "License".into(),
                    String::new(),
                    true,
                ))
                .unwrap();
            let shop = VersionSchema::new(store.clone(), SHOP, VERSION);
            for name in ["sf_east", "sf_west"] {
                shop.create_integration(Integration {
                    name: name.into(),
                    label: name.into(),
                    r#type: "alice/pkg.warehouse".into(),
                    ..Integration::default()
                })
                .unwrap();
            }
            let data: std::sync::Arc<dyn loco_lake::DataAdapter> =
                std::sync::Arc::new(InMemoryAdapter::new());
            let secrets: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(
                LakeSecretStore::new(data.clone(), KeyStatus::Ready([9u8; 32])),
            );
            Self {
                _dir: dir,
                store,
                data,
                secrets,
            }
        }

        fn schema(&self) -> VersionSchema {
            VersionSchema::new_read_only(self.store.clone(), SHOP, VERSION)
        }
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

    fn deps(
        data: std::sync::Arc<dyn loco_lake::DataAdapter>,
        secrets: std::sync::Arc<dyn SecretStore>,
    ) -> HandlerDeps {
        let variables: std::sync::Arc<dyn VariableStore> =
            std::sync::Arc::new(LakeVariableStore::new(std::sync::Arc::clone(&data)));
        HandlerDeps {
            data,
            secrets,
            variables,
            http: crate::actions::http_client(),
        }
    }

    fn address(schema: &VersionSchema, name: &str) -> crate::http::version_schema::ActionAddress {
        match schema.action_address(name) {
            AddressResolution::Resolved(address) => address,
            AddressResolution::Missing => panic!("{name} did not resolve"),
        }
    }

    async fn run(
        fixture: &Fixture,
        registry: &TypeActionRegistry,
        secrets: std::sync::Arc<dyn SecretStore>,
        name: &str,
        input: serde_json::Value,
    ) -> Dispatch {
        let schema = fixture.schema();
        let address = address(&schema, name);
        let input = input.as_object().cloned().unwrap_or_else(Map::new);
        dispatch_type_action(
            &schema,
            registry,
            DATASET,
            deps(fixture.data.clone(), secrets),
            &caller(),
            &address,
            &input,
        )
        .await
    }

    fn read_registry(flag: std::sync::Arc<AtomicBool>) -> TypeActionRegistry {
        let mut registry = TypeActionRegistry::default();
        registry.register(PKG, "warehouse", "read", move |ctx| {
            let flag = std::sync::Arc::clone(&flag);
            async move {
                flag.store(true, Ordering::SeqCst);
                let integration = ctx.connection.integration.clone();
                let loose = match ctx.secret("license") {
                    Ok(value) => json!(value),
                    Err(err) => json!({ "error": err.to_string() }),
                };
                Ok(json!({
                    "integration": integration,
                    "token": ctx.secret("token")?,
                    "consumer_key": ctx.secret("consumer_key")?,
                    "region": ctx.variable("region")?,
                    "loose": loose,
                }))
            }
        });
        registry.register("alice/pkg", "other", "read", |_| async {
            Ok(json!({"which": "other"}))
        });
        registry.register("alice/shop", "warehouse", "read", |_| async {
            Ok(json!({"which": "shop"}))
        });
        registry
    }

    fn messages(outcome: &Dispatch) -> Vec<&str> {
        let Dispatch::MissingConfig(diagnostics) = outcome else {
            panic!("expected missing configuration, got {outcome:?}");
        };
        diagnostics.iter().map(|d| d.message.as_str()).collect()
    }

    #[tokio::test]
    async fn each_integration_reads_its_own_values() {
        let fixture = Fixture::new();
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        let registry = read_registry(std::sync::Arc::clone(&ran));
        fixture
            .secrets
            .put(DATASET, "token", "loose-token")
            .unwrap();
        fixture
            .secrets
            .put(DATASET, "license", "loose-license")
            .unwrap();
        fixture
            .secrets
            .put(DATASET, "sf_east:token", "east-token")
            .unwrap();
        fixture
            .secrets
            .put(DATASET, "sf_west:token", "west-token")
            .unwrap();
        fixture
            .secrets
            .put(DATASET, "alice/pkg.store:token", "store-token")
            .unwrap();
        for id in ["sf_east:lane", "sf_west:lane", "alice/pkg.store:lane"] {
            LakeVariableStore::new(std::sync::Arc::clone(&fixture.data))
                .set(DATASET, id, "1")
                .unwrap();
        }

        ran.store(false, Ordering::SeqCst);
        let outcome = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "sf_east:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected east to run, got {outcome:?}");
        };
        assert!(ran.load(Ordering::SeqCst));
        assert_eq!(body["integration"], "sf_east");
        assert_eq!(body["token"], "east-token");
        assert!(body["consumer_key"].is_null());
        assert_eq!(body["region"], "us");
        assert_eq!(
            body["loose"]["error"],
            "secret 'license' is not declared by integration type 'alice/pkg.warehouse'"
        );
        let text = body.to_string();
        assert!(!text.contains("loose-token"), "{text}");
        assert!(!text.contains("loose-license"), "{text}");
        assert!(!text.contains("west-token"), "{text}");
        assert!(!text.contains("store-token"), "{text}");

        let outcome = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "sf_west:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected west to run, got {outcome:?}");
        };
        assert_eq!(body["integration"], "sf_west");
        assert_eq!(body["token"], "west-token");
        assert!(body["consumer_key"].is_null());
        assert!(!body.to_string().contains("east-token"));

        fixture
            .secrets
            .put(DATASET, "sf_east:consumer_key", "east-key")
            .unwrap();
        LakeVariableStore::new(std::sync::Arc::clone(&fixture.data))
            .set(DATASET, "sf_east:region", "")
            .unwrap();
        let outcome = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "sf_east:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected east to reread, got {outcome:?}");
        };
        assert_eq!(body["consumer_key"], "east-key");
        assert_eq!(body["region"], "");
        let west = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "sf_west:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = west else {
            panic!("expected west to stay on its own row, got {west:?}");
        };
        assert!(body["consumer_key"].is_null());
        assert_eq!(body["region"], "us");

        let outcome = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "alice/pkg.store:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the dependency integration to run, got {outcome:?}");
        };
        assert_eq!(body["integration"], "alice/pkg.store");
        assert_eq!(body["token"], "store-token");
        assert!(body["consumer_key"].is_null());
        assert_eq!(
            body["loose"]["error"],
            "secret 'license' is not declared by integration type 'alice/pkg.warehouse'"
        );
        assert!(!body.to_string().contains("east-token"));
    }

    #[tokio::test]
    async fn missing_required_names_this_integration_only() {
        let fixture = Fixture::new();
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        let registry = read_registry(std::sync::Arc::clone(&ran));
        let listed: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(ListOnly {
            names: vec!["token".into(), "license".into(), "sf_west:token".into()],
            allow_list: true,
        });
        let outcome = run(&fixture, &registry, listed, "sf_east:read", json!({})).await;
        assert_eq!(
            messages(&outcome),
            vec!["secret 'token' is not set", "variable 'lane' is not set",]
        );
        assert!(!ran.load(Ordering::SeqCst));
        let Dispatch::MissingConfig(diagnostics) = &outcome else {
            unreachable!();
        };
        assert!(diagnostics.iter().all(|d| d.kind == kind::REQUIRED));
        let text = diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!text.contains("license"), "{text}");
        assert!(!text.contains("loose"), "{text}");
    }

    #[tokio::test]
    async fn missing_handler_does_not_require_configuration() {
        let fixture = Fixture::new();
        let registry = TypeActionRegistry::default();
        let listed: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(ListOnly {
            names: Vec::new(),
            allow_list: false,
        });
        let outcome = run(&fixture, &registry, listed, "sf_east:unread", json!({})).await;
        match outcome {
            Dispatch::NoHandler { project, name } => {
                assert_eq!(project, "alice/pkg.warehouse");
                assert_eq!(name, "unread");
            }
            other => panic!("expected no handler, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_input_does_not_call_the_handler() {
        let fixture = Fixture::new();
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        let registry = read_registry(std::sync::Arc::clone(&ran));
        let listed: std::sync::Arc<dyn SecretStore> = std::sync::Arc::new(ListOnly {
            names: Vec::new(),
            allow_list: false,
        });
        let outcome = run(
            &fixture,
            &registry,
            listed,
            "sf_east:read",
            json!({"extra": true}),
        )
        .await;
        let Dispatch::Invalid(report) = outcome else {
            panic!("expected invalid input, got {outcome:?}");
        };
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|d| d.kind.as_str())
                .collect::<Vec<_>>(),
            vec![kind::UNKNOWN_FIELD]
        );
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn handler_is_keyed_by_project_type_and_name() {
        let fixture = Fixture::new();
        let registry = read_registry(std::sync::Arc::new(AtomicBool::new(false)));
        fixture
            .secrets
            .put(DATASET, "sf_east:token", "east-token")
            .unwrap();
        LakeVariableStore::new(std::sync::Arc::clone(&fixture.data))
            .set(DATASET, "sf_east:lane", "1")
            .unwrap();
        let outcome = run(
            &fixture,
            &registry,
            fixture.secrets.clone(),
            "sf_east:read",
            json!({}),
        )
        .await;
        let Dispatch::Done(body) = outcome else {
            panic!("expected the warehouse handler, got {outcome:?}");
        };
        assert_eq!(body["integration"], "sf_east");
        assert!(body.get("which").is_none(), "{body}");
    }
}
