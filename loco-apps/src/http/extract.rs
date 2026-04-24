use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;

use crate::auth::{AuthSession, AuthenticatedUser};
use crate::http::authz::{
    authorize_user, require_config_site, require_draft, validate_collection, validate_project,
};
use crate::http::paths::{collection_key, resolve_dataset_id};
use crate::http::response::error_response;
use crate::server::AppState;

/// X-Site-Id header (or `?site=` query param for browser testing).
pub struct SiteId(pub String);

impl FromRequestParts<Arc<AppState>> for SiteId {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let site_id = parts
            .headers
            .get("x-site-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        let site_id = site_id.or_else(|| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .find(|(k, _)| *k == "site")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| !v.is_empty())
            })
        });

        match site_id {
            Some(id) => Ok(SiteId(id)),
            None => Err(error_response(
                StatusCode::BAD_REQUEST,
                "missing site: use X-Site-Id header or ?site= query param",
            )),
        }
    }
}

async fn path_params(
    parts: &mut Parts,
    state: &Arc<AppState>,
) -> Result<HashMap<String, String>, Response> {
    let Path(params): Path<HashMap<String, String>> = Path::from_request_parts(parts, state)
        .await
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, &e.to_string()))?;
    Ok(params)
}

/// Authenticated + authorized context for `/schema/{user}/{project}/{version}/...` routes.
///
/// Runs (in order): authenticate → `require_config_site` → `authorize_user(user)` →
/// `validate_project(user, project)`. Mutating handlers should call [`SchemaAuth::require_draft`].
pub struct SchemaAuth {
    pub auth: AuthSession,
    pub user: String,
    pub project: String,
    pub version: String,
}

impl SchemaAuth {
    pub fn require_draft(&self) -> Result<(), Response> {
        require_draft(&self.version)
    }
}

impl FromRequestParts<Arc<AppState>> for SchemaAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(auth) = AuthenticatedUser::from_request_parts(parts, state).await?;
        require_config_site(&auth.user)?;

        let params = path_params(parts, state).await?;
        let user = params.get("user").cloned().unwrap_or_default();
        let project = params.get("project").cloned().unwrap_or_default();
        let version = params.get("version").cloned().unwrap_or_default();

        authorize_user(&auth.user, &user)?;
        validate_project(&state.schema, &user, &project)?;

        Ok(SchemaAuth {
            auth,
            user,
            project,
            version,
        })
    }
}

/// Authenticated + authorized context for `/config/{get|create|update|delete}/{*path}` routes.
///
/// Parses the wildcard `{*path}` as `"<type_name>/<id>"`. Runs: authenticate →
/// `require_config_site` → `authorize_user` (when the id starts with a username).
pub struct ConfigAuth {
    pub auth: AuthSession,
    pub type_name: String,
    pub id: String,
}

impl FromRequestParts<Arc<AppState>> for ConfigAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthenticatedUser(auth) = AuthenticatedUser::from_request_parts(parts, state).await?;
        require_config_site(&auth.user)?;

        let params = path_params(parts, state).await?;
        let path = params.get("path").cloned().unwrap_or_default();

        let Some((type_name, id)) = path.split_once('/') else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "invalid config path",
            ));
        };
        if type_name.is_empty() || id.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "invalid config path",
            ));
        }

        if let Some(path_user) = id.split('/').next() {
            authorize_user(&auth.user, path_user)?;
        }

        Ok(ConfigAuth {
            auth,
            type_name: type_name.to_string(),
            id: id.to_string(),
        })
    }
}

/// Resolved record-level context for `/{user}/{project}/collection/{name}/...` routes.
///
/// Runs: `validate_collection` → `resolve_dataset_id` (via [`SiteId`]). Supplies the computed
/// `collection_key` and `dataset_id` the lake adapter needs.
pub struct DataScope {
    pub collection_key: String,
    pub dataset_id: String,
}

impl FromRequestParts<Arc<AppState>> for DataScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let params = path_params(parts, state).await?;
        let user = params.get("user").cloned().unwrap_or_default();
        let project = params.get("project").cloned().unwrap_or_default();
        let name = params.get("name").cloned().unwrap_or_default();

        let SiteId(site_id) = SiteId::from_request_parts(parts, state).await?;

        validate_collection(&state.schema, &user, &project, &name)?;

        let namespace = format!("{user}/{project}");
        let dataset_id = resolve_dataset_id(&state.schema, &namespace, &site_id)
            .map_err(|msg| error_response(StatusCode::BAD_REQUEST, &msg))?;

        Ok(DataScope {
            collection_key: collection_key(&user, &project, &name),
            dataset_id,
        })
    }
}
