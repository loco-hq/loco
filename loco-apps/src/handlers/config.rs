use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;

use crate::http::response::{ApiResponse, error_response, schema_error_to_response};
use crate::http::scope::{ConfigProjectScope, ConfigUserScope};
use crate::server::AppState;
use crate::{Dataset, DatasetUpdate, Project, ProjectUpdate, Site, SiteUpdate};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{get, post};
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
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProjectBody>,
) -> Response {
    if body.name.is_empty() || body.name.contains('/') {
        return error_response(
            StatusCode::BAD_REQUEST,
            "project name must be a single path segment",
        );
    }

    let project_id = format!("{}/{}", scope.username(), body.name);
    let label = body.label.clone();

    let project = match state.schema.projects().create(Project::new(
        project_id.clone(),
        body.label,
        body.description,
    )) {
        Ok(v) => v,
        Err(e) => return schema_error_to_response(e),
    };

    // Auto-bootstrap a default "dev" dataset + site for the new project.
    let _ = state.schema.datasets().create(Dataset::new(
        project_id.clone(),
        "dev".to_string(),
        format!("{label} Dev"),
        "Default development dataset".to_string(),
    ));
    let _ = state.schema.sites().create(Site::new(
        project_id,
        "dev".to_string(),
        format!("{label} Dev"),
        "0.0.1-dev".to_string(),
        "dev".to_string(),
    ));

    (StatusCode::CREATED, ApiResponse::success(project)).into_response()
}

pub async fn list_projects(
    scope: ConfigUserScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    let prefix = format!("{}/", scope.username());
    ApiResponse::success(state.schema.projects().list(&prefix)).into_response()
}

pub async fn get_project(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    let id = Project::to_path(&scope.project_id());
    match state.schema.projects().get(&id) {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("project not found: {id}")),
    }
}

pub async fn update_project(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Json(patch): Json<ProjectUpdate>,
) -> Response {
    let id = Project::to_path(&scope.project_id());
    match state.schema.projects().update(&id, patch) {
        Ok(v) => ApiResponse::success(v).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_project(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    let project_id = scope.project_id();
    let prefix = format!("{project_id}/");

    // Cascade datasets — purge each from the lake before removing metadata.
    for (ds_id, _) in state.schema.datasets().list_all() {
        if !ds_id.starts_with(&prefix) {
            continue;
        }
        if let Some(dataset_name) =
            Dataset::from_path(&ds_id).and_then(|v| v.get("name").cloned())
        {
            let qualified = format!("{project_id}/{dataset_name}");
            let _ = state.data_adapter.delete_dataset(&qualified);
        }
        let _ = state.schema.datasets().delete(&ds_id);
    }

    // Cascade sites
    for (site_id, _) in state.schema.sites().list_all() {
        if site_id.starts_with(&prefix) {
            let _ = state.schema.sites().delete(&site_id);
        }
    }

    let id = Project::to_path(&project_id);
    match state.schema.projects().delete(&id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- dataset ---

#[derive(Deserialize)]
pub struct CreateDatasetBody {
    name: String,
    label: String,
    description: String,
}

pub async fn create_dataset(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateDatasetBody>,
) -> Response {
    let dataset = Dataset::new(
        scope.project_id(),
        body.name,
        body.label,
        body.description,
    );
    match state.schema.datasets().create(dataset) {
        Ok(v) => (StatusCode::CREATED, ApiResponse::success(v)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_datasets(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    let prefix = format!("{}/", scope.project_id());
    ApiResponse::success(state.schema.datasets().list(&prefix)).into_response()
}

pub async fn get_dataset(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    let id = Dataset::to_path(&scope.project_id(), &name);
    match state.schema.datasets().get(&id) {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("dataset not found: {id}")),
    }
}

pub async fn update_dataset(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
    Json(patch): Json<DatasetUpdate>,
) -> Response {
    let id = Dataset::to_path(&scope.project_id(), &name);
    match state.schema.datasets().update(&id, patch) {
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

    let id = Dataset::to_path(&scope.project_id(), &name);
    match state.schema.datasets().delete(&id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- site ---

#[derive(Deserialize)]
pub struct CreateSiteBody {
    name: String,
    label: String,
    version: String,
    dataset: String,
}

pub async fn create_site(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSiteBody>,
) -> Response {
    let site = Site::new(
        scope.project_id(),
        body.name,
        body.label,
        body.version,
        body.dataset,
    );
    match state.schema.sites().create(site) {
        Ok(v) => (StatusCode::CREATED, ApiResponse::success(v)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_sites(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
) -> Response {
    let prefix = format!("{}/", scope.project_id());
    ApiResponse::success(state.schema.sites().list(&prefix)).into_response()
}

pub async fn get_site(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    let id = Site::to_path(&scope.project_id(), &name);
    match state.schema.sites().get(&id) {
        Some(v) => ApiResponse::success(v).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("site not found: {id}")),
    }
}

pub async fn update_site(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
    Json(patch): Json<SiteUpdate>,
) -> Response {
    let id = Site::to_path(&scope.project_id(), &name);
    match state.schema.sites().update(&id, patch) {
        Ok(v) => ApiResponse::success(v).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_site(
    scope: ConfigProjectScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, name)): Path<(String, String, String)>,
) -> Response {
    let id = Site::to_path(&scope.project_id(), &name);
    match state.schema.sites().delete(&id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}
