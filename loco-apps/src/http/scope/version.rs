use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::{AuthUser, AuthenticatedUser};
use crate::http::authz::require_developer;
use crate::http::response::error_response;
use crate::http::version_schema::VersionSchema;
use crate::server::AppState;
use crate::Project;

use super::helpers::read_path_params;

#[derive(Deserialize)]
struct VersionPathParams {
    user: String,
    project: String,
    version: String,
}

/// Authenticated identity plus a writable `VersionSchema` for the
/// `{user}/{project}/{version}` triple in the request path.
///
/// Authz is membership on the path project (developer or org owner).
/// Site headers are not required — capability is on the member.
pub struct VersionScope {
    pub user: AuthUser,
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
        let AuthenticatedUser(session) =
            AuthenticatedUser::from_request_parts(parts, state).await?;

        let VersionPathParams {
            user,
            project,
            version,
        } = read_path_params(parts, state).await?;

        require_developer(state, &session.user.username, &format!("{user}/{project}"))?;

        let project_id = format!("{user}/{project}");
        if !state.schema.projects().has(&Project::to_path(&project_id)) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {project_id}"),
            ));
        }

        let schema = VersionSchema::new(state.schema.clone(), &project_id, &version);

        Ok(VersionScope {
            user: session.user,
            schema,
        })
    }
}
