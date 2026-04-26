use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;

use loco_lake::{InsertRequest, UpdatePatch, Value};

use crate::http::response::{ApiResponse, error_response, lake_error_to_response};
use crate::http::scope::CollectionScope;
use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete as route_delete, get as route_get, post, put};
    Router::new()
        .route("/{name}/add", post(add))
        .route("/{name}/list", route_get(list))
        .route("/{name}/get/{id}", route_get(get))
        .route("/{name}/update/{id}", put(update))
        .route("/{name}/delete/{id}", route_delete(delete))
}

#[derive(Deserialize)]
pub struct AddRecordRequest {
    fields: HashMap<String, Value>,
    #[serde(default)]
    owner: Option<String>,
}

pub async fn add(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let req = InsertRequest {
        user: body.owner.unwrap_or_default(),
        fields: body.fields,
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
        Ok(records) => ApiResponse::success(records).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn get(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Path((_, id)): Path<(String, String)>,
) -> Response {
    match state
        .data_adapter
        .get(&scope.dataset_id(), &scope.collection_key, &id)
    {
        Ok(Some(record)) => ApiResponse::success(record).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn delete(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Path((_, id)): Path<(String, String)>,
) -> Response {
    match state
        .data_adapter
        .delete(&scope.dataset_id(), &scope.collection_key, &id)
    {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn update(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Path((_, id)): Path<(String, String)>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let patch = UpdatePatch {
        user: body.owner.unwrap_or_default(),
        fields: body.fields,
    };
    match state
        .data_adapter
        .update(&scope.dataset_id(), &scope.collection_key, &id, patch)
    {
        Ok(rec) => ApiResponse::success(rec).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}
