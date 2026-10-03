//! Site-scoped action reads and the runner.
//!
//! `GET` follows the `/data/.../fields` read rule, lifted off one collection:
//! a caller who can read any collection this version shows, or who has data
//! access. `POST` is `require_can_write_data`, so `public` cannot run an
//! action even when a public permission set grants `create`. An unknown name
//! is 404 before that check, the same order as `/data/{collection}/add`.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Map;

use crate::actions::{dispatch, ActionFailure, Dispatch, HandlerDeps};
use crate::http::authz::forbidden;
use crate::http::response::{
    error_response, error_response_with_diagnostics, validation_error_response, ApiResponse,
};
use crate::http::scope::SiteScope;
use crate::server::AppState;

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::get;
    Router::new()
        .route("/", get(list_actions))
        .route("/{name}", get(get_action).post(run_action))
}

fn require_read(scope: &SiteScope) -> Result<(), Response> {
    if scope.may_read_actions()? {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub async fn list_actions(scope: SiteScope) -> Response {
    if let Err(resp) = require_read(&scope) {
        return resp;
    }
    ApiResponse::success(scope.schema.actions()).into_response()
}

pub async fn get_action(scope: SiteScope, Path(name): Path<String>) -> Response {
    let Some(action) = scope.schema.action(&name) else {
        return error_response(StatusCode::NOT_FOUND, &format!("action not found: {name}"));
    };
    if let Err(resp) = require_read(&scope) {
        return resp;
    }
    ApiResponse::success(action).into_response()
}

#[derive(Deserialize)]
struct RunBody {
    #[serde(default)]
    input: Map<String, serde_json::Value>,
}

async fn run_action(
    scope: SiteScope,
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    body: Result<Json<RunBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return error_response(StatusCode::BAD_REQUEST, &rejection.body_text());
        }
    };
    // Unknown before the write check, matching the collection extractor:
    // a public caller learns the name is missing, not that they are refused.
    if scope.schema.action(&name).is_none() {
        return error_response(StatusCode::NOT_FOUND, &format!("action not found: {name}"));
    }
    if let Err(resp) = scope.require_can_write_data() {
        return resp;
    }
    let dataset_id = scope.dataset_id();
    match dispatch(
        &scope.schema,
        &state.actions,
        &dataset_id,
        HandlerDeps {
            data: state.data_adapter.clone(),
            secrets: state.secrets.clone(),
            http: state.http.clone(),
        },
        scope.user(),
        &name,
        &body.input,
    )
    .await
    {
        Dispatch::NotFound => {
            error_response(StatusCode::NOT_FOUND, &format!("action not found: {name}"))
        }
        Dispatch::Invalid(report) => validation_error_response(report.diagnostics),
        Dispatch::NoHandler { project, name } => error_response(
            StatusCode::NOT_IMPLEMENTED,
            &format!("no handler for action {project}.{name}"),
        ),
        Dispatch::MissingConfig(diagnostics) => error_response_with_diagnostics(
            StatusCode::BAD_REQUEST,
            "required configuration is not set",
            diagnostics,
        ),
        Dispatch::Done(value) => ApiResponse::success(value).into_response(),
        Dispatch::Failed(failure) => failure_response(failure),
    }
}

fn failure_response(failure: ActionFailure) -> Response {
    match failure {
        ActionFailure::BadInput {
            message,
            diagnostics,
        } => error_response_with_diagnostics(StatusCode::BAD_REQUEST, &message, diagnostics),
        ActionFailure::Conflict {
            message,
            diagnostics,
        } => error_response_with_diagnostics(StatusCode::CONFLICT, &message, diagnostics),
        ActionFailure::Failed {
            message,
            diagnostics,
        } => error_response_with_diagnostics(
            StatusCode::INTERNAL_SERVER_ERROR,
            &message,
            diagnostics,
        ),
        ActionFailure::Upstream { status, message } => error_response(
            StatusCode::BAD_GATEWAY,
            &format!("upstream {status}: {message}"),
        ),
        ActionFailure::Unavailable { message } => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, &message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_failures_map_to_status() {
        let cases = [
            (
                ActionFailure::BadInput {
                    message: "no".into(),
                    diagnostics: Vec::new(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                ActionFailure::Conflict {
                    message: "taken".into(),
                    diagnostics: Vec::new(),
                },
                StatusCode::CONFLICT,
            ),
            (
                ActionFailure::Failed {
                    message: "wrote the order, the charge did not".into(),
                    diagnostics: Vec::new(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ActionFailure::Upstream {
                    status: 503,
                    message: "upstream is down".into(),
                },
                StatusCode::BAD_GATEWAY,
            ),
            (
                ActionFailure::Unavailable {
                    message: "LOCO_SECRET_KEY is not set".into(),
                },
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ];
        for (failure, status) in cases {
            assert_eq!(failure_response(failure).status(), status);
        }
    }
}
