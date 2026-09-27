use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::{session_or_public, AuthUser, AuthenticatedUser, PUBLIC_USERNAME};
use crate::http::authz::{forbidden, require_developer};
use crate::http::response::error_response;
use crate::http::version_schema::{unknown_version, VersionSchema};
use crate::server::AppState;
use crate::{Project, Site};

use super::helpers::{read_optional_site_headers, read_path_params};

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
/// Site headers are not required — capability is on the member. Used by
/// `/schema` writes; GET uses [`VersionReadScope`]. 404 when the version does
/// not exist (has no manifest), before the handler reads the body.
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

        // Before any handler reads the body: a version with no manifest was
        // never created through `/config` (#95). The write itself checks
        // again under `PINS`, since a delete can land in between.
        let schema = VersionSchema::new(state.schema.clone(), &project_id, &version);
        if !schema.exists() {
            return Err(unknown_version_response(&project_id, &version));
        }

        Ok(VersionScope {
            user: session.user,
            schema,
        })
    }
}

fn unknown_version_response(project_id: &str, version: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, &unknown_version(project_id, version))
}

/// Read-only schema view for GET `/schema` routes.
///
/// - `developer` / `editor` (or org owner) on the path project: any version,
///   no site headers. 404 when the version does not exist, as for a write.
/// - Otherwise: `X-Project-Id` + `X-Site-Id` required, must name the path
///   project, path `{version}` must equal the site pin, and the site must
///   assign at least one permission set to `public`. The whole pinned
///   version is visible (no per-collection filter).
pub struct VersionReadScope {
    pub user: AuthUser,
    pub schema: VersionSchema,
}

impl FromRequestParts<Arc<AppState>> for VersionReadScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let VersionPathParams {
            user,
            project,
            version,
        } = read_path_params(parts, state).await?;
        let project_id = format!("{user}/{project}");

        let auth = session_or_public(parts, state).await?;
        let is_public = auth.user.username == PUBLIC_USERNAME;

        if !is_public {
            match state
                .auth_adapter
                .project_access(&auth.user.username, &project_id)
            {
                Ok(Some(role)) if role.can_edit_data() => {
                    if !state.schema.projects().has(&Project::to_path(&project_id)) {
                        return Err(error_response(
                            StatusCode::NOT_FOUND,
                            &format!("unknown project: {project_id}"),
                        ));
                    }
                    let schema =
                        VersionSchema::new_read_only(state.schema.clone(), &project_id, &version);
                    // A missing version is a 404 on every `/schema` route,
                    // read or write, not an empty version.
                    if !schema.exists() {
                        return Err(unknown_version_response(&project_id, &version));
                    }
                    return Ok(VersionReadScope {
                        user: auth.user,
                        schema,
                    });
                }
                Ok(_) => {}
                Err(e) => return Err(crate::auth::auth_error_to_response(e)),
            }
        }

        let Some((header_user, header_project, site_name)) = read_optional_site_headers(parts)?
        else {
            return if is_public {
                Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "authentication required",
                ))
            } else {
                Err(forbidden())
            };
        };

        if format!("{header_user}/{header_project}") != project_id {
            return Err(forbidden());
        }

        if !state.schema.projects().has(&Project::to_path(&project_id)) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                &format!("unknown project: {project_id}"),
            ));
        }

        let site = state
            .schema
            .sites()
            .get(&Site::to_path(&project_id, &site_name))
            .ok_or_else(|| {
                error_response(
                    StatusCode::NOT_FOUND,
                    &format!("unknown site: {site_name} in project {project_id}"),
                )
            })?;

        if site.version() != version {
            return Err(forbidden());
        }

        // A pinned version with no manifest assigns nothing to `public`, so
        // it is a 403 below: this caller may not learn whether it exists.
        let schema = VersionSchema::new_read_only(state.schema.clone(), &project_id, &version);
        if schema.public_permission_sets().is_empty() {
            return Err(forbidden());
        }

        Ok(VersionReadScope {
            user: auth.user,
            schema,
        })
    }
}
