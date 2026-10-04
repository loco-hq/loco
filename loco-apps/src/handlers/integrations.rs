//! `/schema` routes for integration types and integrations.
//!
//! Standard collections, their fields, and the type's actions are lists on
//! the type document. Custom collections and their fields are a list on the
//! integration document. A published version is refused before a secret body
//! is read, the same as `/schema/.../secret`.

use std::sync::Arc;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::http::response::{error_response, version_schema_error_to_response, ApiResponse};
use crate::http::scope::{VersionReadScope, VersionScope};
use crate::http::version_schema::{reject_integration_secret_values, VersionSchemaError};
use crate::server::AppState;
use crate::{Integration, IntegrationType, IntegrationTypeUpdate, IntegrationUpdate};

pub fn routes() -> Router<Arc<AppState>> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            "/{user}/{project}/{version}/integration_type",
            post(create_integration_type),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/list",
            get(list_integration_types),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{name}",
            get(get_integration_type)
                .put(update_integration_type)
                .delete(delete_integration_type),
        )
        .route(
            "/{user}/{project}/{version}/integration",
            post(create_integration),
        )
        .route(
            "/{user}/{project}/{version}/integration/list",
            get(list_integrations),
        )
        .route(
            "/{user}/{project}/{version}/integration/{name}",
            get(get_integration)
                .put(update_integration)
                .delete(delete_integration),
        )
}

fn integration_type_from_body<T: DeserializeOwned>(
    scope: &VersionScope,
    body: serde_json::Value,
) -> Result<T, Response> {
    scope
        .schema
        .require_writable()
        .map_err(version_schema_error_to_response)?;
    reject_integration_secret_values(&body).map_err(version_schema_error_to_response)?;
    serde_json::from_value(body)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))
}

fn created<T: Serialize>(result: Result<T, VersionSchemaError>) -> Response {
    match result {
        Ok(value) => (StatusCode::CREATED, ApiResponse::success(value)).into_response(),
        Err(err) => version_schema_error_to_response(err),
    }
}

fn updated<T: Serialize>(result: Result<T, VersionSchemaError>) -> Response {
    match result {
        Ok(value) => ApiResponse::success(value).into_response(),
        Err(err) => version_schema_error_to_response(err),
    }
}

fn removed(result: Result<(), VersionSchemaError>) -> Response {
    match result {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(err) => version_schema_error_to_response(err),
    }
}

fn found<T: Serialize>(value: Option<T>, kind: &str, name: &str) -> Response {
    match value {
        Some(value) => ApiResponse::success(value).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("{kind} not found: {name}")),
    }
}

fn listed<T: Serialize>(value: T) -> Response {
    ApiResponse::success(value).into_response()
}

pub async fn create_integration_type(
    scope: VersionScope,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let input: IntegrationType = match integration_type_from_body(&scope, body) {
        Ok(input) => input,
        Err(response) => return response,
    };
    created(scope.schema.create_integration_type(input))
}

pub async fn list_integration_types(scope: VersionReadScope) -> Response {
    listed(scope.schema.integration_types())
}

pub async fn get_integration_type(
    scope: VersionReadScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    found(
        scope.schema.integration_type(&name),
        "integration type",
        &name,
    )
}

pub async fn update_integration_type(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let patch: IntegrationTypeUpdate = match integration_type_from_body(&scope, body) {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    updated(scope.schema.update_integration_type(&name, patch))
}

pub async fn delete_integration_type(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    removed(scope.schema.delete_integration_type(&name))
}

pub async fn create_integration(scope: VersionScope, Json(input): Json<Integration>) -> Response {
    created(scope.schema.create_integration(input))
}

pub async fn list_integrations(scope: VersionReadScope) -> Response {
    listed(scope.schema.integrations())
}

pub async fn get_integration(
    scope: VersionReadScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    found(scope.schema.integration(&name), "integration", &name)
}

pub async fn update_integration(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
    Json(patch): Json<IntegrationUpdate>,
) -> Response {
    updated(scope.schema.update_integration(&name, patch))
}

pub async fn delete_integration(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    removed(scope.schema.delete_integration(&name))
}
