use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter, Record, Value};

use crate::{Collection, Field};

type MetaEntries = Vec<(String, HashMap<String, String>)>;

#[derive(Debug, Deserialize)]
pub struct TenantConfig {
    pub name: String,
}

pub struct AppState {
    pub adapter: Box<dyn DataAdapter>,
    pub collections: HashSet<String>,
    pub meta: HashMap<String, MetaEntries>,
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

fn collection_key(user: &str, project: &str, name: &str) -> String {
    format!("{user}/{project}.{name}")
}

fn validate_collection(state: &AppState, key: &str) -> Result<(), Box<Response>> {
    if state.collections.contains(key) {
        Ok(())
    } else {
        Err(Box::new(error_response(
            StatusCode::NOT_FOUND,
            &format!("unknown collection: {key}"),
        )))
    }
}

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

async fn handle_meta_list(
    State(state): State<Arc<AppState>>,
    Path((user, project, type_name)): Path<(String, String, String)>,
) -> Response {
    let Some(entries) = state.meta.get(&type_name) else {
        return error_response(StatusCode::NOT_FOUND, &format!("unknown type: {type_name}"));
    };

    let prefix = format!("{user}/{project}.");
    let filtered: Vec<_> = entries
        .iter()
        .filter(|(ns, _)| ns.starts_with(&prefix))
        .collect();

    ApiResponse::success(filtered).into_response()
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

    let instances = Collection::load_all_instances();
    let collections: HashSet<String> = instances.iter().map(|(ns, _)| ns.to_string()).collect();

    println!("Loaded {} collection(s):", collections.len());
    for ns in &collections {
        println!("  - {ns}");
    }

    let field_instances = Field::load_all_instances();
    println!("Loaded {} field(s):", field_instances.len());
    for (ns, _) in &field_instances {
        println!("  - {ns}");
    }

    let mut meta: HashMap<String, MetaEntries> = HashMap::new();

    meta.insert(
        "collection".to_string(),
        instances
            .iter()
            .map(|(ns, inst)| {
                let mut fields = HashMap::new();
                fields.insert("name".to_string(), inst.name().to_string());
                fields.insert("label".to_string(), inst.label().to_string());
                fields.insert("label_plural".to_string(), inst.label_plural().to_string());
                (ns.to_string(), fields)
            })
            .collect(),
    );

    meta.insert(
        "field".to_string(),
        field_instances
            .iter()
            .map(|(ns, inst)| {
                let mut fields = HashMap::new();
                fields.insert("name".to_string(), inst.name().to_string());
                fields.insert("collection".to_string(), inst.collection().to_string());
                fields.insert("type".to_string(), inst.r#type().to_string());
                (ns.to_string(), fields)
            })
            .collect(),
    );

    let state = Arc::new(AppState {
        adapter: build_adapter(),
        collections,
        meta,
        tenants,
    });

    Router::new()
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
        .route(
            "/meta/{user}/{project}/{type_name}/list",
            get(handle_meta_list),
        )
        .with_state(state)
}
