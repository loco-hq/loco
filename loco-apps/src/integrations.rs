//! Registries for integration types.
//!
//! A source is keyed by the owning project and the type name. A type action
//! is keyed by the owning project, the type name, and the action name.
//! Ordinary actions stay [`crate::actions::HandlerRegistry`], keyed by
//! `(project, name)` only.
//!
//! Both registries are empty in the production binary. There is no source
//! trait yet: [`RegisteredSource`] is a placeholder until `CollectionSource`
//! exists. A resolved type action is 501. Dispatch waits until a handler
//! receives the connection it was called for, rather than the type project's
//! loose secrets.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::actions::{ActionContext, ActionFailure};

/// Placeholder registered for one integration type.
///
/// `CollectionSource` is not this issue. The map exists so a later trait
/// object can take this slot without a second key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisteredSource;

/// Sources keyed by `(owning project, type name)`.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    sources: HashMap<(String, String), RegisteredSource>,
}

impl SourceRegistry {
    /// Record that `project`'s integration type `type_name` has a source.
    pub fn register(&mut self, project: impl Into<String>, type_name: impl Into<String>) {
        self.sources
            .insert((project.into(), type_name.into()), RegisteredSource);
    }

    pub fn contains(&self, project: &str, type_name: &str) -> bool {
        self.sources
            .contains_key(&(project.to_string(), type_name.to_string()))
    }
}

type TypeActionFuture =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, ActionFailure>> + Send>>;

trait TypeActionHandler: Send + Sync {
    /// Stored until a handler receives the connection it was called for.
    /// A resolved type action is 501 and does not call this.
    #[allow(dead_code)]
    fn call(&self, ctx: ActionContext) -> TypeActionFuture;
}

impl<F, Fut> TypeActionHandler for F
where
    F: Fn(ActionContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<serde_json::Value, ActionFailure>> + Send + 'static,
{
    fn call(&self, ctx: ActionContext) -> TypeActionFuture {
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
    /// A same-named action on another type, or an ordinary action of the
    /// same name, does not call it.
    pub fn register<F, Fut>(
        &mut self,
        project: impl Into<String>,
        type_name: impl Into<String>,
        name: impl Into<String>,
        handler: F,
    ) where
        F: Fn(ActionContext) -> Fut + Send + Sync + 'static,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registries_key_by_type_and_start_empty() {
        let sources = SourceRegistry::default();
        assert!(!sources.contains("alice/pkg", "bricklink"));

        let mut sources = SourceRegistry::default();
        sources.register("alice/pkg", "bricklink");
        sources.register("alice/pkg", "warehouse");
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
}
