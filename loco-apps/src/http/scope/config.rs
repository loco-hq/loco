use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::{AuthUser, AuthenticatedUser};
use crate::http::authz::require_developer;
use crate::http::project_config::ProjectConfig;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{Project, SchemaStore};

use super::helpers::read_path_params;

#[derive(Deserialize)]
struct ConfigProjectPathParams {
    user: String,
    project: String,
}

/// Authenticated identity plus a `ProjectConfig` pinned to the
/// `{user}/{project}` in the request path, for `/config/...` routes
/// that target an existing project.
///
/// Authz: developer (or org owner) on the path project. Site headers
/// are not required.
pub struct ConfigProjectScope {
    pub auth: AuthUser,
    pub config: ProjectConfig,
}

impl ConfigProjectScope {
    pub fn user(&self) -> &str {
        self.config.user()
    }

    pub fn project(&self) -> &str {
        self.config.project()
    }

    pub fn project_id(&self) -> String {
        self.config.project_id()
    }
}

impl FromRequestParts<Arc<AppState>> for ConfigProjectScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(session) =
            AuthenticatedUser::from_request_parts(parts, state).await?;

        let ConfigProjectPathParams { user, project } = read_path_params(parts, state).await?;

        require_developer(state, &session.user.username, &format!("{user}/{project}"))?;

        let config = ProjectConfig::new(state.schema.clone(), user, project);
        if !config.exists() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {}", config.project_id()),
            ));
        }

        Ok(ConfigProjectScope {
            auth: session.user,
            config,
        })
    }
}

/// Authenticated identity for `/config` routes that don't point at an
/// existing project — `POST /config/project`, `POST /config/org`, and
/// `GET /config/project/list`. No site headers required: capability is
/// on the member, not a magic editor site.
pub struct ConfigUserScope {
    pub user: AuthUser,
    store: Arc<SchemaStore>,
    state: Arc<AppState>,
}

impl ConfigUserScope {
    pub fn username(&self) -> &str {
        &self.user.username
    }

    /// Projects the identity can see: person-owned `{handle}/*`, org-owned
    /// projects they own, and projects they have an explicit membership on.
    pub fn list_projects(&self) -> Vec<(String, Arc<Project>)> {
        let handle = self.username();
        self.store
            .projects()
            .list_all()
            .into_iter()
            .filter(|(_, project)| {
                self.state
                    .auth_adapter
                    .project_access(handle, project.project())
                    .ok()
                    .flatten()
                    .is_some()
            })
            .collect()
    }

    /// Build a `ProjectConfig` for `(account, project)`. Caller is
    /// responsible for any existence semantics (e.g. `create_project`
    /// expects no entry; `update`/`delete` expect one).
    pub fn project_config(&self, account: &str, project: &str) -> ProjectConfig {
        ProjectConfig::new(self.store.clone(), account.to_string(), project.to_string())
    }
}

impl FromRequestParts<Arc<AppState>> for ConfigUserScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(session) =
            AuthenticatedUser::from_request_parts(parts, state).await?;
        Ok(ConfigUserScope {
            user: session.user,
            store: state.schema.clone(),
            state: state.clone(),
        })
    }
}
