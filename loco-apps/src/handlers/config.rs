use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::response::{ApiResponse, error_response, schema_error_to_response};
use crate::http::scope::{ConfigProjectScope, ConfigUserScope};
use crate::server::AppState;
use crate::{Dataset, DatasetUpdate, ProjectUpdate, Site, SiteUpdate};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete, get, post};
    Router::new()
        // project
        .route("/project", post(create_project))
        .route("/project/list", get(list_projects))
        .route(
            "/project/{user}/{project}",
            get(get_project).put(update_project).delete(delete_project),
        )
        // dataset
        .route("/dataset/{user}/{project}", post(create_dataset))
        .route("/dataset/{user}/{project}/list", get(list_datasets))
        .route(
            "/dataset/{user}/{project}/{name}",
            get(get_dataset).put(update_dataset).delete(delete_dataset),
        )
        // site
        .route("/site/{user}/{project}", post(create_site))
        .route("/site/{user}/{project}/list", get(list_sites))
        .route(
            "/site/{user}/{project}/{name}",
            get(get_site).put(update_site).delete(delete_site),
        )
        // version (read/edit of the manifest itself lives under /schema)
        .route("/version/{user}/{project}", post(create_version))
        .route("/version/{user}/{project}/list", get(list_versions))
        .route("/version/{user}/{project}/{version}", delete(delete_version))
}

// --- project ---

#[derive(Deserialize)]
pub struct CreateProjectBody {
    name: String,
    label: String,
    description: String,
}

pub async fn create_project(
    scope: ConfigUserScope,
    Json(body): Json<CreateProjectBody>,
) -> Response {
    if body.name.is_empty() || body.name.contains('/') {
        return error_response(
            StatusCode::BAD_REQUEST,
            "project name must be a single path segment",
        );
    }

    let pc = scope.project_config(&body.name);

    let project = match pc.create_project(body.label.clone(), body.description) {
        Ok(v) => v,
        Err(e) => return schema_error_to_response(e),
    };

    // Auto-bootstrap a default "dev" dataset + site for the new project.
    let default_label = format!("{} Dev", body.label);
    let _ = pc.create_dataset(Dataset::new(
        String::new(),
        "dev".to_string(),
        default_label.clone(),
        "Default development dataset".to_string(),
    ));
    let _ = pc.create_site(Site::new(
        String::new(),
        "dev".to_string(),
        default_label,
        "0.0.1-dev".to_string(),
        "dev".to_string(),
    ));

    (StatusCode::CREATED, ApiResponse::success(project)).into_response()
}

pub async fn list_projects(scope: ConfigUserScope) -> Response {
    ApiResponse::success(scope.list_projects()).into_response()
}

pub async fn get_project(scope: ConfigProjectScope) -> Response {
    match scope.config.project_record() {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("project not found: {}", scope.project_id()),
        ),
    }
}

pub async fn update_project(
    scope: ConfigProjectScope,
    Json(patch): Json<ProjectUpdate>,
) -> Response {
    match scope.config.update_project(patch) {
        Ok(v) => ApiResponse::success(v).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_project(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    // Schema-level cascade lives in `delete_project`; it returns the dataset
    // names so we can purge their records from the lake here. Lake errors
    // during cascade are swallowed — the metadata is what's authoritative.
    let project_id = scope.project_id();
    match scope.config.delete_project() {
        Ok(dataset_names) => {
            for name in dataset_names {
                let qualified = format!("{project_id}/{name}");
                let _ = state.data_adapter.delete_dataset(&qualified);
            }
            ApiResponse::success("deleted").into_response()
        }
        Err(e) => schema_error_to_response(e),
    }
}

// --- dataset ---

pub async fn create_dataset(
    scope: ConfigProjectScope,
    Json(input): Json<Dataset>,
) -> Response {
    match scope.config.create_dataset(input) {
        Ok(v) => (StatusCode::CREATED, ApiResponse::success(v)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_datasets(scope: ConfigProjectScope) -> Response {
    ApiResponse::success(scope.config.datasets()).into_response()
}

pub async fn get_dataset(
    scope: ConfigProjectScope,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    match scope.config.dataset(&name) {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("dataset not found: {}/{name}", scope.project_id()),
        ),
    }
}

pub async fn update_dataset(
    scope: ConfigProjectScope,
    Path((_, _, name)): Path<(String, String, String)>,
    Json(patch): Json<DatasetUpdate>,
) -> Response {
    match scope.config.update_dataset(&name, patch) {
        Ok(v) => ApiResponse::success(v).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_dataset(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    let qualified = format!("{}/{name}", scope.project_id());
    if let Err(e) = state.data_adapter.delete_dataset(&qualified) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to purge dataset records: {e}"),
        );
    }

    match scope.config.delete_dataset(&name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- site ---

pub async fn create_site(
    scope: ConfigProjectScope,
    Json(input): Json<Site>,
) -> Response {
    match scope.config.create_site(input) {
        Ok(v) => (StatusCode::CREATED, ApiResponse::success(v)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_sites(scope: ConfigProjectScope) -> Response {
    ApiResponse::success(scope.config.sites()).into_response()
}

pub async fn get_site(
    scope: ConfigProjectScope,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    match scope.config.site(&name) {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("site not found: {}/{name}", scope.project_id()),
        ),
    }
}

pub async fn update_site(
    scope: ConfigProjectScope,
    Path((_, _, name)): Path<(String, String, String)>,
    Json(patch): Json<SiteUpdate>,
) -> Response {
    match scope.config.update_site(&name, patch) {
        Ok(v) => ApiResponse::success(v).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_site(
    scope: ConfigProjectScope,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    match scope.config.delete_site(&name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- version ---

#[derive(Deserialize)]
pub struct CreateVersionBody {
    version: String,
}

pub async fn create_version(
    scope: ConfigProjectScope,
    Json(body): Json<CreateVersionBody>,
) -> Response {
    if body.version.is_empty() || body.version.contains('/') {
        return error_response(
            StatusCode::BAD_REQUEST,
            "version must be a single path segment",
        );
    }
    match scope.config.create_version(body.version) {
        Ok(v) => (StatusCode::CREATED, ApiResponse::success(v)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_versions(scope: ConfigProjectScope) -> Response {
    ApiResponse::success(scope.config.manifests()).into_response()
}

pub async fn delete_version(
    scope: ConfigProjectScope,
    Path((_, _, version)): Path<(String, String, String)>,
) -> Response {
    match scope.config.delete_version(&version) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}
