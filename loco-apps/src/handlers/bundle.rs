//! `/schema/{account}/{project}/{version}/bundle` — the version's static file
//! tree.
//!
//! Deploy is a draft `/schema` write, not a separate hosting API: PUT replaces
//! the whole tree from a zip of a Vite `dist/`, GET reports metadata about the
//! stored tree (never its files), DELETE drops it. Writes take the same
//! developer-plus-draft bar as any other `/schema` write, so a published
//! version's bytes can never change; the read takes the same bar as any other
//! `/schema` GET.
//!
//! Serving the files at a site URL is a different route entirely (#30).
//!
//! ## Limits
//!
//! A PUT is refused with 400 when the archive exceeds any of these (defined in
//! [`crate::bundle`]): 32 MiB of request body, 64 MiB unpacked across the whole
//! tree, 16 MiB for one file, 2000 files. It is also refused when an entry
//! escapes the tree (`..`, an absolute path, an empty segment), when an entry
//! is a symlink, or when there is no `index.html` at the zip root.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::bundle::{unpack_zip, BundleMetadata, MAX_ZIP_BYTES};
use crate::http::response::{error_response, version_schema_error_to_response, ApiResponse};
use crate::http::scope::{VersionReadScope, VersionScope};
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    use axum::routing::get;
    Router::new().route(
        "/{user}/{project}/{version}/bundle",
        get(get_bundle)
            .put(put_bundle)
            .delete(delete_bundle)
            // Axum's default body cap is 2 MB — far under a real `dist/`. The
            // handler's own check is what produces a readable 400; this layer
            // is the backstop that keeps an oversized body from being buffered
            // in the first place.
            .layer(DefaultBodyLimit::max(MAX_ZIP_BYTES)),
    )
}

/// Metadata for the stored tree: `{ hash, uploaded_at, size, files }`. 404 when
/// the version has no bundle — including every published version until
/// copy-version can carry one forward.
pub async fn get_bundle(scope: VersionReadScope) -> Response {
    let tree = match scope.schema.bundle() {
        Ok(Some(tree)) => tree,
        Ok(None) => return not_found(&scope.schema),
        Err(e) => return version_schema_error_to_response(e),
    };
    let uploaded_at = match scope.schema.bundle_uploaded_at() {
        Ok(Some(t)) => t,
        // The tree read a moment ago; a missing timestamp means it went away
        // between the two calls, which is the same answer as never having one.
        Ok(None) => return not_found(&scope.schema),
        Err(e) => return version_schema_error_to_response(e),
    };
    ApiResponse::success(BundleMetadata::new(&tree, uploaded_at)).into_response()
}

/// Replace the tree from a zip. Developer on the project, draft version only.
pub async fn put_bundle(scope: VersionScope, body: Bytes) -> Response {
    // Check the version before spending CPU on the archive: a published
    // version is not writable no matter what the body holds.
    if let Err(e) = scope.schema.require_writable() {
        return version_schema_error_to_response(e);
    }
    let tree = match unpack_zip(&body) {
        Ok(tree) => tree,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    if let Err(e) = scope.schema.put_bundle(&tree) {
        return version_schema_error_to_response(e);
    }
    // Report the timestamp the store actually recorded, so a PUT and a
    // follow-up GET agree.
    let uploaded_at = scope
        .schema
        .bundle_uploaded_at()
        .ok()
        .flatten()
        .unwrap_or_else(std::time::SystemTime::now);
    ApiResponse::success(BundleMetadata::new(&tree, uploaded_at)).into_response()
}

/// Drop the tree. Developer on the project, draft version only.
pub async fn delete_bundle(scope: VersionScope) -> Response {
    match scope.schema.delete_bundle() {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(e) => version_schema_error_to_response(e),
    }
}

fn not_found(schema: &crate::http::version_schema::VersionSchema) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        &format!(
            "no bundle for {} at version {}",
            schema.project_id(),
            schema.version()
        ),
    )
}
