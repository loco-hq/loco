//! Charsets for the names `/config` creates: projects, datasets, sites, and
//! versions. Each name becomes a path segment under `schemas/instances/`, and a
//! site's also becomes a host label, so an odd one does not fail loudly — a
//! `.loco-` name is skipped by `load_all` as a temp artefact and vanishes on the
//! next boot, and a version with an `@` can never be named as a dependency.
//!
//! Checked on write only. A name already on disk loads whatever it is.

/// Longest accepted name. A site name is a DNS label, and 63 is that limit;
/// the other kinds share it so there is one number to remember.
const MAX_LEN: usize = 63;

/// Project, dataset, site, and account-handle names: `[a-z0-9_]`, starting
/// with a letter or `_`. A new account handle uses this too (signup, a new
/// org handle, login auto-create). Inviting a member does not: a handle
/// already on disk is not passed through it.
pub fn check_slug(kind: &str, name: &str) -> Result<(), String> {
    let valid = (1..=MAX_LEN).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{kind} name {name:?} must be 1-{MAX_LEN} characters of a-z, 0-9, and _, \
             starting with a letter or _"
        ))
    }
}

/// Version names: `[a-z0-9._-]`, not starting with `.` (which also rules out
/// `.` and `..`). No `@`, which separates project from version in a manifest
/// dependency.
pub fn check_version(version: &str) -> Result<(), String> {
    let valid = (1..=MAX_LEN).contains(&version.len())
        && version
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
        && !version.starts_with('.');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "version {version:?} must be 1-{MAX_LEN} characters of a-z, 0-9, '.', '_', and '-', \
             not starting with '.'"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_names() {
        for ok in ["crm", "inventory", "my_app", "_x", "app2", &"a".repeat(63)] {
            assert!(check_slug("project", ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".loco-x",
            "My",
            "my-app",
            "2app",
            "a b",
            "a/b",
            "a.b",
            &"a".repeat(64),
        ] {
            assert!(check_slug("project", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn version_names() {
        for ok in [
            "0.0.1",
            "0.0.1-dev",
            "my-app",
            "0-draft",
            "v1",
            "1.0_rc",
            &"1".repeat(63),
        ] {
            assert!(check_version(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".hidden",
            ".loco-1",
            ".",
            "..",
            "1.0@2",
            "1.0/x",
            "1.0 x",
            "V1",
            &"1".repeat(64),
        ] {
            assert!(check_version(bad).is_err(), "{bad}");
        }
    }
}
