use std::sync::Arc;

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;
use serde::de::DeserializeOwned;

use crate::http::response::error_response;
use crate::server::AppState;

fn read_header(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

pub(super) fn read_project_id(parts: &Parts) -> Result<(String, String), Response> {
    let raw = read_header(parts, "x-project-id").ok_or_else(|| {
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

pub(super) fn read_site_id(parts: &Parts) -> Result<String, Response> {
    read_header(parts, "x-site-id").ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "missing site: use X-Site-Id header",
        )
    })
}

pub(super) async fn read_path_params<T>(
    parts: &mut Parts,
    state: &Arc<AppState>,
) -> Result<T, Response>
where
    T: DeserializeOwned + Send + 'static,
{
    let Path(params): Path<T> = Path::from_request_parts(parts, state)
        .await
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, &e.to_string()))?;
    Ok(params)
}
