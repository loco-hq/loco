use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;

use crate::auth::{AuthenticatedUser, AuthSession, AuthUser};
use crate::http::authz::validate_collection;
use crate::http::paths::collection_key;
use crate::http::response::error_response;
use crate::http::site_schema::SiteSchema;
use crate::server::AppState;
use crate::Site;

use super::helpers::{read_project_id, read_site_id};
use super::project::ProjectScope;

pub struct SiteScope {
    pub project: ProjectScope,
    pub site: Arc<Site>,
    /// Always populated — synthesized as `AuthSession::public(...)` when no
    /// token is provided. So every scope has a user.
    pub auth: AuthSession,
    /// Scoped schema view — bounded by this site's project+version plus its
    /// installed dependencies. Use this instead of `state.schema` from
    /// handlers under `/data` so they can't reach metadata for unrelated
    /// projects or non-installed versions.
    pub schema: SiteSchema,
}

impl SiteScope {
    /// `{user}/{project}/{site_name}` — matches `AuthUser.site_id`.
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

        let qualified = format!("{project_id}/{site_name}");
        let auth = AuthenticatedUser::from_request_parts(parts, state)
            .await
            .map(|AuthenticatedUser(s)| s)
            .unwrap_or_else(|_| AuthSession::public(&qualified));

        let version = site.version().to_string();
        let schema = SiteSchema::new(state.schema.clone(), &project_id, &version)
            .map_err(|e| error_response(StatusCode::BAD_REQUEST, &e.to_string()))?;

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
