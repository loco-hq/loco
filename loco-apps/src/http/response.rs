use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
        })
    }
}

pub fn error_response(status: StatusCode, msg: &str) -> Response {
    let body = ApiResponse::<()> {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
    };
    (status, Json(body)).into_response()
}

pub fn lake_error_to_response(err: loco_lake::Error) -> Response {
    match err {
        loco_lake::Error::NotFound => error_response(StatusCode::NOT_FOUND, "not found"),
        loco_lake::Error::AlreadyExists => error_response(StatusCode::CONFLICT, "already exists"),
        loco_lake::Error::InvalidDataset(msg) => {
            error_response(StatusCode::BAD_REQUEST, &format!("invalid dataset: {msg}"))
        }
        loco_lake::Error::Internal(msg) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &msg)
        }
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
