//! Committed projects live in `schemas/seed/`; the live store is
//! `schemas/instances/`, which is wholly gitignored. At boot a seed project is
//! copied into the store only when the store does not have it, so every write
//! — Studio edits, bundle deploys — lands outside version control.
//!
//! "Has it" means the project's `project.yaml` exists, not its directory. An
//! older checkout can hold leftovers (a deployed `bundle/`) under a project
//! whose tracked YAML git has since removed; those must not block the seed.
//!
//! A seed is never re-synced. Once a project is in the store, the seed is not
//! read for it again; deleting the project from the store restores it on the
//! next boot.

use std::io;
use std::path::Path;

use crate::Project;

/// Copy each seed project absent from `instances_dir` into it, and return the
/// ids of the projects copied. A missing `seed_dir` seeds nothing.
///
/// Files already in the store are never overwritten, even inside a project
/// being seeded. Symlinks in the seed are skipped, not followed.
pub fn seed_instances(seed_dir: &Path, instances_dir: &Path) -> io::Result<Vec<String>> {
    std::fs::create_dir_all(instances_dir)?;
    let mut seeded = Vec::new();
    for account in sorted_dirs(seed_dir)? {
        for project in sorted_dirs(&seed_dir.join(&account))? {
            let id = format!("{account}/{project}");
            let project_file = format!("{}.yaml", Project::to_path(&id));
            if !seed_dir.join(&project_file).is_file() {
                continue;
            }
            if instances_dir.join(&project_file).exists() {
                continue;
            }
            copy_missing(&seed_dir.join(&id), &instances_dir.join(&id))?;
            seeded.push(id);
        }
    }
    Ok(seeded)
}

/// Names of the real (non-symlink) directories directly under `dir`, sorted.
/// A missing `dir` has none.
fn sorted_dirs(dir: &Path) -> io::Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

fn copy_missing(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_missing(&entry.path(), &to)?;
        } else if ty.is_file() && !to.exists() {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn read(root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    fn seed_tree() -> tempfile::TempDir {
        let seed = tempfile::TempDir::new().unwrap();
        write(seed.path(), "acme/crm/project.yaml", "label: seed\n");
        write(
            seed.path(),
            "acme/crm/versions/0.0.1-dev/collections/account.yaml",
            "label: Account\n",
        );
        write(
            seed.path(),
            "acme/crm/versions/0.0.1-dev/bundle/index.html",
            "<p>",
        );
        seed
    }

    #[test]
    fn copies_an_absent_project_whole() {
        let seed = seed_tree();
        let store = tempfile::TempDir::new().unwrap();
        let instances = store.path().join("instances");

        let seeded = seed_instances(seed.path(), &instances).unwrap();

        assert_eq!(seeded, vec!["acme/crm".to_string()]);
        assert_eq!(read(&instances, "acme/crm/project.yaml"), "label: seed\n");
        assert_eq!(
            read(
                &instances,
                "acme/crm/versions/0.0.1-dev/collections/account.yaml"
            ),
            "label: Account\n"
        );
        assert_eq!(
            read(&instances, "acme/crm/versions/0.0.1-dev/bundle/index.html"),
            "<p>"
        );
    }

    #[test]
    fn leaves_a_present_project_alone() {
        let seed = seed_tree();
        let instances = tempfile::TempDir::new().unwrap();
        write(instances.path(), "acme/crm/project.yaml", "label: edited\n");

        let seeded = seed_instances(seed.path(), instances.path()).unwrap();

        assert!(seeded.is_empty());
        assert_eq!(
            read(instances.path(), "acme/crm/project.yaml"),
            "label: edited\n"
        );
        // Deleted from the store on purpose: the seed must not bring it back
        // while the project is present.
        assert!(!instances
            .path()
            .join("acme/crm/versions/0.0.1-dev/collections/account.yaml")
            .exists());
    }

    /// An old checkout: git removed the tracked YAML, an untracked bundle
    /// stayed. The directory exists, `project.yaml` does not — so seed, and
    /// keep the leftover file rather than overwrite it.
    #[test]
    fn leftovers_without_project_yaml_do_not_block_the_seed() {
        let seed = seed_tree();
        let instances = tempfile::TempDir::new().unwrap();
        write(
            instances.path(),
            "acme/crm/versions/0.0.1-dev/bundle/index.html",
            "<p>deployed",
        );

        let seeded = seed_instances(seed.path(), instances.path()).unwrap();

        assert_eq!(seeded, vec!["acme/crm".to_string()]);
        assert_eq!(
            read(instances.path(), "acme/crm/project.yaml"),
            "label: seed\n"
        );
        assert_eq!(
            read(
                instances.path(),
                "acme/crm/versions/0.0.1-dev/bundle/index.html"
            ),
            "<p>deployed"
        );
    }

    #[test]
    fn a_deleted_project_is_restored() {
        let seed = seed_tree();
        let instances = tempfile::TempDir::new().unwrap();
        seed_instances(seed.path(), instances.path()).unwrap();
        std::fs::remove_dir_all(instances.path().join("acme/crm")).unwrap();

        let seeded = seed_instances(seed.path(), instances.path()).unwrap();

        assert_eq!(seeded, vec!["acme/crm".to_string()]);
        assert!(instances.path().join("acme/crm/project.yaml").is_file());
    }

    #[test]
    fn a_seed_dir_without_project_yaml_is_not_a_project() {
        let seed = tempfile::TempDir::new().unwrap();
        write(seed.path(), "acme/crm/datasets/dev.yaml", "");
        let instances = tempfile::TempDir::new().unwrap();

        assert!(seed_instances(seed.path(), instances.path())
            .unwrap()
            .is_empty());
        assert!(!instances.path().join("acme").exists());
    }

    #[test]
    fn a_missing_seed_dir_seeds_nothing() {
        let root = tempfile::TempDir::new().unwrap();
        let instances = root.path().join("instances");

        let seeded = seed_instances(&root.path().join("seed"), &instances).unwrap();

        assert!(seeded.is_empty());
        assert!(instances.is_dir());
    }
}
