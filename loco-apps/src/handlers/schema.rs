use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AuthenticatedUser;
use crate::http::authz::authorize_user;
use crate::http::response::{ApiResponse, error_response, schema_error_to_response};
use crate::http::scope::{require_metadata_editor, SiteScope, VersionScope};
use crate::server::AppState;
use crate::{Collection, CollectionUpdate, Field, FieldUpdate};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{get, post};
    Router::new()
        .route("/{user}/{project}/{version}/collection", post(create_collection))
        .route(
            "/{user}/{project}/{version}/collection/list",
            get(list_collections),
        )
        .route(
            "/{user}/{project}/{version}/collection/{name}",
            get(get_collection)
                .put(update_collection)
                .delete(delete_collection),
        )
        .route(
            "/{user}/{project}/{version}/field/{collection}",
            post(create_field),
        )
        .route(
            "/{user}/{project}/{version}/field/{collection}/list",
            get(list_fields),
        )
        .route(
            "/{user}/{project}/{version}/field/{collection}/{name}",
            axum::routing::put(update_field).delete(delete_field),
        )
        .route("/collections", get(introspect))
}

#[derive(Deserialize)]
pub struct CreateCollectionRequest {
    name: String,
    label: String,
    label_plural: String,
}

#[derive(Deserialize)]
pub struct CreateFieldRequest {
    name: String,
    r#type: String,
}

pub async fn create_collection(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateCollectionRequest>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    let value = Collection::new(
        scope.project_id(),
        scope.version.clone(),
        body.name,
        body.label,
        body.label_plural,
    );

    match state.schema.collections().create(value) {
        Ok(result) => (StatusCode::CREATED, ApiResponse::success(result)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_collections(scope: VersionScope, State(state): State<Arc<AppState>>) -> Response {
    let entries = state
        .schema
        .collections()
        .list(&format!("{}/", scope.project_id()));
    ApiResponse::success(entries).into_response()
}

pub async fn get_collection(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    let key = Collection::to_path(&scope.project_id(), &scope.version, &name);
    match state.schema.collections().get(&key) {
        Some(c) => ApiResponse::success(c).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("collection not found: {name}")),
    }
}

pub async fn update_collection(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
    Json(patch): Json<CollectionUpdate>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    let key = Collection::to_path(&scope.project_id(), &scope.version, &name);
    match state.schema.collections().update(&key, patch) {
        Ok(result) => ApiResponse::success(result).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_collection(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    // First delete all fields belonging to this collection
    let field_prefix = format!(
        "{}/versions/{}/fields/{}/",
        scope.project_id(),
        scope.version,
        name
    );
    let _ = state.schema.fields().delete_by_prefix(&field_prefix);

    let key = Collection::to_path(&scope.project_id(), &scope.version, &name);
    match state.schema.collections().delete(&key) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn create_field(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, collection)): Path<(String, String, String, String)>,
    Json(body): Json<CreateFieldRequest>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    let value = Field::new(
        scope.project_id(),
        scope.version.clone(),
        collection,
        body.name,
        body.r#type,
    );

    match state.schema.fields().create(value) {
        Ok(result) => (StatusCode::CREATED, ApiResponse::success(result)).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn list_fields(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, collection)): Path<(String, String, String, String)>,
) -> Response {
    let prefix = format!(
        "{}/versions/{}/fields/{}/",
        scope.project_id(),
        scope.version,
        collection
    );
    let entries = state.schema.fields().list(&prefix);
    ApiResponse::success(entries).into_response()
}

pub async fn update_field(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<FieldUpdate>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    let key = Field::to_path(&scope.project_id(), &scope.version, &collection, &name);
    match state.schema.fields().update(&key, patch) {
        Ok(result) => ApiResponse::success(result).into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

pub async fn delete_field(
    scope: VersionScope,
    State(state): State<Arc<AppState>>,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
) -> Response {
    if let Err(resp) = scope.require_draft() {
        return resp;
    }

    let key = Field::to_path(&scope.project_id(), &scope.version, &collection, &name);
    match state.schema.fields().delete(&key) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => schema_error_to_response(e),
    }
}

// --- Schema introspection ---

#[derive(Serialize)]
struct CollectionWithFields {
    name: String,
    fields: Arc<Collection>,
    collection_fields: Vec<(String, Arc<Field>)>,
}

#[derive(Serialize)]
struct NamespaceCollections {
    namespace: String,
    collections: Vec<CollectionWithFields>,
}

pub async fn introspect(
    scope: SiteScope,
    auth_user: AuthenticatedUser,
    State(state): State<Arc<AppState>>,
) -> Response {
    if let Err(resp) = require_metadata_editor(&auth_user.0.user.site_id) {
        return resp;
    }
    if let Err(resp) = authorize_user(&auth_user.0.user, &scope.project.user) {
        return resp;
    }

    let version_scope = scope.version_scope();
    if version_scope.version.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "site has no version configured");
    }

    let namespace_str = format!("{}@{}", scope.project.project_id(), version_scope.version);
    let ns_pairs = match crate::manifest::resolve_dependency_tree(&state.schema, &namespace_str) {
        Ok(pairs) => pairs,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let mut result: Vec<NamespaceCollections> = Vec::new();
    for (user, project) in &ns_pairs {
        let collections = state
            .schema
            .collections()
            .list(&format!("{user}/{project}/"));
        let all_fields = state.schema.fields().list(&format!("{user}/{project}/"));

        let mut coll_with_fields: Vec<CollectionWithFields> = Vec::new();
        for (col_ns, col) in &collections {
            let col_name = col.name();
            let version_prefix = col_ns.split("/collections/").next().unwrap_or("");
            let field_prefix = format!("{version_prefix}/fields/{col_name}/");
            let matching_fields: Vec<_> = all_fields
                .iter()
                .filter(|(ns, _)| ns.starts_with(&field_prefix))
                .map(|(ns, f)| (ns.clone(), f.clone()))
                .collect();

            coll_with_fields.push(CollectionWithFields {
                name: col_name.to_string(),
                fields: col.clone(),
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
