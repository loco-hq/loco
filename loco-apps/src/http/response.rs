use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::validation::Diagnostic;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<Diagnostic>>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
            diagnostics: None,
        })
    }

    /// Like `success`, but attaches non-fatal diagnostics. Empty list collapses
    /// to `None` so the field stays absent on clean reads.
    pub fn success_with_diagnostics(data: T, diagnostics: Vec<Diagnostic>) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
            diagnostics: if diagnostics.is_empty() {
                None
            } else {
                Some(diagnostics)
            },
        })
    }
}

pub fn error_response(status: StatusCode, msg: &str) -> Response {
    error_response_with_diagnostics(status, msg, Vec::new())
}

/// Like [`error_response`], attaching diagnostics when the handler has any.
/// An empty list stays off the body, the same as a clean success.
pub fn error_response_with_diagnostics(
    status: StatusCode,
    msg: &str,
    diagnostics: Vec<Diagnostic>,
) -> Response {
    let body = ApiResponse::<()> {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
        diagnostics: if diagnostics.is_empty() {
            None
        } else {
            Some(diagnostics)
        },
    };
    (status, Json(body)).into_response()
}

/// 400 with the validation report attached. Use for write paths when
/// `ValidationReport::has_errors()` is true.
pub fn validation_error_response(diagnostics: Vec<Diagnostic>) -> Response {
    let body = ApiResponse::<()> {
        ok: false,
        data: None,
        error: Some("validation failed".to_string()),
        diagnostics: Some(diagnostics),
    };
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

pub fn lake_error_to_response(err: loco_lake::Error) -> Response {
    match err {
        loco_lake::Error::NotFound => error_response(StatusCode::NOT_FOUND, "not found"),
        loco_lake::Error::AlreadyExists => error_response(StatusCode::CONFLICT, "already exists"),
        loco_lake::Error::InvalidDataset(msg) => {
            error_response(StatusCode::BAD_REQUEST, &format!("invalid dataset: {msg}"))
        }
        loco_lake::Error::InvalidQuery(msg) => {
            error_response(StatusCode::BAD_REQUEST, &format!("invalid query: {msg}"))
        }
        loco_lake::Error::Internal(msg) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &msg),
    }
}

pub fn schema_error_to_response(err: loco_schema_runtime::Error) -> Response {
    match &err {
        loco_schema_runtime::Error::AlreadyExists(_) => {
            error_response(StatusCode::CONFLICT, &err.to_string())
        }
        loco_schema_runtime::Error::NotFound(_) => {
            error_response(StatusCode::NOT_FOUND, &err.to_string())
        }
        _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub fn value_error_to_response(err: crate::http::config_values::ValueError) -> Response {
    use crate::http::config_values::ValueError;
    match err {
        e @ (ValueError::InvalidName(_) | ValueError::Undeclared(_)) => {
            error_response(StatusCode::BAD_REQUEST, &e.to_string())
        }
        e @ (ValueError::UnknownDataset(_) | ValueError::NotFound(_)) => {
            error_response(StatusCode::NOT_FOUND, &e.to_string())
        }
        e @ ValueError::Unavailable(_) => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, &e.to_string())
        }
        ValueError::Lake(e) => lake_error_to_response(e),
    }
}

pub fn config_error_to_response(err: crate::http::project_config::ConfigError) -> Response {
    use crate::http::project_config::ConfigError;
    match err {
        e @ (ConfigError::InvalidName(_)
        | ConfigError::InvalidPin(_)
        | ConfigError::InvalidDependency(_)) => {
            error_response(StatusCode::BAD_REQUEST, &e.to_string())
        }
        e @ ConfigError::Pinned(_) => error_response(StatusCode::CONFLICT, &e.to_string()),
        e @ (ConfigError::Purge(_) | ConfigError::LeftBehind(_)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
        ConfigError::Schema(e) => schema_error_to_response(e),
    }
}

pub fn version_schema_error_to_response(
    err: crate::http::version_schema::VersionSchemaError,
) -> Response {
    use crate::http::version_schema::VersionSchemaError;
    match err {
        e @ (VersionSchemaError::NotWritable(_)
        | VersionSchemaError::InvalidDependency(_)
        | VersionSchemaError::InvalidFieldType(_)
        | VersionSchemaError::InvalidDeclaration(_)
        | VersionSchemaError::InvalidName(_)) => {
            error_response(StatusCode::BAD_REQUEST, &e.to_string())
        }
        e @ VersionSchemaError::UnknownVersion(_) => {
            error_response(StatusCode::NOT_FOUND, &e.to_string())
        }
        e @ VersionSchemaError::LeftBehind(_) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
        VersionSchemaError::Schema(e) => schema_error_to_response(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::config_values::ValueError;

    #[test]
    fn unavailable_maps_to_503() {
        let response =
            value_error_to_response(ValueError::Unavailable("LOCO_SECRET_KEY is not set".into()));
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
