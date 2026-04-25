//! Request-scoped accessors that pre-resolve "what's visible from here".
//!
//! - [`ProjectScope`]: pinned to a project; configs only (project, dataset, site).
//! - [`VersionScope`]: pinned to a (project, version); versioned metadata
//!   (collection, field, manifest).
//! - [`SiteScope`]: pinned to a site, which itself pins a version + dataset;
//!   the natural shape for data API routes and pre-auth flows.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;

use crate::auth::AuthenticatedUser;
use crate::http::authz::{require_draft, validate_collection};
use crate::http::extract::SiteId;
use crate::http::paths::collection_key;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{Project, Site};

/// Fully-qualified `{user}/{project}/{site}` ids of sites allowed to edit
/// versioned metadata via /schema routes.
const METADATA_EDITOR_SITES: &[&str] = &["loco/studio/studio", "loco/cards/cards"];

pub fn is_metadata_editor(qualified_site_id: &str) -> bool {
    METADATA_EDITOR_SITES.contains(&qualified_site_id)
}

pub fn require_metadata_editor(qualified_site_id: &str) -> Result<(), Response> {
    if is_metadata_editor(qualified_site_id) {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::FORBIDDEN,
            "this site does not have metadata editing permissions",
        ))
    }
}

pub struct ProjectScope {
    pub user: String,
    pub project: String,
    pub state: Arc<AppState>,
}

impl ProjectScope {
    pub fn project_id(&self) -> String {
        format!("{}/{}", self.user, self.project)
    }
}

pub struct VersionScope {
    pub project: ProjectScope,
    pub version: String,
}

impl VersionScope {
    pub fn user(&self) -> &str {
        &self.project.user
    }

    pub fn project_id(&self) -> String {
        self.project.project_id()
    }

    pub fn require_draft(&self) -> Result<(), Response> {
        require_draft(&self.version)
    }
}

pub struct SiteScope {
    pub project: ProjectScope,
    pub site: Arc<Site>,
}

impl SiteScope {
    /// `{user}/{project}/{site_name}` — matches `AuthUser.site_id`.
    pub fn qualified_site_id(&self) -> String {
        format!("{}/{}", self.project.project_id(), self.site.name())
    }

    pub fn dataset_id(&self) -> String {
        let ds = self.site.dataset();
        let ds = if ds.is_empty() { self.site.name() } else { ds };
        format!("{}/{}", self.project.project_id(), ds)
    }

    pub fn version_scope(&self) -> VersionScope {
        VersionScope {
            project: ProjectScope {
                user: self.project.user.clone(),
                project: self.project.project.clone(),
                state: self.project.state.clone(),
            },
            version: self.site.version().to_string(),
        }
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

// --- Extractors ---

fn read_project_id(parts: &Parts) -> Result<(String, String), Response> {
    let raw = parts
        .headers
        .get("x-project-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "missing project: use X-Project-Id header",
            )
        })?;
    let (user, project) = raw.split_once('/').ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "X-Project-Id must be in the form 'user/project'",
        )
    })?;
    Ok((user.to_string(), project.to_string()))
}

async fn read_path_params(
    parts: &mut Parts,
    state: &Arc<AppState>,
) -> Result<HashMap<String, String>, Response> {
    let Path(params): Path<HashMap<String, String>> = Path::from_request_parts(parts, state)
        .await
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, &e.to_string()))?;
    Ok(params)
}

impl FromRequestParts<Arc<AppState>> for SiteScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let (user, project) = read_project_id(parts)?;
        let SiteId(site_name) = SiteId::from_request_parts(parts, state).await?;

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

        Ok(SiteScope {
            project: ProjectScope {
                user,
                project,
                state: state.clone(),
            },
            site,
        })
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

        let params = read_path_params(parts, state).await?;
        let user = params.get("user").cloned().unwrap_or_default();
        let project = params.get("project").cloned().unwrap_or_default();
        let version = params.get("version").cloned().unwrap_or_default();

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

        Ok(VersionScope {
            project: ProjectScope {
                user,
                project,
                state: state.clone(),
            },
            version,
        })
    }
}
