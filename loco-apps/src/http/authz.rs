use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::AuthUser;
use crate::http::response::error_response;
use crate::SchemaStore;

/// Sites that are allowed to edit config/schema (like admin tools).
const CONFIG_SITES: &[&str] = &["studio", "cards"];

pub fn require_config_site(auth_user: &AuthUser) -> Result<(), Response> {
    // site_id is fully qualified (e.g. "alice/testapp/cards"); check only the site name
    let site_name = auth_user
        .site_id
        .rsplit('/')
        .next()
        .unwrap_or(&auth_user.site_id);
    if CONFIG_SITES.contains(&site_name) {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::FORBIDDEN,
            "this site does not have config editing permissions",
        ))
    }
}

pub fn authorize_user(auth_user: &AuthUser, path_user: &str) -> Result<(), Response> {
    if auth_user.username == path_user {
        Ok(())
    } else {
        Err(error_response(
            StatusCode::FORBIDDEN,
            "you do not have access to this resource",
        ))
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
