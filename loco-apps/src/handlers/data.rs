use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use loco_lake::{InsertRequest, UpdatePatch, Value};

use crate::http::response::{
    error_response, lake_error_to_response, validation_error_response, ApiResponse,
};
use crate::http::scope::{CollectionScope, RecordScope};
use crate::server::AppState;
use crate::validation::ValidationMode;

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete as route_delete, get as route_get, post, put};
    Router::new()
        .route("/{name}/add", post(add))
        .route("/{name}/list", route_get(list))
        .route("/{name}/get/{id}", route_get(get))
        .route("/{name}/update/{id}", put(update))
        .route("/{name}/delete/{id}", route_delete(delete))
}

pub async fn add(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Json(fields): Json<HashMap<String, Value>>,
) -> Response {
    if let Err(resp) = scope.site.require_can_write_data() {
        return resp;
    }
    let report = scope.validate(&fields, ValidationMode::Create);
    if report.has_errors() {
        return validation_error_response(report.diagnostics);
    }

    let req = InsertRequest {
        user: scope.user().username.clone(),
        fields,
    };
    match state
        .data_adapter
        .insert(&scope.dataset_id(), &scope.collection_key, req)
    {
        Ok(rec) => (StatusCode::CREATED, ApiResponse::success(rec)).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn list(scope: CollectionScope, State(state): State<Arc<AppState>>) -> Response {
    match state
        .data_adapter
        .list(&scope.dataset_id(), &scope.collection_key)
    {
        Ok(records) => {
            let report = scope.validate_records(
                records.iter().map(|r| (r.id.as_str(), &r.fields)),
                ValidationMode::Read,
            );
            ApiResponse::success_with_diagnostics(records, report.diagnostics).into_response()
        }
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn get(scope: RecordScope, State(state): State<Arc<AppState>>) -> Response {
    match state
        .data_adapter
        .get(&scope.dataset_id(), scope.collection_key(), &scope.id)
    {
        Ok(Some(record)) => {
            let report = scope.validate(&record.fields, ValidationMode::Read);
            ApiResponse::success_with_diagnostics(record, report.diagnostics).into_response()
        }
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn delete(scope: RecordScope, State(state): State<Arc<AppState>>) -> Response {
    if let Err(resp) = scope.collection.site.require_can_write_data() {
        return resp;
    }
    match state
        .data_adapter
        .delete(&scope.dataset_id(), scope.collection_key(), &scope.id)
    {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn update(
    scope: RecordScope,
    State(state): State<Arc<AppState>>,
    Json(fields): Json<HashMap<String, Value>>,
) -> Response {
    if let Err(resp) = scope.collection.site.require_can_write_data() {
        return resp;
    }
    let report = scope.validate(&fields, ValidationMode::Update);
    if report.has_errors() {
        return validation_error_response(report.diagnostics);
    }

    let patch = UpdatePatch {
        user: scope.user().username.clone(),
        fields,
    };
    match state.data_adapter.update(
        &scope.dataset_id(),
        scope.collection_key(),
        &scope.id,
        patch,
    ) {
        Ok(rec) => ApiResponse::success(rec).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}
