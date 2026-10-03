//! `/config` routes for dataset secret and variable values.
//!
//! Writes are developer (or org owner), the same gate as the rest of
//! `/config`. Reads are any project role: a secret list entry is whether
//! the value is set, not the value, so it is no more sensitive than a
//! variable read, which an editor is allowed.

use std::sync::Arc;

use axum::extract::Path;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::put;
use axum::Json;
use axum::Router;
use serde::Deserialize;

use crate::http::response::value_error_to_response;
use crate::http::response::ApiResponse;
use crate::http::scope::ConfigMemberScope;
use crate::http::scope::ConfigProjectScope;
use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/secret/{user}/{project}/{dataset}/list", get(list_secrets))
        .route(
            "/secret/{user}/{project}/{dataset}/{name}",
            put(put_secret).delete(delete_secret),
        )
        .route(
            "/variable/{user}/{project}/{dataset}/list",
            get(list_variables),
        )
        .route(
            "/variable/{user}/{project}/{dataset}/{name}",
            put(put_variable).delete(delete_variable),
        )
}

#[derive(Deserialize)]
struct SetValueBody {
    value: String,
}

async fn list_secrets(
    scope: ConfigMemberScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset)): Path<(String, String, String)>,
) -> Response {
    match scope
        .config
        .list_secret_values(state.secrets.as_ref(), &dataset)
    {
        Ok(rows) => ApiResponse::success(rows).into_response(),
        Err(err) => value_error_to_response(err),
    }
}

async fn put_secret(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset, name)): Path<(String, String, String, String)>,
    Json(body): Json<SetValueBody>,
) -> Response {
    match scope
        .config
        .set_secret_value(state.secrets.as_ref(), &dataset, &name, &body.value)
    {
        Ok(view) => ApiResponse::success(view).into_response(),
        Err(err) => value_error_to_response(err),
    }
}

async fn delete_secret(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope
        .config
        .delete_secret_value(state.secrets.as_ref(), &dataset, &name)
    {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(err) => value_error_to_response(err),
    }
}

async fn list_variables(
    scope: ConfigMemberScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset)): Path<(String, String, String)>,
) -> Response {
    match scope
        .config
        .list_variable_values(state.data_adapter.as_ref(), &dataset)
    {
        Ok(rows) => ApiResponse::success(rows).into_response(),
        Err(err) => value_error_to_response(err),
    }
}

async fn put_variable(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset, name)): Path<(String, String, String, String)>,
    Json(body): Json<SetValueBody>,
) -> Response {
    match scope
        .config
        .set_variable_value(state.data_adapter.as_ref(), &dataset, &name, &body.value)
    {
        Ok(view) => ApiResponse::success(view).into_response(),
        Err(err) => value_error_to_response(err),
    }
}

async fn delete_variable(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, dataset, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope
        .config
        .delete_variable_value(state.data_adapter.as_ref(), &dataset, &name)
    {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(err) => value_error_to_response(err),
    }
}
