use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use loco_lake::{Record, Value};

use crate::http::extract::DataScope;
use crate::http::response::{ApiResponse, error_response, lake_error_to_response};
use crate::server::AppState;

#[derive(Deserialize)]
pub struct AddRecordRequest {
    fields: HashMap<String, Value>,
    #[serde(default)]
    owner: Option<String>,
}

pub async fn add(
    scope: DataScope,
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let now = chrono::Utc::now().to_rfc3339();
    let owner = body.owner.unwrap_or_default();
    let record = Record {
        id: uuid::Uuid::new_v4().to_string(),
        dataset_id: Some(scope.dataset_id.clone()),
        created_at: now.clone(),
        created_by: owner.clone(),
        updated_at: now,
        updated_by: owner.clone(),
        owner,
        fields: body.fields,
    };

    match state.adapter.insert(&scope.dataset_id, &scope.collection_key, record) {
        Ok(rec) => (StatusCode::CREATED, ApiResponse::success(rec)).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn list(scope: DataScope, State(state): State<Arc<AppState>>) -> Response {
    match state.adapter.list(&scope.dataset_id, &scope.collection_key) {
        Ok(records) => ApiResponse::success(records).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn get(
    scope: DataScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, id)): Path<(String, String, String, String)>,
) -> Response {
    match state.adapter.get(&scope.dataset_id, &scope.collection_key, &id) {
        Ok(Some(record)) => ApiResponse::success(record).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn delete(
    scope: DataScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, id)): Path<(String, String, String, String)>,
) -> Response {
    match state.adapter.delete(&scope.dataset_id, &scope.collection_key, &id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

pub async fn update(
    scope: DataScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, id)): Path<(String, String, String, String)>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let existing = match state.adapter.get(&scope.dataset_id, &scope.collection_key, &id) {
        Ok(Some(r)) => r,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => return lake_error_to_response(e),
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut fields = existing.fields;
    for (k, v) in body.fields {
        fields.insert(k, v);
    }

    let record = Record {
        id: existing.id,
        dataset_id: existing.dataset_id,
        created_at: existing.created_at,
        created_by: existing.created_by,
        updated_at: now,
        updated_by: body.owner.unwrap_or(existing.updated_by),
        owner: existing.owner,
        fields,
    };

    match state.adapter.update(&scope.dataset_id, &scope.collection_key, &id, record) {
        Ok(rec) => ApiResponse::success(rec).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}
