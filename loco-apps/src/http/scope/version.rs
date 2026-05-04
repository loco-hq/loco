use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::http::response::error_response;
use crate::http::version_schema::VersionSchema;
use crate::server::AppState;
use crate::Project;

use super::helpers::read_path_params;
use super::project::ProjectScope;

#[derive(Deserialize)]
struct VersionPathParams {
    user: String,
    project: String,
    version: String,
}

/// Fully-qualified `{user}/{project}/{site}` ids of sites allowed to edit
/// versioned metadata via /schema routes.
const METADATA_EDITOR_SITES: &[&str] = &["loco/studio/studio", "loco/cards/cards"];

fn is_metadata_editor(qualified_site_id: &str) -> bool {
    METADATA_EDITOR_SITES.contains(&qualified_site_id)
}

fn require_metadata_editor(qualified_site_id: &str) -> Result<(), Response> {
    if is_metadata_editor(qualified_site_id) {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::FORBIDDEN,
            "this site does not have metadata editing permissions",
        ))
    }
}

pub struct VersionScope {
    pub project: ProjectScope,
    pub version: String,
    /// Scoped schema view: read across the manifest closure, write only to
    /// `(project_id, version)`. Use this instead of `state.schema` from
    /// /schema handlers.
    pub schema: VersionSchema,
}

impl VersionScope {
    pub fn user(&self) -> &str {
        &self.project.user
    }

    pub fn project_id(&self) -> String {
        self.project.project_id()
    }
}

impl FromRequestParts<Arc<AppState>> for VersionScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(auth) =
            AuthenticatedUser::from_request_parts(parts, state).await?;

        require_metadata_editor(&auth.user.site_id)?;

        let VersionPathParams {
            user,
            project,
            version,
        } = read_path_params(parts, state).await?;

        if auth.user.username != user {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "you do not have access to this resource",
            ));
        }

        let project_id = format!("{user}/{project}");
        if !state.schema.projects().has(&Project::to_path(&project_id)) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {project_id}"),
            ));
        }

        let schema = VersionSchema::new(state.schema.clone(), &project_id, &version)
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, &e.to_string()))?;

        Ok(VersionScope {
            project: ProjectScope {
                user,
                project,
                state: state.clone(),
            },
            version,
            schema,
        })
    }
}
