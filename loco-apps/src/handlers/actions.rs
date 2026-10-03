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
use serde::Serialize;
use serde_json::Map;

use crate::actions::{dispatch, ActionFailure, Dispatch};
use crate::http::authz::forbidden;
use crate::http::response::{
    error_response, error_response_with_diagnostics, validation_error_response, ApiResponse,
};
use crate::http::scope::SiteScope;
use crate::http::version_schema::VersionSchema;
use crate::server::AppState;
use crate::{Action, ActionParam};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::get;
    Router::new()
        .route("/", get(list_actions))
        .route("/{name}", get(get_action).post(run_action))
}

#[derive(Serialize)]
struct ActionView {
    #[serde(flatten)]
    action: Action,
    params: Vec<ActionParam>,
}

fn view(schema: &VersionSchema, action: &Action) -> ActionView {
    let params = schema
        .action_params_of(action.project(), action.name())
        .into_iter()
        .map(|param| (*param).clone())
        .collect();
    ActionView {
        action: action.clone(),
        params,
    }
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
    let views: Vec<ActionView> = scope
        .schema
        .actions()
        .iter()
        .map(|action| view(&scope.schema, action))
        .collect();
    ApiResponse::success(views).into_response()
}

pub async fn get_action(scope: SiteScope, Path(name): Path<String>) -> Response {
    let Some(action) = scope.schema.action(&name) else {
        return error_response(StatusCode::NOT_FOUND, &format!("action not found: {name}"));
    };
    if let Err(resp) = require_read(&scope) {
        return resp;
    }
    ApiResponse::success(view(&scope.schema, &action)).into_response()
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
    match dispatch(
        &scope.schema,
        &state.actions,
        &scope.dataset_id(),
        state.data_adapter.as_ref(),
        scope.user(),
        &name,
        &body.input,
    ) {
        Dispatch::NotFound => {
            error_response(StatusCode::NOT_FOUND, &format!("action not found: {name}"))
        }
        Dispatch::Invalid(report) => validation_error_response(report.diagnostics),
        Dispatch::NoHandler { project, name } => error_response(
            StatusCode::NOT_IMPLEMENTED,
            &format!("no handler for action {project}.{name}"),
        ),
        Dispatch::Done(value) => ApiResponse::success(value).into_response(),
        Dispatch::Failed(failure) => failure_response(failure),
    }
}

fn failure_response(failure: ActionFailure) -> Response {
    let status = match &failure {
        ActionFailure::BadInput { .. } => StatusCode::BAD_REQUEST,
        ActionFailure::Conflict { .. } => StatusCode::CONFLICT,
        ActionFailure::Failed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response_with_diagnostics(status, failure.message(), failure.diagnostics().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_failures_map_to_400_409_500() {
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
        ];
        for (failure, status) in cases {
            assert_eq!(failure_response(failure).status(), status);
        }
    }
}
