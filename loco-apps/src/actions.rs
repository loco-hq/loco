//! Declared actions: a handler registry and the run that sits in front of it.
//!
//! A handler is registered in code against the project that owns the
//! declaration and the action's bare name. A same-named declaration in
//! another project does not bind it. The production binary registers nothing
//! (`AppOptions::default`); the Hurl fixture handler lives in the test runner.
//!
//! The lake has no transactions. A handler that fails after it has written
//! leaves those writes in place. Handlers must be safe to re-run, and a
//! handler error should say what was already written. [`ActionFailure`] is
//! how that error becomes 400, 409, or 500.

use std::collections::HashMap;
use std::sync::Arc;

use loco_lake::{DataAdapter, Value};
use serde_json::Map;

use crate::auth::AuthUser;
use crate::http::version_schema::VersionSchema;
use crate::validation::{validate_action_input, Diagnostic, ValidationReport};

/// Why a handler refused to finish. The HTTP layer maps these to 400, 409,
/// and 500. `diagnostics` uses the same kinds as `/data` when the handler has
/// them; an empty list is omitted from the body.
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
            | Self::Failed { message, .. } => message,
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::BadInput { diagnostics, .. }
            | Self::Conflict { diagnostics, .. }
            | Self::Failed { diagnostics, .. } => diagnostics,
        }
    }
}

/// What a handler is allowed to see. Secrets, variables, and outbound HTTP
/// are a later issue: they are not on this context.
pub struct ActionContext<'a> {
    pub dataset_id: &'a str,
    pub data: &'a dyn DataAdapter,
    pub schema: &'a VersionSchema,
    pub caller: &'a AuthUser,
    pub input: &'a HashMap<String, Value>,
}

type ActionHandler = Arc<
    dyn for<'a> Fn(&'a ActionContext<'a>) -> Result<serde_json::Value, ActionFailure> + Send + Sync,
>;

/// Handlers keyed by `(owning project, bare action name)`.
#[derive(Clone, Default)]
pub struct HandlerRegistry {
    handlers: HashMap<(String, String), ActionHandler>,
}

impl HandlerRegistry {
    pub fn register(
        &mut self,
        project: impl Into<String>,
        name: impl Into<String>,
        handler: impl for<'a> Fn(&'a ActionContext<'a>) -> Result<serde_json::Value, ActionFailure>
            + Send
            + Sync
            + 'static,
    ) {
        self.handlers
            .insert((project.into(), name.into()), Arc::new(handler));
    }

    pub fn get(&self, project: &str, name: &str) -> Option<&ActionHandler> {
        self.handlers.get(&(project.to_string(), name.to_string()))
    }
}

/// The result of resolving, validating, and running one action. Auth stays in
/// the HTTP layer: this runs only after the caller is allowed to write.
#[derive(Debug)]
pub enum Dispatch {
    NotFound,
    Invalid(ValidationReport),
    NoHandler { project: String, name: String },
    Done(serde_json::Value),
    Failed(ActionFailure),
}

