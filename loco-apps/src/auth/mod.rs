pub mod local;

use std::fmt;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::server::AppState;

// --- Error ---

#[derive(Debug)]
pub enum AuthError {
    InvalidCredentials,
    SessionExpired,
    SessionNotFound,
    UserNotFound,
    UserAlreadyExists,
    Unauthorized,
    Internal(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "invalid credentials"),
            AuthError::SessionExpired => write!(f, "session expired"),
            AuthError::SessionNotFound => write!(f, "session not found"),
            AuthError::UserNotFound => write!(f, "user not found"),
            AuthError::UserAlreadyExists => write!(f, "user already exists"),
            AuthError::Unauthorized => write!(f, "unauthorized"),
            AuthError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

// --- Data types ---

#[derive(Debug, Clone, Serialize)]
pub struct AuthSession {
    pub token: String,
    pub user: AuthUser,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub site_id: String,
    pub username: String,
    pub name: String,
    pub role: String,
    pub status: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

pub struct LoginCredentials {
    pub username: String,
    pub password: Option<String>,
    pub site_id: String,
}

pub struct CreateUserRequest {
    pub username: String,
    pub name: String,
    pub role: Option<String>,
    pub password: Option<String>,
}

pub struct UpdateUserRequest {
    pub name: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked: bool,
}

// --- Trait ---

pub trait AuthAdapter: Send + Sync {
    fn login(&self, site_id: &str, credentials: &LoginCredentials) -> Result<AuthSession, AuthError>;
    fn validate_session(&self, token: &str) -> Result<AuthSession, AuthError>;
    fn logout(&self, token: &str) -> Result<(), AuthError>;
    fn revoke_all_sessions(&self, site_id: &str, user_id: &str) -> Result<(), AuthError>;
    fn get_user(&self, site_id: &str, user_id: &str) -> Result<Option<AuthUser>, AuthError>;
    fn list_users(&self, site_id: &str) -> Result<Vec<AuthUser>, AuthError>;
    fn create_user(&self, site_id: &str, user: &CreateUserRequest) -> Result<AuthUser, AuthError>;
    fn update_user(&self, site_id: &str, user_id: &str, updates: &UpdateUserRequest) -> Result<AuthUser, AuthError>;
    fn delete_user(&self, site_id: &str, user_id: &str) -> Result<(), AuthError>;
    fn create_api_key(&self, site_id: &str, user_id: &str, label: &str) -> Result<ApiKey, AuthError>;
    fn validate_api_key(&self, key: &str) -> Result<AuthSession, AuthError>;
    fn revoke_api_key(&self, site_id: &str, key_id: &str) -> Result<(), AuthError>;
    fn list_api_keys(&self, site_id: &str, user_id: &str) -> Result<Vec<ApiKeyInfo>, AuthError>;
}

// --- Extractor ---

pub struct AuthenticatedUser(pub AuthSession);

impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 1. Authorization: Bearer {token} header
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v.to_string());

        // 2. ?session= query param (for browser dev)
        let token = token.or_else(|| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .find(|(k, _)| *k == "session")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| !v.is_empty())
            })
        });

        let Some(token) = token else {
            return Err(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "missing auth: use Authorization: Bearer <token> header or ?session= query param",
            ));
        };

        // Try session token first, then API key
        match state.auth.validate_session(&token) {
            Ok(session) => Ok(AuthenticatedUser(session)),
            Err(_) => match state.auth.validate_api_key(&token) {
                Ok(session) => Ok(AuthenticatedUser(session)),
                Err(_) => Err(auth_error_response(
                    StatusCode::UNAUTHORIZED,
                    "invalid or expired session",
                )),
            },
        }
    }
}

fn auth_error_response(status: StatusCode, msg: &str) -> Response {
    #[derive(Serialize)]
    struct ErrBody {
        ok: bool,
        error: String,
    }
    (
        status,
        Json(ErrBody {
            ok: false,
            error: msg.to_string(),
        }),
    )
        .into_response()
}

pub fn auth_error_to_response(err: AuthError) -> Response {
    match err {
        AuthError::InvalidCredentials => {
            auth_error_response(StatusCode::UNAUTHORIZED, "invalid credentials")
        }
        AuthError::SessionExpired | AuthError::SessionNotFound => {
            auth_error_response(StatusCode::UNAUTHORIZED, &err.to_string())
        }
        AuthError::UserNotFound => {
            auth_error_response(StatusCode::NOT_FOUND, "user not found")
        }
        AuthError::UserAlreadyExists => {
            auth_error_response(StatusCode::CONFLICT, "user already exists")
        }
        AuthError::Unauthorized => {
            auth_error_response(StatusCode::FORBIDDEN, "unauthorized")
        }
        AuthError::Internal(msg) => {
            auth_error_response(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
    }
}
