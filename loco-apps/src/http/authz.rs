use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::auth_error_to_response;
use crate::http::response::error_response;
use crate::http::version_schema::split_qualified;
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

/// Whether a grant written in a permission set owned by `set_project` names
/// the collection `collection_name` owned by `collection_project`.
///
/// A bare grant means the set's own project's collection — the rule every
/// bare name follows ("Name resolution" in CLAUDE.md), applied from where the
/// grant is written. So a consumer's set granting `contacts` opens only the
/// consumer's `contacts`, never a dependency's of the same name; and a set a
/// dependency ships, once a consumer opts into it, opens the dependency's
/// collections and never the consumer's. A qualified grant
/// (`{account}/{project}.{name}`) names its owner exactly.
pub fn collection_grant_matches(
    grant: &str,
    set_project: &str,
    collection_name: &str,
    collection_project: &str,
) -> bool {
    let (project, name) = split_qualified(grant).unwrap_or((set_project, grant));
    project == collection_project && name == collection_name
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
            collection_grant_matches(
                g.collection(),
                s.project(),
                collection_name,
                collection_project,
            ) && grant_allows(g, verb)
        })
    })
}

/// A draft is a version whose name ends in `-dev` (`0.0.1-dev`). Only drafts
/// accept `/schema` writes, and only published versions serve immutable
/// bundle bytes. The suffix is the rule, not any hyphen: `my-app` is
/// published.
pub fn is_draft_version(version: &str) -> bool {
    version.ends_with("-dev")
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
            "0-dev".into(),
            name.into(),
            name.into(),
            String::new(),
            grants,
        )
    }

    #[test]
    fn draft_is_a_dev_suffix_not_any_hyphen() {
        assert!(is_draft_version("0.0.1-dev"));
        assert!(is_draft_version("0-boot-dev"));
        assert!(!is_draft_version("my-app"));
        assert!(!is_draft_version("1.0.0"));
        assert!(!is_draft_version("0-draft"));
        assert!(!is_draft_version("0-dev-final"));
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
    fn bare_grant_matches_only_the_sets_own_project() {
        // A set owned by alice/testapp: bare `contacts` is alice's, not a
        // dependency's collection of the same name.
        let g = set("r", vec![grant("contacts", true, false, false, false)]);
        assert!(public_may(
            [&g],
            "contacts",
            "alice/testapp",
            DataVerb::Read
        ));
        assert!(!public_may([&g], "contacts", "loco/core", DataVerb::Read));
        // A set a dependency ships: bare means the dependency's collection.
        assert!(collection_grant_matches(
            "contacts",
            "loco/core",
            "contacts",
            "loco/core"
        ));
        assert!(!collection_grant_matches(
            "contacts",
            "loco/core",
            "contacts",
            "alice/testapp"
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
        assert!(collection_grant_matches(
            "alice/testapp.guestbook",
            "loco/core",
            "guestbook",
            "alice/testapp"
        ));
        assert!(!collection_grant_matches(
            "alice/testapp.guestbook",
            "alice/testapp",
            "guestbook",
            "loco/core"
        ));
    }
}
