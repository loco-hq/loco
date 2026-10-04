//! `/schema` routes for integration types and integrations.
//!
//! Standard collections, fields, and actions nest under the type. Custom
//! collections and fields nest under the integration. A published version is
//! refused before a secret body is read, the same as `/schema/.../secret`.

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
use crate::{
    Integration, IntegrationCollection, IntegrationCollectionUpdate, IntegrationField,
    IntegrationFieldUpdate, IntegrationType, IntegrationTypeUpdate, IntegrationUpdate, TypeAction,
    TypeActionUpdate, TypeCollection, TypeCollectionUpdate, TypeField, TypeFieldUpdate,
};

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
            "/{user}/{project}/{version}/integration_type/{type_name}/collection",
            post(create_type_collection),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/collection/list",
            get(list_type_collections),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/collection/{name}",
            get(get_type_collection)
                .put(update_type_collection)
                .delete(delete_type_collection),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/field",
            post(create_type_field),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/field/{collection}/list",
            get(list_type_fields),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/field/{collection}/{name}",
            axum::routing::put(update_type_field).delete(delete_type_field),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/action",
            post(create_type_action),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/action/list",
            get(list_type_actions),
        )
        .route(
            "/{user}/{project}/{version}/integration_type/{type_name}/action/{name}",
            get(get_type_action)
                .put(update_type_action)
                .delete(delete_type_action),
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
        .route(
            "/{user}/{project}/{version}/integration/{integration}/collection",
            post(create_integration_collection),
        )
        .route(
            "/{user}/{project}/{version}/integration/{integration}/collection/list",
            get(list_integration_collections),
        )
        .route(
            "/{user}/{project}/{version}/integration/{integration}/collection/{name}",
            get(get_integration_collection)
                .put(update_integration_collection)
                .delete(delete_integration_collection),
        )
        .route(
            "/{user}/{project}/{version}/integration/{integration}/field",
            post(create_integration_field),
        )
        .route(
            "/{user}/{project}/{version}/integration/{integration}/field/{collection}/list",
            get(list_integration_fields),
        )
        .route(
            "/{user}/{project}/{version}/integration/{integration}/field/{collection}/{name}",
            axum::routing::put(update_integration_field).delete(delete_integration_field),
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

pub async fn create_type_collection(
    scope: VersionScope,
    Path((_, _, _, type_name)): Path<(String, String, String, String)>,
    Json(input): Json<TypeCollection>,
) -> Response {
    created(scope.schema.create_type_collection(&type_name, input))
}

pub async fn list_type_collections(
    scope: VersionReadScope,
    Path((_, _, _, type_name)): Path<(String, String, String, String)>,
) -> Response {
    listed(scope.schema.type_collections(&type_name))
}

pub async fn get_type_collection(
    scope: VersionReadScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
) -> Response {
    found(
        scope.schema.type_collection(&type_name, &name),
        "collection",
        &name,
    )
}

pub async fn update_type_collection(
    scope: VersionScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<TypeCollectionUpdate>,
) -> Response {
    updated(
        scope
            .schema
            .update_type_collection(&type_name, &name, patch),
    )
}

pub async fn delete_type_collection(
    scope: VersionScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
) -> Response {
    removed(scope.schema.delete_type_collection(&type_name, &name))
}

pub async fn create_type_field(
    scope: VersionScope,
    Path((_, _, _, type_name)): Path<(String, String, String, String)>,
    Json(input): Json<TypeField>,
) -> Response {
    created(scope.schema.create_type_field(&type_name, input))
}

pub async fn list_type_fields(
    scope: VersionReadScope,
    Path((_, _, _, type_name, collection)): Path<(String, String, String, String, String)>,
) -> Response {
    listed(scope.schema.type_fields(&type_name, &collection))
}

pub async fn update_type_field(
    scope: VersionScope,
    Path((_, _, _, type_name, collection, name)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
    Json(patch): Json<TypeFieldUpdate>,
) -> Response {
    updated(
        scope
            .schema
            .update_type_field(&type_name, &collection, &name, patch),
    )
}

pub async fn delete_type_field(
    scope: VersionScope,
    Path((_, _, _, type_name, collection, name)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Response {
    removed(
        scope
            .schema
            .delete_type_field(&type_name, &collection, &name),
    )
}

pub async fn create_type_action(
    scope: VersionScope,
    Path((_, _, _, type_name)): Path<(String, String, String, String)>,
    Json(input): Json<TypeAction>,
) -> Response {
    created(scope.schema.create_type_action(&type_name, input))
}

pub async fn list_type_actions(
    scope: VersionReadScope,
    Path((_, _, _, type_name)): Path<(String, String, String, String)>,
) -> Response {
    listed(scope.schema.type_actions(&type_name))
}

pub async fn get_type_action(
    scope: VersionReadScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
) -> Response {
    found(scope.schema.type_action(&type_name, &name), "action", &name)
}

pub async fn update_type_action(
    scope: VersionScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<TypeActionUpdate>,
) -> Response {
    updated(scope.schema.update_type_action(&type_name, &name, patch))
}

pub async fn delete_type_action(
    scope: VersionScope,
    Path((_, _, _, type_name, name)): Path<(String, String, String, String, String)>,
) -> Response {
    removed(scope.schema.delete_type_action(&type_name, &name))
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

pub async fn create_integration_collection(
    scope: VersionScope,
    Path((_, _, _, integration)): Path<(String, String, String, String)>,
    Json(input): Json<IntegrationCollection>,
) -> Response {
    created(
        scope
            .schema
            .create_integration_collection(&integration, input),
    )
}

pub async fn list_integration_collections(
    scope: VersionReadScope,
    Path((_, _, _, integration)): Path<(String, String, String, String)>,
) -> Response {
    listed(scope.schema.integration_collections(&integration))
}

pub async fn get_integration_collection(
    scope: VersionReadScope,
    Path((_, _, _, integration, name)): Path<(String, String, String, String, String)>,
) -> Response {
    found(
        scope.schema.integration_collection(&integration, &name),
        "collection",
        &name,
    )
}

pub async fn update_integration_collection(
    scope: VersionScope,
    Path((_, _, _, integration, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<IntegrationCollectionUpdate>,
) -> Response {
    updated(
        scope
            .schema
            .update_integration_collection(&integration, &name, patch),
    )
}

pub async fn delete_integration_collection(
    scope: VersionScope,
    Path((_, _, _, integration, name)): Path<(String, String, String, String, String)>,
) -> Response {
    removed(
        scope
            .schema
            .delete_integration_collection(&integration, &name),
    )
}

pub async fn create_integration_field(
    scope: VersionScope,
    Path((_, _, _, integration)): Path<(String, String, String, String)>,
    Json(input): Json<IntegrationField>,
) -> Response {
    created(scope.schema.create_integration_field(&integration, input))
}

pub async fn list_integration_fields(
    scope: VersionReadScope,
    Path((_, _, _, integration, collection)): Path<(String, String, String, String, String)>,
) -> Response {
    listed(scope.schema.integration_fields(&integration, &collection))
}

pub async fn update_integration_field(
    scope: VersionScope,
    Path((_, _, _, integration, collection, name)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
    Json(patch): Json<IntegrationFieldUpdate>,
) -> Response {
    updated(
        scope
            .schema
            .update_integration_field(&integration, &collection, &name, patch),
    )
}

pub async fn delete_integration_field(
    scope: VersionScope,
    Path((_, _, _, integration, collection, name)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
) -> Response {
    removed(
        scope
            .schema
            .delete_integration_field(&integration, &collection, &name),
    )
}
