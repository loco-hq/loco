use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::http::project_config::ProjectConfig;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{Project, SchemaStore};

use super::helpers::read_path_params;
use super::site::SiteScope;

#[derive(Deserialize)]
struct ConfigProjectPathParams {
    user: String,
    project: String,
}

/// A `SiteScope` plus a `ProjectConfig` pinned to the `{user}/{project}`
/// in the request path, for `/config/...` routes that target an existing
/// project.
///
/// Authz lives entirely on `SiteScope` — this extractor composes it
/// (authenticate, gate to metadata-editor sites, scope the path target to
/// the authed user) and verifies the project exists before handing the
/// `ProjectConfig` to the handler.
pub struct ConfigProjectScope {
    pub site: SiteScope,
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
        let site = SiteScope::from_request_parts(parts, state).await?;
        site.require_authenticated()?;
        site.require_metadata_editing_site()?;

        let ConfigProjectPathParams { user, project } = read_path_params(parts, state).await?;

        site.require_can_edit_user(&user)?;

        let config = ProjectConfig::new(state.schema.clone(), user, project);
        if !config.exists() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {}", config.project_id()),
            ));
        }

        Ok(ConfigProjectScope { site, config })
    }
}

/// A `SiteScope` for `/config/...` routes that don't (or can't) point at an
/// existing project — `POST /config/project` (the project doesn't exist
/// yet) and `GET /config/project/list` (no project in the URL at all).
///
/// Holds the schema store privately so handlers can list user projects or
/// build a `ProjectConfig` without reaching into `state.schema`.
pub struct ConfigUserScope {
    pub site: SiteScope,
    store: Arc<SchemaStore>,
}

impl ConfigUserScope {
    pub fn username(&self) -> &str {
        &self.site.user().username
    }

    /// Projects owned by the authenticated user.
    pub fn list_projects(&self) -> Vec<(String, Arc<Project>)> {
        let prefix = format!("{}/", self.username());
        self.store.projects().list(&prefix)
    }

    /// Build a `ProjectConfig` for `(auth_user, project)`. Caller is
    /// responsible for any existence semantics (e.g. `create_project`
    /// expects no entry; `update`/`delete` expect one).
    pub fn project_config(&self, project: &str) -> ProjectConfig {
        ProjectConfig::new(self.store.clone(), self.username().to_string(), project.to_string())
    }
}

impl FromRequestParts<Arc<AppState>> for ConfigUserScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let site = SiteScope::from_request_parts(parts, state).await?;
        site.require_authenticated()?;
        site.require_metadata_editing_site()?;
        Ok(ConfigUserScope {
            site,
            store: state.schema.clone(),
        })
    }
}

