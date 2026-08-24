use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::auth_error_to_response;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{CollectionGrant, PermissionSet};

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

/// The caller's identity id must match the path id. Person accounts are
/// personal — org owners manage membership, not the identity record.
pub fn require_self(caller_id: &str, target_id: &str) -> Result<(), Response> {
    if caller_id == target_id {
        Ok(())
    } else {
        Err(forbidden())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataVerb {
    Read,
    Create,
    Update,
    Delete,
}

/// Bare names match any visible collection with that name (`VersionSchema`
/// already prefers self on a lookup). Qualified `{project}.{name}` (e.g.
/// `ben/crm.contacts`) pins the owning project so two deps that share a
/// name can be distinguished later.
pub fn collection_grant_matches(
    grant: &str,
    collection_name: &str,
    collection_project: &str,
) -> bool {
    if grant == collection_name {
        return true;
    }
    grant == format!("{collection_project}.{collection_name}")
}

fn grant_allows(g: &CollectionGrant, verb: DataVerb) -> bool {
    match verb {
        DataVerb::Read => g.read(),
        DataVerb::Create => g.create(),
        DataVerb::Update => g.update(),
        DataVerb::Delete => g.delete(),
    }
}

/// Union of grants across stacked permission sets. Duplicate rows OR.
/// Unspecified flags are false — a collection listed with no verbs is inert.
pub fn public_may<'a, I>(
    sets: I,
    collection_name: &str,
    collection_project: &str,
    verb: DataVerb,
) -> bool
where
    I: IntoIterator<Item = &'a PermissionSet>,
{
    sets.into_iter().any(|s| {
        s.collections().iter().any(|g| {
            collection_grant_matches(g.collection(), collection_name, collection_project)
                && grant_allows(g, verb)
        })
    })
}

pub fn is_draft_version(version: &str) -> bool {
    version.contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(
        collection: &str,
        read: bool,
        create: bool,
        update: bool,
        delete: bool,
    ) -> CollectionGrant {
        CollectionGrant::new(collection.into(), read, create, update, delete)
    }

    fn set(name: &str, grants: Vec<CollectionGrant>) -> PermissionSet {
        PermissionSet::new(
            "alice/testapp".into(),
            "0-draft".into(),
            name.into(),
            name.into(),
            String::new(),
            grants,
        )
    }

    #[test]
    fn no_sets_means_no_public_access() {
        let none: [&PermissionSet; 0] = [];
        assert!(!public_may(
            none,
            "guestbook",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(!public_may(
            none,
            "guestbook",
            "alice/testapp",
            DataVerb::Create
        ));
    }

    #[test]
    fn verbs_are_independent_and_default_false() {
        let readable = set("r", vec![grant("guestbook", true, false, false, false)]);
        assert!(public_may(
            [&readable],
            "guestbook",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(!public_may(
            [&readable],
            "guestbook",
            "alice/testapp",
            DataVerb::Create
        ));
        assert!(!public_may(
            [&readable],
            "guestbook",
            "alice/testapp",
            DataVerb::Update
        ));
        assert!(!public_may(
            [&readable],
            "guestbook",
            "alice/testapp",
            DataVerb::Delete
        ));
        assert!(!public_may(
            [&readable],
            "secrets",
            "alice/testapp",
            DataVerb::Read
        ));
    }

    #[test]
    fn stacking_is_union() {
        let read = set(
            "guestbook_read",
            vec![grant("guestbook", true, false, false, false)],
        );
        let create = set(
            "guestbook_create",
            vec![grant("guestbook", false, true, false, false)],
        );
        let stacked = [&read, &create];
        assert!(public_may(
            stacked,
            "guestbook",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(public_may(
            stacked,
            "guestbook",
            "alice/testapp",
            DataVerb::Create
        ));
        assert!(!public_may(
            stacked,
            "guestbook",
            "alice/testapp",
            DataVerb::Update
        ));
        assert!(!public_may(
            stacked,
            "secrets",
            "alice/testapp",
            DataVerb::Read
        ));
    }

    #[test]
    fn duplicate_rows_in_one_set_or() {
        let both = set(
            "split",
            vec![
                grant("guestbook", true, false, false, false),
                grant("guestbook", false, true, false, false),
            ],
        );
        assert!(public_may(
            [&both],
            "guestbook",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(public_may(
            [&both],
            "guestbook",
            "alice/testapp",
            DataVerb::Create
        ));
    }

    #[test]
    fn update_and_delete_are_honored() {
        let full = set("wiki", vec![grant("wiki", true, true, true, true)]);
        assert!(public_may(
            [&full],
            "wiki",
            "alice/testapp",
            DataVerb::Update
        ));
        assert!(public_may(
            [&full],
            "wiki",
            "alice/testapp",
            DataVerb::Delete
        ));
    }

    #[test]
    fn bare_name_matches_regardless_of_owning_project() {
        // Package collection resolved as loco/core.contacts — bare grant still hits.
        let g = set("r", vec![grant("contacts", true, false, false, false)]);
        assert!(public_may([&g], "contacts", "loco/core", DataVerb::Read));
        assert!(public_may(
            [&g],
            "contacts",
            "alice/testapp",
            DataVerb::Read
        ));
    }

    #[test]
    fn qualified_grant_pins_owning_project() {
        let g = set(
            "r",
            vec![grant("loco/core.contacts", true, false, false, false)],
        );
        assert!(public_may([&g], "contacts", "loco/core", DataVerb::Read));
        assert!(!public_may(
            [&g],
            "contacts",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(!collection_grant_matches(
            "loco/core.contacts",
            "contacts",
            "alice/testapp"
        ));
        assert!(collection_grant_matches(
            "alice/testapp.guestbook",
            "guestbook",
            "alice/testapp"
        ));
        assert!(collection_grant_matches(
            "guestbook",
            "guestbook",
            "alice/testapp"
        ));
    }
}
