use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use loco_gen_schema::registry::SchemaRegistry;
use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter, Record, Value};

use crate::auth::{
    AuthAdapter, AuthenticatedUser, CreateUserRequest, LoginCredentials, UpdateUserRequest,
    auth_error_to_response,
};
use crate::auth::local::LocalAuthAdapter;

pub struct AppState {
    pub adapter: Box<dyn DataAdapter>,
    pub registry: SchemaRegistry,
    pub auth: Box<dyn AuthAdapter>,
}

pub struct SiteId(pub String);

/// Look up a site from the registry config by its site_id field (global lookup).
fn lookup_site(registry: &SchemaRegistry, site_id: &str) -> Option<HashMap<String, String>> {
    registry.find_config("site", "site_id", site_id)
        .map(|(_, fields)| fields)
}

/// Look up a site scoped to a namespace + site_id.
fn lookup_site_in_namespace(registry: &SchemaRegistry, namespace: &str, site_id: &str) -> Option<HashMap<String, String>> {
    let sites = registry.list_config("site");
    sites.into_iter().find(|(_, fields)| {
        fields.get("namespace").map(|n| n == namespace).unwrap_or(false)
            && fields.get("site_id").map(|s| s == site_id).unwrap_or(false)
    }).map(|(_, fields)| fields)
}

/// Resolve a qualified dataset ID from namespace + site name.
/// Returns `{namespace}/{dataset_name}` for use as the data lake tenant key.
fn resolve_dataset_id(registry: &SchemaRegistry, namespace: &str, site_name: &str) -> Result<String, String> {
    let site = lookup_site_in_namespace(registry, namespace, site_name)
        .ok_or_else(|| format!("unknown site: {site_name} in namespace {namespace}"))?;
    let dataset = site.get("dataset").filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .unwrap_or(site_name);
    Ok(format!("{namespace}/{dataset}"))
}

/// Extract namespace from a config ID like "projects/ben/pets/datasets/dev" → "ben/pets"
fn namespace_from_config_id(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("projects/")?;
    let end = rest.find("/datasets/")
        .or_else(|| rest.find("/sites/"))
        .or_else(|| rest.find("/project"))?;
    Some(&rest[..end])
}

impl FromRequestParts<Arc<AppState>> for SiteId {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 1. X-Site-Id header
        let site_id = parts
            .headers
            .get("x-site-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        // 2. ?site= query param (for browser testing)
        let site_id = site_id.or_else(|| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .find(|(k, _)| *k == "site")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| !v.is_empty())
            })
        });

        match site_id {
            Some(id) => {
                Ok(SiteId(id))
            }
            None => Err(error_response(
                StatusCode::BAD_REQUEST,
                "missing site: use X-Site-Id header or ?site= query param",
            )),
        }
    }
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn success(data: T) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
        })
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    let body = ApiResponse::<()> {
        ok: false,
        data: None,
        error: Some(msg.to_string()),
    };
    (status, Json(body)).into_response()
}

