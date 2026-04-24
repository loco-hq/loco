use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::http::authz::require_config_site;
use crate::http::extract::ConfigAuth;
use crate::http::response::{ApiResponse, error_response, schema_error_to_response};
use crate::server::AppState;
use crate::{Dataset, DatasetUpdate, Project, ProjectUpdate, Site, SiteUpdate};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete as route_delete, get as route_get, post, put};
    Router::new()
        .route("/{type_name}/list", route_get(list))
        .route("/get/{*path}", route_get(get))
        .route("/create/{*path}", post(create))
        .route("/update/{*path}", put(update))
        .route("/delete/{*path}", route_delete(delete))
}

#[derive(Deserialize)]
pub struct CreateConfigRequest {
    fields: HashMap<String, String>,
}

fn unknown_type(type_name: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, &format!("unknown type: {type_name}"))
}

pub async fn list(
    auth_user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(type_name): Path<String>,
) -> Response {
    if let Err(resp) = require_config_site(&auth_user.0.user) {
        return resp;
    }
    let prefix = format!("{}/", auth_user.0.user.username);
    match type_name.as_str() {
        "project" => ApiResponse::success(state.schema.projects().list(&prefix)).into_response(),
        "dataset" => ApiResponse::success(state.schema.datasets().list(&prefix)).into_response(),
        "site" => ApiResponse::success(state.schema.sites().list(&prefix)).into_response(),
        _ => unknown_type(&type_name),
    }
}

pub async fn get(ctx: ConfigAuth, State(state): State<Arc<AppState>>) -> Response {
    let not_found = || {
        error_response(
            StatusCode::NOT_FOUND,
            &format!("{} not found: {}", ctx.type_name, ctx.id),
        )
    };
    match ctx.type_name.as_str() {
        "project" => match state.schema.projects().get(&ctx.id) {
            Some(v) => ApiResponse::success(v).into_response(),
            None => not_found(),
        },
        "dataset" => match state.schema.datasets().get(&ctx.id) {
            Some(v) => ApiResponse::success(v).into_response(),
            None => not_found(),
        },
        "site" => match state.schema.sites().get(&ctx.id) {
            Some(v) => ApiResponse::success(v).into_response(),
            None => not_found(),
        },
        _ => unknown_type(&ctx.type_name),
    }
}

pub async fn create(
    ctx: ConfigAuth,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateConfigRequest>,
) -> Response {
    let template_vars = match ctx.type_name.as_str() {
        "project" => Project::from_path(&ctx.id),
        "dataset" => Dataset::from_path(&ctx.id),
        "site" => Site::from_path(&ctx.id),
        _ => return unknown_type(&ctx.type_name),
    }
    .unwrap_or_default();

    let mut fields = body.fields;
    for (k, v) in &template_vars {
        fields.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let result = match ctx.type_name.as_str() {
        "project" => state
            .schema
            .projects()
            .create(Project::from_map(&fields))
            .map(|v| v.to_map()),
        "dataset" => state
            .schema
            .datasets()
            .create(Dataset::from_map(&fields))
            .map(|v| v.to_map()),
        "site" => state
            .schema
            .sites()
            .create(Site::from_map(&fields))
            .map(|v| v.to_map()),
        _ => return unknown_type(&ctx.type_name),
    };
    let result = match result {
        Ok(r) => r,
        Err(e) => return schema_error_to_response(e),
    };

    // When a project is created, bootstrap a default "dev" site and dataset
    if ctx.type_name == "project" {
        if let Some(project_path) = template_vars.get("project") {
            let project_slug = project_path.split('/').next_back().unwrap_or("project");
            let label = result
                .get("label")
                .cloned()
                .unwrap_or_else(|| project_slug.to_string());

            let _ = state.schema.datasets().create(Dataset::new(
                project_path.clone(),
                "dev".to_string(),
                format!("{label} Dev"),
                "Default development dataset".to_string(),
            ));

            let _ = state.schema.sites().create(Site::new(
                project_path.clone(),
                "dev".to_string(),
                format!("{label} Dev"),
                "0.0.1-dev".to_string(),
                "dev".to_string(),
            ));
        }
    }

    (StatusCode::CREATED, ApiResponse::success(result)).into_response()
}

pub async fn update(
    ctx: ConfigAuth,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateConfigRequest>,
) -> Response {
    let result = match ctx.type_name.as_str() {
        "project" => state
            .schema
            .projects()
            .update(&ctx.id, ProjectUpdate::from_map(&body.fields))
            .map(|v| v.to_map()),
        "dataset" => state
            .schema
            .datasets()
            .update(&ctx.id, DatasetUpdate::from_map(&body.fields))
            .map(|v| v.to_map()),
        "site" => state
            .schema
            .sites()
            .update(&ctx.id, SiteUpdate::from_map(&body.fields))
            .map(|v| v.to_map()),
        _ => return unknown_type(&ctx.type_name),
    };
    match result {
        Ok(r) => ApiResponse::success(r).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete(ctx: ConfigAuth, State(state): State<Arc<AppState>>) -> Response {
    // Cascade: when deleting a project, delete all its child sites and datasets
    if ctx.type_name == "project" {
        if let Some(project_path) = Project::from_path(&ctx.id).and_then(|v| v.get("project").cloned())
        {
            let prefix = format!("{project_path}/");

            // Delete child datasets (which cascades to lake data)
            let datasets = state.schema.datasets().list_all();
            for (ds_id, _) in &datasets {
                if ds_id.starts_with(&prefix) {
                    if let Some(dataset_name) =
                        Dataset::from_path(ds_id).and_then(|v| v.get("name").cloned())
                    {
                        let qualified = format!("{project_path}/{dataset_name}");
                        let _ = state.adapter.delete_dataset(&qualified);
                    }
                    let _ = state.schema.datasets().delete(ds_id);
                }
            }

            // Delete child sites
            let sites = state.schema.sites().list_all();
            for (site_id, _) in &sites {
                if site_id.starts_with(&prefix) {
                    let _ = state.schema.sites().delete(site_id);
                }
            }
        }
    }

    // Cascade: when deleting a dataset, purge all its records from the lake
    if ctx.type_name == "dataset" {
        let ds_vars = Dataset::from_path(&ctx.id).unwrap_or_default();
        if let (Some(project_path), Some(dataset_name)) =
            (ds_vars.get("project"), ds_vars.get("name"))
        {
            let qualified = format!("{project_path}/{dataset_name}");
            if let Err(e) = state.adapter.delete_dataset(&qualified) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("failed to purge dataset records: {e}"),
                );
            }
        }
    }

    let result = match ctx.type_name.as_str() {
        "project" => state.schema.projects().delete(&ctx.id),
        "dataset" => state.schema.datasets().delete(&ctx.id),
        "site" => state.schema.sites().delete(&ctx.id),
        _ => return unknown_type(&ctx.type_name),
    };
    match result {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

