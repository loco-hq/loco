use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::auth_error_to_response;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{PermissionSet, SchemaStore};

pub fn forbidden() -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        "you do not have access to this resource",
    )
}

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
        Ok(_) => Err(forbidden()),
        Err(e) => Err(auth_error_to_response(e)),
    }
}

/// Union of `read` grants across stacked permission sets.
pub fn public_may_read<'a, I>(sets: I, collection: &str) -> bool
where
    I: IntoIterator<Item = &'a PermissionSet>,
{
    sets.into_iter()
        .any(|s| s.read().iter().any(|c| c == collection))
}

/// Union of `create` grants across stacked permission sets.
/// Update/delete are never public (until record-level security).
pub fn public_may_create<'a, I>(sets: I, collection: &str) -> bool
where
    I: IntoIterator<Item = &'a PermissionSet>,
{
    sets.into_iter()
        .any(|s| s.create().iter().any(|c| c == collection))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, read: &[&str], create: &[&str]) -> PermissionSet {
        PermissionSet::new(
            "alice/testapp".into(),
            "0-draft".into(),
            name.into(),
            name.into(),
            String::new(),
            read.iter().map(|s| (*s).to_string()).collect(),
            create.iter().map(|s| (*s).to_string()).collect(),
        )
    }

    #[test]
    fn no_sets_means_no_public_access() {
        let none: [&PermissionSet; 0] = [];
        assert!(!public_may_read(none, "guestbook"));
        assert!(!public_may_create(none, "guestbook"));
    }

    #[test]
    fn read_and_create_are_independent() {
        let readable = set("r", &["guestbook"], &[]);
        assert!(public_may_read([&readable], "guestbook"));
        assert!(!public_may_create([&readable], "guestbook"));
        assert!(!public_may_read([&readable], "secrets"));
    }

    #[test]
    fn stacking_is_union() {
        let read = set("guestbook_read", &["guestbook"], &[]);
        let create = set("guestbook_create", &[], &["guestbook"]);
        let stacked = [&read, &create];
        assert!(public_may_read(stacked, "guestbook"));
        assert!(public_may_create(stacked, "guestbook"));
        assert!(!public_may_read(stacked, "secrets"));
        assert!(!public_may_create(stacked, "secrets"));
    }
}
