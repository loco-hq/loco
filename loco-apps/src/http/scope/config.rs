use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::http::response::error_response;
use crate::server::AppState;
use crate::Project;

use super::helpers::read_path_params;
use super::site::SiteScope;

#[derive(Deserialize)]
struct ConfigProjectPathParams {
    user: String,
    project: String,
}

/// A `SiteScope` plus a `{user}/{project}` pulled from the request path,
/// for `/config/...` routes that target an existing project.
///
/// Authz lives entirely on `SiteScope` — this extractor just composes:
/// authenticate, gate to metadata-editor sites, scope the path target to
/// the authed user, and confirm the project exists.
pub struct ConfigProjectScope {
    pub site: SiteScope,
    pub user: String,
    pub project: String,
}

impl ConfigProjectScope {
    pub fn project_id(&self) -> String {
        format!("{}/{}", self.user, self.project)
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

        let project_id = format!("{user}/{project}");
        if !state.schema.projects().has(&Project::to_path(&project_id)) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {project_id}"),
            ));
        }

        Ok(ConfigProjectScope {
            site,
            user,
            project,
        })
    }
}

/// A `SiteScope` for `/config/...` routes that don't (or can't) point at an
/// existing project — `POST /config/project` (the project doesn't exist
/// yet) and `GET /config/project/list` (no project in the URL at all).
///
/// Same authz chain as `ConfigProjectScope` but skips path reading and the
/// project-existence check. The target user comes from the auth session.
pub struct ConfigUserScope {
    pub site: SiteScope,
}

impl ConfigUserScope {
    pub fn username(&self) -> &str {
        &self.site.user().username
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
        Ok(ConfigUserScope { site })
    }
}
