use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use loco_lake::{InsertRequest, UpdatePatch, Value};
use serde_json::{json, Map};

use crate::http::response::{
    error_response, lake_error_to_response, validation_error_response, ApiResponse,
};
use crate::http::scope::{CollectionScope, RecordScope, SiteScope};
use crate::query::{self as q, Plan};
use crate::server::AppState;
use crate::validation::ValidationMode;

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete as route_delete, get as route_get, post, put};
    Router::new()
        .route("/query", post(query))
        .route("/{name}/add", post(add))
        .route("/{name}/list", route_get(list))
        .route("/{name}/fields", route_get(fields))
        .route("/{name}/get/{id}", route_get(get))
        .route("/{name}/update/{id}", put(update))
        .route("/{name}/delete/{id}", route_delete(delete))
}

pub async fn add(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Json(fields): Json<HashMap<String, Value>>,
) -> Response {
    if let Err(resp) = scope.require_can_create_data() {
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
    if let Err(resp) = scope.require_can_read_data() {
        return resp;
    }
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

/// The collection's fields in the site's pinned version — the same list, order,
/// and shape as `/schema/{user}/{project}/{version}/field/{collection}/list`,
/// but the version comes from the site, so a hosted frontend never has to be
/// told which version it runs on. Readable by whoever may read the records.
pub async fn fields(scope: CollectionScope) -> Response {
    if let Err(resp) = scope.require_can_read_data() {
        return resp;
    }
    let fields = scope
        .site
        .schema
        .fields_of(&scope.collection_project, &scope.collection_name);
    ApiResponse::success(fields).into_response()
}

pub async fn get(scope: RecordScope, State(state): State<Arc<AppState>>) -> Response {
    if let Err(resp) = scope.collection.require_can_read_data() {
        return resp;
    }
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
    if let Err(resp) = scope.collection.require_can_delete_data() {
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
    if let Err(resp) = scope.collection.require_can_update_data() {
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

/// `POST /data/query` — a batch of named read queries (`docs/query.md`).
///
/// Each query is resolved, type-checked, and authorized on its own; one that
/// fails gets an error result and the rest still run. Every runnable query
/// goes to the lake in one call, so the batch reads one snapshot. The status
/// is 400 only when the body is not a batch at all.
pub async fn query(
    scope: SiteScope,
    State(state): State<Arc<AppState>>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.body_text()),
    };
    let queries = match q::parse_batch(&body) {
        Ok(queries) => queries,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };

    let mut results = Map::new();
    let mut runnable: Vec<(String, Plan)> = Vec::new();
    for (name, raw) in queries {
        match plan_one(&scope, &name, &raw) {
            Ok(Ok(plan)) => runnable.push((name, plan)),
            Ok(Err(diagnostics)) => {
                results.insert(
                    name,
                    json!({ "error": "query failed", "diagnostics": diagnostics }),
                );
            }
            Err(resp) => return resp,
        }
    }

    let mut diagnostics = Vec::new();
    if !runnable.is_empty() {
        let lake: Vec<_> = runnable.iter().map(|(_, p)| p.lake.clone()).collect();
        let pages = match state.data_adapter.query(&scope.dataset_id(), &lake) {
            Ok(pages) => pages,
            Err(e) => return lake_error_to_response(e),
        };
        for ((name, plan), page) in runnable.into_iter().zip(pages) {
            diagnostics.extend(q::record_diagnostics(
                &scope.schema,
                &name,
                &plan.target.project,
                &plan.target.name,
                &page.records,
                plan.lake.fields.as_deref(),
            ));
            let cursor = page.next.map(|next| q::encode_cursor(&plan.hash, &next));
            results.insert(name, json!({ "records": page.records, "cursor": cursor }));
        }
    }

    ApiResponse::success_with_diagnostics(results, diagnostics).into_response()
}

/// One query → a plan, or the diagnostics that stop it. The outer `Err` is a
/// site-level failure (the membership lookup), which fails the whole request.
fn plan_one(
    scope: &SiteScope,
    name: &str,
    raw: &serde_json::Value,
) -> Result<Result<Plan, Vec<q::Diagnostic>>, Response> {
    let target = match q::target(&scope.schema, name, raw) {
        Ok(target) => target,
        Err(d) => return Ok(Err(vec![d])),
    };
    if !scope.may_read_collection(&target.name, &target.project)? {
        return Ok(Err(vec![q::Diagnostic::error(
            q::kind::FORBIDDEN,
            Some(name.to_string()),
            format!("no read grant on {}", target.key()),
        )]));
    }
    Ok(q::plan(&scope.schema, name, raw, target))
}
