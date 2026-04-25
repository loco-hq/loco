use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;

use crate::auth::{AuthSession, AuthenticatedUser};
use crate::http::authz::{authorize_user, require_config_site};
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