/// Resolve `name` in `schema`, validate `input` with the `/data` checker, and
/// call the handler registered for the action's owning project and bare name.
/// An invalid input does not call the handler.
pub fn dispatch(
    schema: &VersionSchema,
    registry: &HandlerRegistry,
    dataset_id: &str,
    data: &dyn DataAdapter,
    caller: &AuthUser,
    name: &str,
    input: &Map<String, serde_json::Value>,
) -> Dispatch {
    let Some(action) = schema.action(name) else {
        return Dispatch::NotFound;
    };
    let input = match validate_action_input(schema, action.project(), action.name(), input) {
        Ok(input) => input,
        Err(report) => return Dispatch::Invalid(report),
    };
    let Some(handler) = registry.get(action.project(), action.name()) else {
        return Dispatch::NoHandler {
            project: action.project().to_string(),
            name: action.name().to_string(),
        };
    };
    let ctx = ActionContext {
        dataset_id,
        data,
        schema,
        caller,
        input: &input,
    };
    match handler(&ctx) {
        Ok(value) => Dispatch::Done(value),
        Err(failure) => Dispatch::Failed(failure),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use loco_lake::InMemoryAdapter;
    use serde_json::json;

    use super::*;
    use crate::validation::kind;
    use crate::{Action, ActionParam, Manifest, SchemaStore};

    const PROJECT: &str = "ben/crm";
    const VERSION: &str = "0.0.1-dev";

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
            ))
            .unwrap();
        store
            .action_params()
            .create(ActionParam {
                project: PROJECT.into(),
                version: VERSION.into(),
                action: "echo".into(),
                name: "qty".into(),
                r#type: "integer".into(),
                required: true,
                ..ActionParam::default()
            })
            .unwrap();
        store
            .action_params()
            .create(ActionParam {
                project: PROJECT.into(),
                version: VERSION.into(),
                action: "echo".into(),
                name: "size".into(),
                r#type: "string".into(),
                required: true,
                options: vec![
                    crate::ActionParamOption {
                        value: "s".into(),
                        label: "Small".into(),
                    },
                    crate::ActionParamOption {
                        value: "m".into(),
                        label: "Medium".into(),
                    },
                ],
                ..ActionParam::default()
            })
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
            echo_flag.store(true, Ordering::SeqCst);
            Ok(json!({ "ran": true }))
        });
        let decoy_flag = Arc::clone(&decoy);
        registry.register("alice/other", "echo", move |_| {
            decoy_flag.store(true, Ordering::SeqCst);
            Ok(json!({ "ran": true }))
        });
        Ran {
            echo,
            decoy,
            registry,
        }
    }

    fn run(schema: &VersionSchema, ran: &Ran, name: &str, input: serde_json::Value) -> Dispatch {
        ran.echo.store(false, Ordering::SeqCst);
        ran.decoy.store(false, Ordering::SeqCst);
        let data = InMemoryAdapter::new();
        let input = input.as_object().cloned().unwrap_or_default();
        dispatch(
            schema,
            &ran.registry,
            "ben/crm/dev",
            &data,
            &caller(),
            name,
            &input,
        )
    }

    fn kinds(outcome: &Dispatch) -> Vec<&str> {
        let Dispatch::Invalid(report) = outcome else {
            panic!("expected a validation error");
        };
        report.diagnostics.iter().map(|d| d.kind.as_str()).collect()
    }

    #[test]
    fn invalid_input_does_not_call_the_handler() {
        let (_dir, schema) = schema();
        let ran = registry();

        let missing = run(&schema, &ran, "echo", json!({"qty": 1}));
        assert_eq!(kinds(&missing), vec![kind::REQUIRED]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let wrong = run(&schema, &ran, "echo", json!({"qty": "two", "size": "m"}));
        assert_eq!(kinds(&wrong), vec![kind::TYPE_MISMATCH]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let option = run(&schema, &ran, "echo", json!({"qty": 1, "size": "xl"}));
        assert_eq!(kinds(&option), vec![kind::INVALID_OPTION]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let unknown = run(
            &schema,
            &ran,
            "echo",
            json!({"qty": 1, "size": "m", "extra": true}),
        );
        assert_eq!(kinds(&unknown), vec![kind::UNKNOWN_FIELD]);
        assert!(!ran.echo.load(Ordering::SeqCst));

        let nested = run(&schema, &ran, "echo", json!({"qty": {"n": 1}, "size": "m"}));
        assert_eq!(kinds(&nested), vec![kind::TYPE_MISMATCH]);
        assert!(!ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));
    }

    #[test]
    fn lookup_is_owning_project_and_bare_name() {
        let (_dir, schema) = schema();
        let ran = registry();

        let outcome = run(&schema, &ran, "echo", json!({"qty": 2, "size": "m"}));
        assert!(matches!(outcome, Dispatch::Done(_)));
        assert!(ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));

        let outcome = run(&schema, &ran, "noop", json!({}));
        match outcome {
            Dispatch::NoHandler { project, name } => {
                assert_eq!(project, "ben/crm");
                assert_eq!(name, "noop");
            }
            other => panic!("expected no handler, got a different outcome: {other:?}"),
        }
        assert!(!ran.echo.load(Ordering::SeqCst));
        assert!(!ran.decoy.load(Ordering::SeqCst));

        let outcome = run(&schema, &ran, "alice/other.echo", json!({}));
        assert!(matches!(outcome, Dispatch::NotFound));
        assert!(!ran.decoy.load(Ordering::SeqCst));
    }
}
