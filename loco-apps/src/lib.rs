// Handlers use `Result<_, axum::response::Response>` pervasively; boxing every Response is noise.
#![allow(clippy::result_large_err)]

include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));

pub mod auth;
pub mod handlers;
pub mod http;
pub mod server;
pub mod validation;

#[cfg(test)]
mod generated_tests {
    use super::*;

    #[test]
    fn collection_path_roundtrip() {
        let path = Collection::to_path("ben/crm", "0.0.1-dev", "account");
        assert_eq!(path, "ben/crm/versions/0.0.1-dev/collections/account");

        let vars = Collection::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");
        assert_eq!(vars.get("name").unwrap(), "account");
    }

    #[test]
    fn dataset_path_roundtrip() {
        let path = Dataset::to_path("ben/crm", "acme");
        assert_eq!(path, "ben/crm/datasets/acme");
        let vars = Dataset::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("name").unwrap(), "acme");
    }

    #[test]
    fn field_path_roundtrip() {
        let path = Field::to_path("ben/crm", "0.0.1-dev", "account", "company");
        assert_eq!(path, "ben/crm/versions/0.0.1-dev/fields/account/company");
        let vars = Field::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");
        assert_eq!(vars.get("collection").unwrap(), "account");
        assert_eq!(vars.get("name").unwrap(), "company");
    }

    #[test]
    fn project_path_roundtrip() {
        // ${project} has 2 segments, trailing literal "project".
        let path = Project::to_path("ben/crm");
        assert_eq!(path, "ben/crm/project");
        let vars = Project::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
    }

    #[test]
    fn from_path_rejects_wrong_shape() {
        // Literal mismatch
        assert!(Dataset::from_path("ben/crm/sites/acme").is_none());
        // Too short
        assert!(Dataset::from_path("ben").is_none());
        // Trailing junk
        assert!(Dataset::from_path("ben/crm/datasets/acme/extra").is_none());
    }

    #[test]
    fn permission_set_path_roundtrip() {
        let path = PermissionSet::to_path("ben/crm", "0.0.1-dev", "public_contacts");
        assert_eq!(
            path,
            "ben/crm/versions/0.0.1-dev/permission_sets/public_contacts"
        );
        let vars = PermissionSet::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");
        assert_eq!(vars.get("name").unwrap(), "public_contacts");
    }

    #[test]
    fn permission_set_yaml_grants() {
        let vars = std::collections::HashMap::from([
            ("project".into(), "alice/testapp".into()),
            ("version".into(), "0-draft".into()),
            ("name".into(), "guestbook_read".into()),
        ]);
        let ps = PermissionSet::from_yaml(
            "label: Guestbook read\ncollections:\n  - collection: guestbook\n    read: true\n",
            &vars,
        )
        .unwrap();
        assert_eq!(ps.collections().len(), 1);
        assert_eq!(ps.collections()[0].collection(), "guestbook");
        assert!(ps.collections()[0].read());
        assert!(!ps.collections()[0].create());
        assert!(!ps.collections()[0].update());
        assert!(!ps.collections()[0].delete());
    }

    /// Public policy is a property of the version, not of the URL that pins
    /// it — so the assignment parses off the manifest.
    #[test]
    fn manifest_yaml_public_permission_sets() {
        let vars = std::collections::HashMap::from([
            ("project".into(), "alice/testapp".into()),
            ("version".into(), "0-draft".into()),
        ]);
        let m = Manifest::from_yaml(
            "dependencies:\n  - acme/crm@1.0\npublic_permission_sets:\n  - guestbook_read\n  - guestbook_create\n",
            &vars,
        )
        .unwrap();
        assert_eq!(m.dependencies(), &["acme/crm@1.0".to_string()]);
        assert_eq!(
            m.public_permission_sets(),
            &["guestbook_read".to_string(), "guestbook_create".to_string()]
        );
    }

    /// A version that assigns nothing gives `public` nothing.
    #[test]
    fn manifest_yaml_public_permission_sets_default_empty() {
        let vars = std::collections::HashMap::from([
            ("project".into(), "alice/testapp".into()),
            ("version".into(), "0-draft".into()),
        ]);
        let m = Manifest::from_yaml("dependencies: []\n", &vars).unwrap();
        assert!(m.public_permission_sets().is_empty());
    }
}
