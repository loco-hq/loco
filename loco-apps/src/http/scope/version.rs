use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::http::response::error_response;
use crate::http::version_schema::VersionSchema;
use crate::server::AppState;
use crate::Project;

use super::helpers::read_path_params;
use super::site::SiteScope;

#[derive(Deserialize)]
struct VersionPathParams {
    user: String,
    project: String,
    version: String,
}

/// A `SiteScope` (the editor session, identified by X-Project-Id /
/// X-Site-Id headers) plus a writable `VersionSchema` for the
/// `{user}/{project}/{version}` triple in the request path.
///
/// Authz lives entirely on `SiteScope` — this extractor just composes:
/// authenticate, gate to metadata-editor sites, scope the path target to
/// the authed user, and resolve the schema view.
pub struct VersionScope {
    pub site: SiteScope,
    /// Writable schema view for the path-extracted `(user/project, version)`.
    /// Writes are still gated on the version being a draft; see
    /// `VersionSchema::require_writable`.
    pub schema: VersionSchema,
}

impl FromRequestParts<Arc<AppState>> for VersionScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let site = SiteScope::from_request_parts(parts, state).await?;
        site.require_authenticated()?;
        site.require_metadata_editing_site()?;

        let VersionPathParams {
            user,
            project,
            version,
        } = read_path_params(parts, state).await?;

        site.require_can_edit_user(&user)?;

        let project_id = format!("{user}/{project}");
        if !state.schema.projects().has(&Project::to_path(&project_id)) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {project_id}"),
            ));
        }

        let schema = VersionSchema::new(state.schema.clone(), &project_id, &version);

        Ok(VersionScope { site, schema })
    }
}
