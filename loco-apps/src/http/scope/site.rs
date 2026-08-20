use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;

use crate::auth::{AuthenticatedUser, AuthSession, AuthUser, PUBLIC_USERNAME};
use crate::http::authz::validate_collection;
use crate::http::paths::collection_key;
use crate::http::response::error_response;
use crate::http::version_schema::VersionSchema;
use crate::server::AppState;
use crate::Site;

use super::helpers::{read_project_id, read_site_id};
use super::project::ProjectScope;

/// Sites whose authenticated users are allowed to edit versioned metadata via
/// /schema routes. Until permission sets land, this is a flat allowlist of
/// fully-qualified site ids (`{user}/{project}/{site_name}`).
const METADATA_EDITOR_SITES: &[&str] = &["loco/studio/studio", "loco/cards/cards"];

pub struct SiteScope {
    pub project: ProjectScope,
    pub site: Arc<Site>,
    /// Always populated — synthesized as `AuthSession::public()` when no
    /// token is provided. So every scope has a principal.
    pub auth: AuthSession,
    /// Read-only scoped schema view — bounded by this site's project+version
    /// plus its installed dependencies. Use this instead of `state.schema`
    /// from handlers under `/data` so they can't reach metadata for
    /// unrelated projects or non-installed versions, and can't mutate
    /// metadata at all (writes go through `VersionScope`).
    pub schema: VersionSchema,
}

impl SiteScope {
    /// `{account}/{project}/{site_name}` — the site named by request headers,
    /// not the logged-in identity.
    pub fn qualified_site_id(&self) -> String {
        format!("{}/{}", self.project.project_id(), self.site.name())
    }

    pub fn user(&self) -> &AuthUser {
        &self.auth.user
    }

    pub fn dataset_id(&self) -> String {
        let ds = self.site.dataset();
        let ds = if ds.is_empty() { self.site.name() } else { ds };
        format!("{}/{}", self.project.project_id(), ds)
    }

    /// Lake key for a collection inside this site's project.
    pub fn collection_key(&self, name: &str) -> String {
        collection_key(&self.project.user, &self.project.project, name)
    }

    /// Returns the validated `collection_key` if the collection exists in this
    /// site's project, otherwise an HTTP error response.
    pub fn require_collection(&self, name: &str) -> Result<String, Response> {
        validate_collection(
            &self.project.state.schema,
            &self.project.user,
            &self.project.project,
            name,
        )?;
        Ok(self.collection_key(name))
    }

    // --- Authz checks ---
    //
    // SiteScope is the single home for request-time authz. As permission
    // sets / profiles / named permissions land, they get added here so
    // every route benefits without each scope re-implementing them.

    /// Reject synthesized public sessions. Use on routes that require a
    /// real logged-in user (writes, anything mutating state).
    pub fn require_authenticated(&self) -> Result<(), Response> {
        if self.auth.user.username == PUBLIC_USERNAME {
            Err(error_response(
                StatusCode::UNAUTHORIZED,
                "authentication required",
            ))
        } else {
            Ok(())
        }
    }

    /// Reject when this site isn't on the metadata-editor allowlist. Used
    /// to gate /schema routes — only sites like `loco/studio/studio` and
    /// `loco/cards/cards` can mutate versioned metadata.
    pub fn require_metadata_editing_site(&self) -> Result<(), Response> {
        if METADATA_EDITOR_SITES.contains(&self.qualified_site_id().as_str()) {
            Ok(())
        } else {
            Err(error_response(
                StatusCode::FORBIDDEN,
                "this site does not have metadata editing permissions",
            ))
        }
    }

    /// Reject when the authed user isn't `path_user`. Used for
    /// path-targeted routes (e.g. /schema/{user}/...) so a session can
    /// only act on its own user's resources.
    pub fn require_can_edit_user(&self, path_user: &str) -> Result<(), Response> {
        if self.auth.user.username == path_user {
            Ok(())
        } else {
            Err(error_response(
                StatusCode::FORBIDDEN,
                "you do not have access to this resource",
            ))
        }
    }
}

impl FromRequestParts<Arc<AppState>> for SiteScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let (user, project) = read_project_id(parts)?;
        let site_name = read_site_id(parts)?;

        let project_id = format!("{user}/{project}");
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

        let auth = AuthenticatedUser::from_request_parts(parts, state)
            .await
            .map(|AuthenticatedUser(s)| s)
            .unwrap_or_else(|_| AuthSession::public());

        let version = site.version().to_string();
        let schema = VersionSchema::new_read_only(state.schema.clone(), &project_id, &version);

        Ok(SiteScope {
            project: ProjectScope {
                user,
                project,
                state: state.clone(),
            },
            site,
            auth,
            schema,
        })
    }
}
