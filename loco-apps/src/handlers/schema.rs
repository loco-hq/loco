use std::sync::Arc;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use crate::http::response::{error_response, version_schema_error_to_response, ApiResponse};
use crate::http::scope::{VersionReadScope, VersionScope};
use crate::server::AppState;
use crate::{
    Collection, CollectionUpdate, Field, FieldUpdate, Fieldset, FieldsetUpdate, ManifestUpdate,
    PermissionSet, PermissionSetUpdate,
};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            "/{user}/{project}/{version}/manifest",
            get(get_manifest).put(update_manifest),
        )
        .route(
            "/{user}/{project}/{version}/collection",
            post(create_collection),
        )
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
        .route("/{user}/{project}/{version}/field", post(create_field))
        .route(
            "/{user}/{project}/{version}/field/{collection}/list",
            get(list_fields),
        )
        .route(
            "/{user}/{project}/{version}/field/{collection}/{name}",
            axum::routing::put(update_field).delete(delete_field),
        )
        .route(
            "/{user}/{project}/{version}/fieldset",
            post(create_fieldset),
        )
        .route(
            "/{user}/{project}/{version}/fieldset/{collection}/list",
            get(list_fieldsets),
        )
        .route(
            "/{user}/{project}/{version}/fieldset/{collection}/{name}",
            get(get_fieldset)
                .put(update_fieldset)
                .delete(delete_fieldset),
        )
        .route(
            "/{user}/{project}/{version}/permission_set",
            post(create_permission_set),
        )
        .route(
            "/{user}/{project}/{version}/permission_set/list",
            get(list_permission_sets),
        )
        .route(
            "/{user}/{project}/{version}/permission_set/{name}",
            get(get_permission_set)
                .put(update_permission_set)
                .delete(delete_permission_set),
        )
        // The bundle is schema too: a version's file tree, written the same
        // way its YAML is. Its own module because the body is a zip, not JSON.
        .merge(crate::handlers::bundle::routes())
}

pub async fn get_manifest(scope: VersionReadScope) -> Response {
    match scope.schema.manifest() {
        Some(m) => ApiResponse::success(m).into_response(),
        None => error_response(StatusCode::NOT_FOUND, "manifest not found for this version"),
    }
}

pub async fn update_manifest(scope: VersionScope, Json(patch): Json<ManifestUpdate>) -> Response {
    match scope.schema.update_manifest(patch) {
        Ok(m) => ApiResponse::success(m).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn create_collection(scope: VersionScope, Json(input): Json<Collection>) -> Response {
    match scope.schema.create_collection(input) {
        Ok(c) => (StatusCode::CREATED, ApiResponse::success(c)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_collections(scope: VersionReadScope) -> Response {
    ApiResponse::success(scope.schema.collections()).into_response()
}

pub async fn get_collection(
    scope: VersionReadScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope.schema.collection(&name) {
        Some(c) => ApiResponse::success(c).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("collection not found: {name}"),
        ),
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

pub async fn create_field(scope: VersionScope, Json(input): Json<Field>) -> Response {
    match scope.schema.create_field(input) {
        Ok(f) => (StatusCode::CREATED, ApiResponse::success(f)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_fields(
    scope: VersionReadScope,
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

pub async fn create_fieldset(scope: VersionScope, Json(input): Json<Fieldset>) -> Response {
    match scope.schema.create_fieldset(input) {
        Ok(fs) => (StatusCode::CREATED, ApiResponse::success(fs)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_fieldsets(
    scope: VersionReadScope,
    Path((_, _, _, collection)): Path<(String, String, String, String)>,
) -> Response {
    ApiResponse::success(scope.schema.fieldsets(&collection)).into_response()
}

pub async fn get_fieldset(
    scope: VersionReadScope,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
) -> Response {
    match scope.schema.fieldset(&collection, &name) {
        Some(fs) => ApiResponse::success(fs).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("fieldset not found: {collection}/{name}"),
        ),
    }
}

pub async fn update_fieldset(
    scope: VersionScope,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
    Json(patch): Json<FieldsetUpdate>,
) -> Response {
    match scope.schema.update_fieldset(&collection, &name, patch) {
        Ok(fs) => ApiResponse::success(fs).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn delete_fieldset(
    scope: VersionScope,
    Path((_, _, _, collection, name)): Path<(String, String, String, String, String)>,
) -> Response {
    match scope.schema.delete_fieldset(&collection, &name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn create_permission_set(
    scope: VersionScope,
    Json(input): Json<PermissionSet>,
) -> Response {
    match scope.schema.create_permission_set(input) {
        Ok(ps) => (StatusCode::CREATED, ApiResponse::success(ps)).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn list_permission_sets(scope: VersionReadScope) -> Response {
    ApiResponse::success(scope.schema.permission_sets()).into_response()
}

pub async fn get_permission_set(
    scope: VersionReadScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope.schema.permission_set(&name) {
        Some(ps) => ApiResponse::success(ps).into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            &format!("permission set not found: {name}"),
        ),
    }
}

pub async fn update_permission_set(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
    Json(patch): Json<PermissionSetUpdate>,
) -> Response {
    match scope.schema.update_permission_set(&name, patch) {
        Ok(ps) => ApiResponse::success(ps).into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

pub async fn delete_permission_set(
    scope: VersionScope,
    Path((_, _, _, name)): Path<(String, String, String, String)>,
) -> Response {
    match scope.schema.delete_permission_set(&name) {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}
