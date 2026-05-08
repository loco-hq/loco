use axum::http::StatusCode;
use axum::response::Response;

use crate::http::response::error_response;
use crate::SchemaStore;

pub fn validate_collection(
    schema: &SchemaStore,
    user: &str,
    project: &str,
    name: &str,
) -> Result<(), Response> {
    let found = schema
        .collections()
        .list(&format!("{user}/{project}/"))
        .into_iter()
        .any(|(_, c)| c.name() == name);
    if found {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::NOT_FOUND,
            &format!("unknown collection: {user}/{project}.{name}"),
        ))
    }
}

pub fn is_draft_version(version: &str) -> bool {
    version.contains('-')
}