fn lake_error_to_response(err: loco_lake::Error) -> Response {
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

fn schema_error_to_response(err: loco_gen_schema::error::Error) -> Response {
    match &err {
        loco_gen_schema::error::Error::AlreadyExists(_) => {
            error_response(StatusCode::CONFLICT, &err.to_string())
        }
        loco_gen_schema::error::Error::NotFound(_) => {
            error_response(StatusCode::NOT_FOUND, &err.to_string())
        }
        _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

fn collection_key(user: &str, project: &str, name: &str) -> String {
    format!("{user}/{project}.{name}")
}

fn validate_collection(state: &AppState, key: &str) -> Result<(), Box<Response>> {
    if state.registry.has_instance("collection", key) {
        Ok(())
    } else {
        Err(Box::new(error_response(
            StatusCode::NOT_FOUND,
            &format!("unknown collection: {key}"),
        )))
    }
}

fn is_draft_version(version: &str) -> bool {
    version.contains('-')
}

fn require_draft(version: &str) -> Result<(), Response> {
    if is_draft_version(version) {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::BAD_REQUEST,
            &format!("version {version} is published and read-only"),
        ))
    }
}


// --- Data endpoints ---

#[derive(Deserialize)]
pub struct AddRecordRequest {
    fields: HashMap<String, Value>,
    #[serde(default)]
    owner: Option<String>,
}

async fn handle_add(
    site: SiteId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name)): Path<(String, String, String)>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let namespace = format!("{user}/{project}");
    let dataset_id = match resolve_dataset_id(&state.registry, &namespace, &site.0) {
        Ok(id) => id,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let owner = body.owner.unwrap_or_default();
    let record = Record {
        id: uuid::Uuid::new_v4().to_string(),
        dataset_id: Some(dataset_id.clone()),
        created_at: now.clone(),
        created_by: owner.clone(),
        updated_at: now,
        updated_by: owner.clone(),
        owner,
        fields: body.fields,
    };

    match state.adapter.insert(&dataset_id, &key, record) {
        Ok(rec) => (StatusCode::CREATED, ApiResponse::success(rec)).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_list(
    site: SiteId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name)): Path<(String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let namespace = format!("{user}/{project}");
    let dataset_id = match resolve_dataset_id(&state.registry, &namespace, &site.0) {
        Ok(id) => id,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };
    match state.adapter.list(&dataset_id, &key) {
        Ok(records) => ApiResponse::success(records).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_get(
    site: SiteId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name, id)): Path<(String, String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let namespace = format!("{user}/{project}");
    let dataset_id = match resolve_dataset_id(&state.registry, &namespace, &site.0) {
        Ok(id) => id,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };
    match state.adapter.get(&dataset_id, &key, &id) {
        Ok(Some(record)) => ApiResponse::success(record).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_delete(
    site: SiteId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name, id)): Path<(String, String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let namespace = format!("{user}/{project}");
    let dataset_id = match resolve_dataset_id(&state.registry, &namespace, &site.0) {
        Ok(id) => id,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };
    match state.adapter.delete(&dataset_id, &key, &id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_update(
    site: SiteId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name, id)): Path<(String, String, String, String)>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let namespace = format!("{user}/{project}");
    let dataset_id = match resolve_dataset_id(&state.registry, &namespace, &site.0) {
        Ok(id) => id,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };

    // Get existing record to preserve metadata
    let existing = match state.adapter.get(&dataset_id, &key, &id) {
        Ok(Some(r)) => r,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => return lake_error_to_response(e),
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut fields = existing.fields;
    for (k, v) in body.fields {
        fields.insert(k, v);
    }

    let record = Record {
        id: existing.id,
        dataset_id: existing.dataset_id,
        created_at: existing.created_at,
        created_by: existing.created_by,
        updated_at: now,
        updated_by: body.owner.unwrap_or(existing.updated_by),
        owner: existing.owner,
        fields,
    };

    match state.adapter.update(&dataset_id, &key, &id, record) {
        Ok(rec) => ApiResponse::success(rec).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

// --- Meta endpoint ---

async fn handle_meta_list(
    State(state): State<Arc<AppState>>,
    Path((user, project, type_name)): Path<(String, String, String)>,
) -> Response {
    let entries = state.registry.list_instances(&type_name, &user, &project);
    ApiResponse::success(entries).into_response()
}

// --- Schema CRUD endpoints ---

#[derive(Deserialize)]
struct CreateCollectionRequest {
    name: String,
    label: String,
    label_plural: String,
}

#[derive(Deserialize)]
struct UpdateCollectionRequest {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    label_plural: Option<String>,
}

#[derive(Deserialize)]
struct CreateFieldRequest {
    name: String,
    r#type: String,
}

#[derive(Deserialize)]
struct UpdateFieldRequest {
    #[serde(default)]
    r#type: Option<String>,
}

async fn handle_schema_create_collection(
    State(state): State<Arc<AppState>>,
    Path((user, project, version)): Path<(String, String, String)>,
    Json(body): Json<CreateCollectionRequest>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    let mut fields = HashMap::new();
    fields.insert("name".to_string(), body.name.clone());
    fields.insert("label".to_string(), body.label);
    fields.insert("label_plural".to_string(), body.label_plural);

    match state
        .registry
        .create_instance("collection", &user, &project, &version, &body.name, fields)
    {
        Ok(result) => (StatusCode::CREATED, ApiResponse::success(result)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_schema_list_collections(
    State(state): State<Arc<AppState>>,
    Path((user, project, _version)): Path<(String, String, String)>,
) -> Response {
    let entries = state.registry.list_instances("collection", &user, &project);
    ApiResponse::success(entries).into_response()
}

async fn handle_schema_get_collection(
    State(state): State<Arc<AppState>>,
    Path((user, project, _version, name)): Path<(String, String, String, String)>,
) -> Response {
    let namespace = format!("{user}/{project}.{name}");
    match state.registry.get_instance("collection", &namespace) {
        Some(fields) => ApiResponse::success(fields).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("collection not found: {name}")),
    }
}

async fn handle_schema_update_collection(
    State(state): State<Arc<AppState>>,
    Path((user, project, version, name)): Path<(String, String, String, String)>,
    Json(body): Json<UpdateCollectionRequest>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    let mut fields = HashMap::new();
    if let Some(label) = body.label {
        fields.insert("label".to_string(), label);
    }
    if let Some(label_plural) = body.label_plural {
        fields.insert("label_plural".to_string(), label_plural);
    }

    match state
        .registry
        .update_instance("collection", &user, &project, &version, &name, fields)
    {
        Ok(result) => ApiResponse::success(result).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_schema_delete_collection(
    State(state): State<Arc<AppState>>,
    Path((user, project, version, name)): Path<(String, String, String, String)>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    // First delete all fields belonging to this collection
    let prefix = format!("{user}/{project}.{name}/");
    let _ = state
        .registry
        .delete_instances_by_prefix("field", &prefix, &version);

    match state
        .registry
        .delete_instance("collection", &user, &project, &version, &name)
    {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_schema_create_field(
    State(state): State<Arc<AppState>>,
    Path((user, project, version, collection)): Path<(String, String, String, String)>,
    Json(body): Json<CreateFieldRequest>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    let mut fields = HashMap::new();
    fields.insert("name".to_string(), body.name.clone());
    fields.insert("collection".to_string(), collection.clone());
    fields.insert("type".to_string(), body.r#type);

    match state.registry.create_nested_instance(
        "field",
        &user,
        &project,
        &version,
        &collection,
        &body.name,
        fields,
    ) {
        Ok(result) => (StatusCode::CREATED, ApiResponse::success(result)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_schema_list_fields(
    State(state): State<Arc<AppState>>,
    Path((user, project, _version, collection)): Path<(String, String, String, String)>,
) -> Response {
    let all_fields = state.registry.list_instances("field", &user, &project);
    let prefix = format!("{user}/{project}.{collection}/");
    let filtered: Vec<_> = all_fields
        .into_iter()
        .filter(|(ns, _)| ns.starts_with(&prefix))
        .collect();
    ApiResponse::success(filtered).into_response()
}

async fn handle_schema_update_field(
    State(state): State<Arc<AppState>>,
    Path((user, project, version, collection, name)): Path<(String, String, String, String, String)>,
    Json(body): Json<UpdateFieldRequest>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    let mut fields = HashMap::new();
    if let Some(r#type) = body.r#type {
        fields.insert("type".to_string(), r#type);
    }

    match state.registry.update_nested_instance(
        "field",
        &user,
        &project,
        &version,
        &collection,
        &name,
        fields,
    ) {
        Ok(result) => ApiResponse::success(result).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_schema_delete_field(
    State(state): State<Arc<AppState>>,
    Path((user, project, version, collection, name)): Path<(String, String, String, String, String)>,
) -> Response {
    if let Err(resp) = require_draft(&version) {
        return resp;
    }

    let namespace = format!("{user}/{project}.{collection}/{name}");

    // Delete from registry (handles both disk and memory)
    {
        let instances = state.registry.get_instance("field", &namespace);
        if instances.is_none() {
            return error_response(StatusCode::NOT_FOUND, &format!("field not found: {collection}/{name}"));
        }
    }

    // Delete from disk and in-memory state via the registry
    let _ = state
        .registry
        .delete_instances_by_prefix("field", &namespace, &version);

    ApiResponse::success("deleted").into_response()
}

// --- Schema introspection ---

#[derive(Serialize)]
struct CollectionWithFields {
    name: String,
    fields: HashMap<String, String>,
    collection_fields: Vec<(String, HashMap<String, String>)>,
}

#[derive(Serialize)]
struct NamespaceCollections {
    namespace: String,
    collections: Vec<CollectionWithFields>,
}

async fn handle_schema_introspect(
    site: SiteId,
    State(state): State<Arc<AppState>>,
) -> Response {
    let site_fields = match lookup_site(&state.registry, &site.0) {
        Some(f) => f,
        None => return error_response(StatusCode::NOT_FOUND, "site not found"),
    };

    let namespace = match site_fields.get("namespace") {
        Some(ns) if !ns.is_empty() => ns.clone(),
        _ => return error_response(StatusCode::BAD_REQUEST, "site has no namespace configured"),
    };
    let version = match site_fields.get("version") {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return error_response(StatusCode::BAD_REQUEST, "site has no version configured"),
    };
    let namespace_str = format!("{namespace}@{version}");

    // Resolve the full dependency tree
    let ns_pairs = match state.registry.resolve_namespace_tree(&namespace_str) {
        Ok(pairs) => pairs,
        Err(e) => return schema_error_to_response(e),
    };

    // For each namespace, gather collections and their fields
    let mut result: Vec<NamespaceCollections> = Vec::new();
    for (user, project) in &ns_pairs {
        let collections = state.registry.list_instances("collection", user, project);
        let all_fields = state.registry.list_instances("field", user, project);

        let mut coll_with_fields: Vec<CollectionWithFields> = Vec::new();
        for (col_ns, col_fields) in &collections {
            let col_name = col_ns.split_once('.').map(|(_, n)| n).unwrap_or("");
            let field_prefix = format!("{col_ns}/");
            let matching_fields: Vec<_> = all_fields
                .iter()
                .filter(|(ns, _)| ns.starts_with(&field_prefix))
                .map(|(ns, f)| (ns.clone(), f.clone()))
                .collect();

            coll_with_fields.push(CollectionWithFields {
                name: col_name.to_string(),
                fields: col_fields.clone(),
                collection_fields: matching_fields,
            });
        }

        result.push(NamespaceCollections {
            namespace: format!("{user}/{project}"),
            collections: coll_with_fields,
        });
    }

    ApiResponse::success(result).into_response()
}

// --- Config endpoints ---

#[derive(Deserialize)]
struct CreateConfigRequest {
    fields: HashMap<String, String>,
}

async fn handle_config_list(
    State(state): State<Arc<AppState>>,
    Path(type_name): Path<String>,
) -> Response {
    let entries = state.registry.list_config(&type_name);
    ApiResponse::success(entries).into_response()
}

/// Extract the config type and ID from a wildcard path like "project/projects/ben/crm/project".
/// Returns (type_name, id) where type_name is the first segment and id is the rest.
fn split_config_path(path: &str) -> Option<(String, String)> {
    let (type_name, id) = path.split_once('/')?;
    if type_name.is_empty() || id.is_empty() {
        return None;
    }
    Some((type_name.to_string(), id.to_string()))
}

async fn handle_config_get(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    let Some((type_name, id)) = split_config_path(&path) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid config path");
    };
    match state.registry.get_config(&type_name, &id) {
        Some(fields) => ApiResponse::success(fields).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("{type_name} not found: {id}")),
    }
}

async fn handle_config_create(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(body): Json<CreateConfigRequest>,
) -> Response {
    let Some((type_name, id)) = split_config_path(&path) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid config path");
    };
    let result = match state.registry.create_config(&type_name, &id, body.fields) {
        Ok(r) => r,
        Err(e) => return schema_error_to_response(e),
    };

    // When a project is created, bootstrap a default "dev" site and dataset
    if type_name == "project" {
        if let (Some(namespace), Some(name)) = (result.get("namespace"), result.get("name")) {
            let project_slug = namespace.split('/').last().unwrap_or("project");

            let mut dataset_fields = HashMap::new();
            dataset_fields.insert("dataset_id".to_string(), "dev".to_string());
            dataset_fields.insert("project".to_string(), project_slug.to_string());
            dataset_fields.insert("name".to_string(), format!("{name} Dev"));
            dataset_fields.insert("description".to_string(), "Default development dataset".to_string());
            let dataset_config_id = format!("projects/{namespace}/datasets/dev");
            let _ = state.registry.create_config("dataset", &dataset_config_id, dataset_fields);

            let mut site_fields = HashMap::new();
            site_fields.insert("site_id".to_string(), "dev".to_string());
            site_fields.insert("project".to_string(), project_slug.to_string());
            site_fields.insert("name".to_string(), format!("{name} Dev"));
            site_fields.insert("namespace".to_string(), namespace.clone());
            site_fields.insert("version".to_string(), "0.0.1-dev".to_string());
            site_fields.insert("dataset".to_string(), "dev".to_string());
            let site_config_id = format!("projects/{namespace}/sites/dev");
            let _ = state.registry.create_config("site", &site_config_id, site_fields);
        }
    }

    (StatusCode::CREATED, ApiResponse::success(result)).into_response()
}

async fn handle_config_update(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(body): Json<CreateConfigRequest>,
) -> Response {
    let Some((type_name, id)) = split_config_path(&path) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid config path");
    };
    match state.registry.update_config(&type_name, &id, body.fields) {
        Ok(result) => ApiResponse::success(result).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

async fn handle_config_delete(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Response {
    let Some((type_name, id)) = split_config_path(&path) else {
        return error_response(StatusCode::BAD_REQUEST, "invalid config path");
    };

    // Cascade: when deleting a project, delete all its child sites and datasets
    if type_name == "project" {
        // Project ID is like "projects/ben/crm/project" → prefix is "projects/ben/crm/"
        let prefix = id.trim_end_matches("project");
        let namespace = prefix.strip_prefix("projects/").unwrap_or("").trim_end_matches('/');

        // Delete child datasets (which cascades to lake data)
        let datasets = state.registry.list_config("dataset");
        for (ds_id, ds_fields) in &datasets {
            if ds_id.starts_with(prefix) {
                if let Some(dataset_name) = ds_fields.get("dataset_id") {
                    let qualified = format!("{namespace}/{dataset_name}");
                    let _ = state.adapter.delete_dataset(&qualified);
                }
                let _ = state.registry.delete_config("dataset", ds_id);
            }
        }

        // Delete child sites
        let sites = state.registry.list_config("site");
        for (site_id, _) in &sites {
            if site_id.starts_with(prefix) {
                let _ = state.registry.delete_config("site", site_id);
            }
        }
    }

    // Cascade: when deleting a dataset, purge all its records from the lake
    if type_name == "dataset" {
        if let Some(fields) = state.registry.get_config(&type_name, &id) {
            if let Some(dataset_name) = fields.get("dataset_id") {
                let namespace = namespace_from_config_id(&id).unwrap_or("");
                let qualified = format!("{namespace}/{dataset_name}");
                if let Err(e) = state.adapter.delete_dataset(&qualified) {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("failed to purge dataset records: {e}"),
                    );
                }
            }
        }
    }

    match state.registry.delete_config(&type_name, &id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- Auth endpoints ---

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    site_id: String,
}

async fn handle_auth_login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Response {
    if lookup_site(&state.registry, &body.site_id).is_none() {
        return error_response(StatusCode::BAD_REQUEST, &format!("unknown site: {}", body.site_id));
    }

    let credentials = LoginCredentials {
        username: body.username,
        password: None,
        site_id: body.site_id.clone(),
    };

    match state.auth.login(&body.site_id, &credentials) {
        Ok(session) => ApiResponse::success(session).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_logout(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.auth.logout(&user.0.token) {
        Ok(()) => ApiResponse::success("logged out").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_me(user: AuthenticatedUser) -> Response {
    ApiResponse::success(user.0.user).into_response()
}

#[derive(Deserialize)]
struct CreateUserHttpRequest {
    username: String,
    name: String,
    #[serde(default)]
    role: Option<String>,
}

async fn handle_auth_create_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateUserHttpRequest>,
) -> Response {
    let req = CreateUserRequest {
        username: body.username,
        name: body.name,
        role: body.role,
        password: None,
    };

    match state.auth.create_user(&user.0.user.site_id, &req) {
        Ok(new_user) => (StatusCode::CREATED, ApiResponse::success(new_user)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_list_users(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.auth.list_users(&user.0.user.site_id) {
        Ok(users) => ApiResponse::success(users).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

#[derive(Deserialize)]
struct UpdateUserHttpRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

async fn handle_auth_update_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserHttpRequest>,
) -> Response {
    let updates = UpdateUserRequest {
        name: body.name,
        role: body.role,
        status: body.status,
    };

    match state.auth.update_user(&user.0.user.site_id, &id, &updates) {
        Ok(updated) => ApiResponse::success(updated).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_delete_user(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.auth.delete_user(&user.0.user.site_id, &id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

#[derive(Deserialize)]
struct CreateApiKeyRequest {
    label: String,
}

async fn handle_auth_create_api_key(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateApiKeyRequest>,
) -> Response {
    match state
        .auth
        .create_api_key(&user.0.user.site_id, &user.0.user.id, &body.label)
    {
        Ok(key) => (StatusCode::CREATED, ApiResponse::success(key)).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_list_api_keys(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state
        .auth
        .list_api_keys(&user.0.user.site_id, &user.0.user.id)
    {
        Ok(keys) => ApiResponse::success(keys).into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

async fn handle_auth_revoke_api_key(
    user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.auth.revoke_api_key(&user.0.user.site_id, &id) {
        Ok(()) => ApiResponse::success("revoked").into_response(),
        Err(e) => auth_error_to_response(e),
    }
}

// --- App setup ---

fn build_adapter() -> Box<dyn DataAdapter> {
    let adapter_type = std::env::var("LOCO_ADAPTER").unwrap_or_else(|_| "sqlite".to_string());
    match adapter_type.as_str() {
        "memory" => {
            println!("Using in-memory adapter");
            Box::new(InMemoryAdapter::new())
        }
        "sqlite" => {
            let path = std::env::var("LOCO_DB_PATH").unwrap_or_else(|_| "loco.db".to_string());
            println!("Using SQLite adapter ({path})");
            Box::new(
                SqliteAdapter::new(std::path::Path::new(&path))
                    .expect("failed to open SQLite database"),
            )
        }
        other => panic!("unknown LOCO_ADAPTER: {other} (expected \"sqlite\" or \"memory\")"),
    }
}

pub fn build_app() -> Router {
    build_app_with_root(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
}

pub fn build_app_with_root(root: &std::path::Path) -> Router {
    let crate_dir = root;

    // Load type definitions for runtime schema loading
    let types_dir = crate_dir.join("schemas/types");
    let mut type_defs = Vec::new();
    if types_dir.exists() {
        for entry in std::fs::read_dir(&types_dir).expect("failed to read schemas/types") {
            let entry = entry.expect("failed to read type entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                let schema = loco_gen_schema::parser::parse_schema_file(&path)
                    .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
                type_defs.push(schema.type_def);
            }
        }
    }

    // Load schema registry from disk
    let instances_dir = crate_dir.join("schemas/instances");
    let config_dir = crate_dir.join("schemas/config");
    let registry = SchemaRegistry::load(&instances_dir, &config_dir, &type_defs)
        .expect("failed to load schema registry");

    let all_collections = registry.list_instances("collection", "", "");
    for (ns, _) in &all_collections {
        println!("  - {ns}");
    }

    let all_fields = registry.list_instances("field", "", "");
    println!("Loaded {} field(s):", all_fields.len());
    for (ns, _) in &all_fields {
        println!("  - {ns}");
    }

    let all_sites = registry.list_config("site");
    println!("Loaded {} site(s):", all_sites.len());
    for (id, _) in &all_sites {
        println!("  - {id}");
    }

    // Initialize data adapter
    let adapter = build_adapter();

    // Initialize auth adapter
    let auth_adapter: Box<dyn AuthAdapter> =
        Box::new(LocalAuthAdapter::new(&crate_dir.join("auth")));
    println!("Using local filesystem auth adapter (auth/)");
    println!("Sites are managed in schemas/config/site/");

    let state = Arc::new(AppState {
        adapter,
        registry,
        auth: auth_adapter,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Data endpoints
        .route(
            "/{user}/{project}/collection/{name}/add",
            post(handle_add),
        )
        .route(
            "/{user}/{project}/collection/{name}/list",
            get(handle_list),
        )
        .route(
            "/{user}/{project}/collection/{name}/get/{id}",
            get(handle_get),
        )
        .route(
            "/{user}/{project}/collection/{name}/update/{id}",
            put(handle_update),
        )
        .route(
            "/{user}/{project}/collection/{name}/delete/{id}",
            delete(handle_delete),
        )
        // Meta endpoint
        .route(
            "/meta/{user}/{project}/{type_name}/list",
            get(handle_meta_list),
        )
        // Schema CRUD endpoints
        .route(
            "/schema/{user}/{project}/{version}/collection",
            post(handle_schema_create_collection),
        )
        .route(
            "/schema/{user}/{project}/{version}/collection/list",
            get(handle_schema_list_collections),
        )
        .route(
            "/schema/{user}/{project}/{version}/collection/{name}",
            get(handle_schema_get_collection)
                .put(handle_schema_update_collection)
                .delete(handle_schema_delete_collection),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}",
            post(handle_schema_create_field),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}/list",
            get(handle_schema_list_fields),
        )
        .route(
            "/schema/{user}/{project}/{version}/field/{collection}/{name}",
            put(handle_schema_update_field).delete(handle_schema_delete_field),
        )
        // Schema introspection
        .route("/schema/collections", get(handle_schema_introspect))
        // Config CRUD endpoints (global-scope types)
        .route("/config/{type_name}/list", get(handle_config_list))
        .route("/config/get/{*path}", get(handle_config_get))
        .route("/config/create/{*path}", post(handle_config_create))
        .route("/config/update/{*path}", put(handle_config_update))
        .route("/config/delete/{*path}", delete(handle_config_delete))
        // Auth endpoints
        .route("/auth/login", post(handle_auth_login))
        .route("/auth/logout", post(handle_auth_logout))
        .route("/auth/me", get(handle_auth_me))
        .route("/auth/users", post(handle_auth_create_user))
        .route("/auth/users/list", get(handle_auth_list_users))
        .route(
            "/auth/users/{id}",
            put(handle_auth_update_user).delete(handle_auth_delete_user),
        )
        .route("/auth/api-keys", post(handle_auth_create_api_key))
        .route("/auth/api-keys/list", get(handle_auth_list_api_keys))
        .route("/auth/api-keys/{id}", delete(handle_auth_revoke_api_key))
        .layer(cors)
        .with_state(state)
}
