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
use crate::http::paths::lookup_site_in_project;
use crate::http::response::{ApiResponse, error_response};
use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/users", post(create_user))
        .route("/users/list", get(list_users))
        .route("/users/{id}", put(update_user).delete(delete_user))
        .route("/api-keys", post(create_api_key))
        .route("/api-keys/list", get(list_api_keys))
        .route("/api-keys/{id}", delete(revoke_api_key))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let project_id = headers
        .get("x-project-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());
    let site_name = headers
        .get("x-site-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());

    let (Some(project_id), Some(site_name)) = (project_id, site_name) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing site context: use X-Project-Id and X-Site-Id headers",
        );
    };

    if lookup_site_in_project(&state.schema, &project_id, &site_name).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!("unknown site: {site_name} in project {project_id}"),
        );
    }

    let qualified_site_id = format!("{project_id}/{site_name}");
    let credentials = LoginCredentials {
        username: body.username,
        password: None,
        site_id: qualified_site_id.clone(),
    };

    match state.auth.login(&qualified_site_id, &credentials) {
        Ok(session) => ApiResponse::success(session).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn logout(user: AuthenticatedUser, State(state): State<Arc<AppState>>) -> Response {
    match state.auth.logout(&user.0.token) {
        Ok(()) => ApiResponse::success("logged out").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn me(user: AuthenticatedUser) -> Response {
    ApiResponse::success(user.0.user).into_response()
}

#[derive(Deserialize)]
pub struct CreateUserHttpRequest {
    username: String,
    name: String,
    #[serde(default)]
    role: Option<String>,
}

pub async fn create_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserHttpRequest>,
) -> Response {
    let req = CreateUserRequest {
        username: body.username,
        name: body.name,
        role: body.role,
        password: None,
    };

    match state.auth.create_user(&user.0.user.site_id, &req) {
        Ok(new_user) => (StatusCode::CREATED, ApiResponse::success(new_user)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn list_users(user: AuthenticatedUser, State(state): State<Arc<AppState>>) -> Response {
    match state.auth.list_users(&user.0.user.site_id) {
        Ok(users) => ApiResponse::success(users).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct UpdateUserHttpRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

pub async fn update_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserHttpRequest>,
) -> Response {
    let updates = UpdateUserRequest {
        name: body.name,
        role: body.role,
        status: body.status,
    };

    match state.auth.update_user(&user.0.user.site_id, &id, &updates) {
        Ok(updated) => ApiResponse::success(updated).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn delete_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.auth.delete_user(&user.0.user.site_id, &id) {
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
        .auth
        .create_api_key(&user.0.user.site_id, &user.0.user.id, &body.label)
    {
        Ok(key) => (StatusCode::CREATED, ApiResponse::success(key)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn list_api_keys(user: AuthenticatedUser, State(state): State<Arc<AppState>>) -> Response {
    match state
        .auth
        .list_api_keys(&user.0.user.site_id, &user.0.user.id)
    {
        Ok(keys) => ApiResponse::success(keys).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

pub async fn revoke_api_key(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.auth.revoke_api_key(&user.0.user.site_id, &id) {
        Ok(()) => ApiResponse::success("revoked").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}
