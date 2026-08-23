use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::{
    auth_error_to_response, AuthenticatedUser, CreateUserRequest, LoginCredentials,
    UpdateUserRequest,
};
use crate::http::authz::require_self;
use crate::http::response::{error_response, ApiResponse};
use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/users", post(create_user))
        .route("/users/{id}", put(update_user).delete(delete_user))
        .route("/api-keys", post(create_api_key))
        .route("/api-keys/list", get(list_api_keys))
        .route("/api-keys/{id}", delete(revoke_api_key))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    #[serde(default)]
    password: Option<String>,
}

/// Global login. Does not use `SiteScope` — identity is not site-scoped.
/// Site headers on this request are ignored.
pub async fn login(State(state): State<Arc<AppState>>, Json(body): Json<LoginRequest>) -> Response {
    let credentials = LoginCredentials {
        username: body.username,
        password: body.password,
    };

    match state.auth_adapter.login(&credentials) {
        Ok(session) => ApiResponse::success(session).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn logout(user: AuthenticatedUser, State(state): State<Arc<AppState>>) -> Response {
    match state.auth_adapter.logout(&user.0.token) {
        Ok(()) => ApiResponse::success("logged out").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

/// Authenticated self-read. `/auth/users/{id}` is not a public lookup.
pub async fn me(user: AuthenticatedUser) -> Response {
    ApiResponse::success(user.0.user).into_response()
}

#[derive(Deserialize)]
pub struct CreateUserHttpRequest {
    username: String,
    name: String,
    #[serde(default)]
    password: Option<String>,
}

/// Self-service signup. No token. Password is required in the adapter
/// (`CreateUserRequest.password` is a `String`); this maps a missing body
/// field to 400 instead of 401.
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserHttpRequest>,
) -> Response {
    let password = body.password.as_deref().map(str::trim).unwrap_or("");
    if password.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "password is required");
    }
    let req = CreateUserRequest {
        username: body.username,
        name: body.name,
        password: password.to_string(),
    };

    match state.auth_adapter.create_user(&req) {
        Ok(new_user) => (StatusCode::CREATED, ApiResponse::success(new_user)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct UpdateUserHttpRequest {
    #[serde(default)]
    name: Option<String>,
}

pub async fn update_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserHttpRequest>,
) -> Response {
    if let Err(resp) = require_self(&user.0.user.id, &id) {
        return resp;
    }
    let updates = UpdateUserRequest { name: body.name };

    match state.auth_adapter.update_user(&id, &updates) {
        Ok(updated) => ApiResponse::success(updated).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn delete_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    if let Err(resp) = require_self(&user.0.user.id, &id) {
        return resp;
    }
    match state.auth_adapter.delete_user(&id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    label: String,
}

pub async fn create_api_key(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Response {
    match state
        .auth_adapter
        .create_api_key(&user.0.user.id, &body.label)
    {
        Ok(key) => (StatusCode::CREATED, ApiResponse::success(key)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn list_api_keys(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.auth_adapter.list_api_keys(&user.0.user.id) {
        Ok(keys) => ApiResponse::success(keys).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn revoke_api_key(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.auth_adapter.revoke_api_key(&user.0.user.id, &id) {
        Ok(()) => ApiResponse::success("revoked").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}
