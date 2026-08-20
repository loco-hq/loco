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

/// Known password for seeded identities and auto-created test users.
/// Login still accepts a missing password as a test-only bypass (removed in PR 2).
pub const TEST_PASSWORD: &str = "password";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Person,
    Org,
}

impl AccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountType::Person => "person",
            AccountType::Org => "org",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthSession {
    pub token: String,
    pub user: AuthUser,
}

/// Reserved username for unauthenticated requests.
pub const PUBLIC_USERNAME: &str = "public";

impl AuthSession {
    /// Synthetic session used when a request arrives with no auth token.
    /// `public` is a principal, not an account — it has no site and no password.
    pub fn public() -> Self {
        AuthSession {
            token: String::new(),
            user: AuthUser {
                id: PUBLIC_USERNAME.to_string(),
                username: PUBLIC_USERNAME.to_string(),
                name: "Public".to_string(),
                account_type: PUBLIC_USERNAME.to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
                last_login_at: None,
            },
        }
    }
}

/// A logged-in identity. `username` is the person-account handle (and the
/// identity handle). This is who you are — not the site named by request
/// headers.
#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub name: String,
    pub account_type: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

pub struct LoginCredentials {
    pub username: String,
    pub password: Option<String>,
}

pub struct CreateUserRequest {
    pub username: String,
    pub name: String,
    pub password: Option<String>,
}

pub struct UpdateUserRequest {
    pub name: Option<String>,
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
    fn login(&self, credentials: &LoginCredentials) -> Result<AuthSession, AuthError>;
    fn validate_session(&self, token: &str) -> Result<AuthSession, AuthError>;
    fn logout(&self, token: &str) -> Result<(), AuthError>;
    fn revoke_all_sessions(&self, identity_id: &str) -> Result<(), AuthError>;
    fn get_user(&self, user_id: &str) -> Result<Option<AuthUser>, AuthError>;
    fn list_users(&self) -> Result<Vec<AuthUser>, AuthError>;
    fn create_user(&self, user: &CreateUserRequest) -> Result<AuthUser, AuthError>;
    fn update_user(&self, user_id: &str, updates: &UpdateUserRequest) -> Result<AuthUser, AuthError>;
    fn delete_user(&self, user_id: &str) -> Result<(), AuthError>;
    fn create_api_key(&self, identity_id: &str, label: &str) -> Result<ApiKey, AuthError>;
    fn validate_api_key(&self, key: &str) -> Result<AuthSession, AuthError>;
    fn revoke_api_key(&self, identity_id: &str, key_id: &str) -> Result<(), AuthError>;
    fn list_api_keys(&self, identity_id: &str) -> Result<Vec<ApiKeyInfo>, AuthError>;
}

// --- Extractor ---

pub struct AuthenticatedUser(pub AuthSession);

impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v.to_string());

        let Some(token) = token else {
            return Err(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "missing auth: use Authorization: Bearer <token> header",
            ));
        };

        // Try session token first, then API key
        match state.auth_adapter.validate_session(&token) {
            Ok(session) => Ok(AuthenticatedUser(session)),
            Err(_) => match state.auth_adapter.validate_api_key(&token) {
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
