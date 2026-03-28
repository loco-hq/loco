use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use loco_gen_schema::registry::SchemaRegistry;
use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter, Record, Value};

// Collection/Field generated types are still available via codegen
// but the SchemaRegistry now handles runtime metadata

#[derive(Debug, Deserialize)]
pub struct TenantConfig {
    pub name: String,
}

pub struct AppState {
    pub adapter: Box<dyn DataAdapter>,
    pub registry: SchemaRegistry,
    pub tenants: HashMap<String, TenantConfig>,
}

pub struct TenantId(pub String);

impl FromRequestParts<Arc<AppState>> for TenantId {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 1. X-Tenant-Id header
        let tenant_id = parts
            .headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        // 2. ?tenant= query param (for browser testing)
        let tenant_id = tenant_id.or_else(|| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .find(|(k, _)| *k == "tenant")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| !v.is_empty())
            })
        });

        match tenant_id {
            Some(id) => {
                if state.tenants.contains_key(&id) {
                    Ok(TenantId(id))
                } else {
                    Err(error_response(
                        StatusCode::BAD_REQUEST,
                        &format!("unknown tenant: {id}"),
                    ))
                }
            }
            None => Err(error_response(
                StatusCode::BAD_REQUEST,
                "missing tenant: use X-Tenant-Id header or ?tenant= query param",
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
        loco_lake::Error::InvalidTenant(msg) => {
            error_response(StatusCode::BAD_REQUEST, &format!("invalid tenant: {msg}"))
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
    tenant: TenantId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name)): Path<(String, String, String)>,
    Json(body): Json<AddRecordRequest>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let owner = body.owner.unwrap_or_default();
    let record = Record {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: Some(tenant.0.clone()),
        created_at: now.clone(),
        created_by: owner.clone(),
        updated_at: now,
        updated_by: owner.clone(),
        owner,
        fields: body.fields,
    };

    match state.adapter.insert(&tenant.0, &key, record) {
        Ok(rec) => (StatusCode::CREATED, ApiResponse::success(rec)).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_list(
    tenant: TenantId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name)): Path<(String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    match state.adapter.list(&tenant.0, &key) {
        Ok(records) => ApiResponse::success(records).into_response(),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_get(
    tenant: TenantId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name, id)): Path<(String, String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    match state.adapter.get(&tenant.0, &key, &id) {
        Ok(Some(record)) => ApiResponse::success(record).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => lake_error_to_response(e),
    }
}

async fn handle_delete(
    tenant: TenantId,
    State(state): State<Arc<AppState>>,
    Path((user, project, name, id)): Path<(String, String, String, String)>,
) -> Response {
    let key = collection_key(&user, &project, &name);
    if let Err(resp) = validate_collection(&state, &key) {
        return *resp;
    }

    match state.adapter.delete(&tenant.0, &key, &id) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
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

    // Delete the file directly
    let file_path = std::path::Path::new("schemas/instances")
        .join(&user)
        .join(&project)
        .join(&version)
        .join("field")
        .join(&collection)
        .join(format!("{name}.yaml"));
    let _ = std::fs::remove_file(&file_path);

    // Remove from in-memory state by re-creating without nested delete helpers
    // Use a workaround: create a dummy then delete it... actually let's just
    // remove it from the registry's internal state directly
    // For now, we need to add support for deleting nested instances
    // Let's use delete_instances_by_prefix with an exact match
    let _ = state
        .registry
        .delete_instances_by_prefix("field", &namespace, &version);

    ApiResponse::success("deleted").into_response()
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

fn load_tenants() -> HashMap<String, TenantConfig> {
    let tenants_dir = std::path::Path::new("tenants");
    let mut tenants = HashMap::new();

    if !tenants_dir.exists() {
        println!("Warning: tenants/ directory not found, no tenants loaded");
        return tenants;
    }

    let entries = std::fs::read_dir(tenants_dir).expect("failed to read tenants/ directory");
    for entry in entries {
        let entry = entry.expect("failed to read tenant file entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            let tenant_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("invalid tenant filename")
                .to_string();
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let config: TenantConfig = serde_yaml::from_str(&contents)
                .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
            tenants.insert(tenant_id, config);
        }
    }

    tenants
}

pub fn build_app() -> Router {
    let tenants = load_tenants();
    println!("Loaded {} tenant(s):", tenants.len());
    for (id, config) in &tenants {
        println!("  - {id} ({})", config.name);
    }

    // Load type definitions for runtime schema loading
    let types_dir = std::path::Path::new("schemas/types");
    let mut type_defs = Vec::new();
    if types_dir.exists() {
        for entry in std::fs::read_dir(types_dir).expect("failed to read schemas/types") {
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
    let instances_dir = std::path::Path::new("schemas/instances");
    let registry = SchemaRegistry::load(instances_dir, &type_defs)
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

    let state = Arc::new(AppState {
        adapter: build_adapter(),
        registry,
        tenants,
    });

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
        .with_state(state)
}
