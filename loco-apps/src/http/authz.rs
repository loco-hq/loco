use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::auth_error_to_response;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::SchemaStore;

/// Developer (or org owner) on `{account}/{project}`. Used by `/schema` and
/// `/config` extractors that target a path project.
pub fn require_developer(
    state: &AppState,
    identity_handle: &str,
    project_id: &str,
) -> Result<(), Response> {
    match state
        .auth_adapter
        .project_access(identity_handle, project_id)
    {
        Ok(Some(role)) if role.can_develop() => Ok(()),
        Ok(_) => Err(error_response(
            StatusCode::FORBIDDEN,
            "you do not have access to this resource",
        )),
        Err(e) => Err(auth_error_to_response(e)),
    }
}

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
