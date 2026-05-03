use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AuthenticatedUser;
use crate::http::authz::authorize_user;
use crate::http::response::{
    error_response, version_schema_error_to_response, ApiResponse,
};
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
    Json(body): Json<CreateCollectionRequest>,
) -> Response {
    match scope
        .schema
        .create_collection(body.name, body.label, body.label_plural)
    {
        Ok(c) => (StatusCode::CREATED, ApiResponse::success(c)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_collections(scope: VersionScope) -> Response {
    ApiResponse::success(scope.schema.collections()).into_response()
}

pub async fn get_collection(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope.schema.collection(&name) {
        Some(c) => ApiResponse::success(c).into_response(),
        None => error_response(StatusCode::NOT_FOUND, &format!("collection not found: {name}")),
    }
}

pub async fn update_collection(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
    Json(patch): Json<CollectionUpdate>,
) -> Response {
    match scope.schema.update_collection(&name, patch) {
        Ok(c) => ApiResponse::success(c).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn delete_collection(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope.schema.delete_collection(&name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn create_field(
    scope: VersionScope,
    Path((_, _, _, collection)): Path<(String, String, String, String)>,
    Json(body): Json<CreateFieldRequest>,
) -> Response {
    match scope.schema.create_field(collection, body.name, body.r#type) {
        Ok(f) => (StatusCode::CREATED, ApiResponse::success(f)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_fields(
    scope: VersionScope,
    Path((_, _, _, collection)): Path<(String, String, String, String)>,
) -> Response {
    ApiResponse::success(scope.schema.fields(&collection)).into_response()
}

pub async fn update_field(
    scope: VersionScope,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<FieldUpdate>,
) -> Response {
    match scope.schema.update_field(&collection, &name, patch) {
        Ok(f) => ApiResponse::success(f).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn delete_field(
    scope: VersionScope,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
) -> Response {
    match scope.schema.delete_field(&collection, &name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => version_schema_error_to_response(e),
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

    let site_version = scope.site.version();
    if site_version.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "site has no version configured");
    }

    let namespace_str = format!("{}@{}", scope.project.project_id(), site_version);
    let deps = match crate::manifest::resolve_dependency_tree(&state.schema, &namespace_str) {
        Ok(deps) => deps,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let mut result: Vec<NamespaceCollections> = Vec::new();
    for dep in &deps {
        let collection_prefix = format!("{}/versions/{}/collections/", dep.project_id, dep.version);
        let field_prefix_base = format!("{}/versions/{}/fields/", dep.project_id, dep.version);
        let collections = state.schema.collections().list(&collection_prefix);
        let all_fields = state.schema.fields().list(&field_prefix_base);

        let mut coll_with_fields: Vec<CollectionWithFields> = Vec::new();
        for (_, col) in &collections {
            let col_name = col.name();
            let field_prefix = format!("{field_prefix_base}{col_name}/");
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
            namespace: dep.project_id.clone(),
            collections: coll_with_fields,
        });
    }

    ApiResponse::success(result).into_response()
}
