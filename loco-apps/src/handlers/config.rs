use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::http::authz::require_config_site;
use crate::http::extract::ConfigAuth;
use crate::http::response::{ApiResponse, error_response, schema_error_to_response};
use crate::server::AppState;
use crate::{Dataset, Project, Site};

#[derive(Deserialize)]
pub struct CreateConfigRequest {
    fields: HashMap<String, String>,
}

pub async fn list(
    auth_user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(type_name): Path<String>,
) -> Response {
    if let Err(resp) = require_config_site(&auth_user.0.user) {
        return resp;
    }
    let username = &auth_user.0.user.username;
    let entries = state.schema.list_all(&type_name);
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|(id, _)| id.starts_with(&format!("{username}/")))
        .collect();
    ApiResponse::success(filtered).into_response()
}

pub async fn get(ctx: ConfigAuth, State(state): State<Arc<AppState>>) -> Response {
    match state.schema.get(&ctx.type_name, &ctx.id) {
        Some(fields) => ApiResponse::success(fields).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("{} not found: {}", ctx.type_name, ctx.id),
        ),
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
        _ => None,
    }
    .unwrap_or_default();

    let mut fields = body.fields;
    for (k, v) in &template_vars {
        fields.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let result = match state.schema.create(&ctx.type_name, &ctx.id, fields) {
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

            let mut dataset_fields = HashMap::new();
            dataset_fields.insert("label".to_string(), format!("{label} Dev"));
            dataset_fields.insert(
                "description".to_string(),
                "Default development dataset".to_string(),
            );
            let _ = state
                .schema
                .datasets()
                .create(&Dataset::to_path(project_path, "dev"), dataset_fields);

            let mut site_fields = HashMap::new();
            site_fields.insert("label".to_string(), format!("{label} Dev"));
            site_fields.insert("version".to_string(), "0.0.1-dev".to_string());
            site_fields.insert("dataset".to_string(), "dev".to_string());
            let _ = state
                .schema
                .sites()
                .create(&Site::to_path(project_path, "dev"), site_fields);
        }
    }

    (StatusCode::CREATED, ApiResponse::success(result)).into_response()
}

pub async fn update(
    ctx: ConfigAuth,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateConfigRequest>,
) -> Response {
    match state.schema.update(&ctx.type_name, &ctx.id, body.fields) {
        Ok(result) => ApiResponse::success(result).into_response(),
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

    match state.schema.delete(&ctx.type_name, &ctx.id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

