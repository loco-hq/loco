// Handlers use `Result<_, axum::response::Response>` pervasively; boxing every Response is noise.
#![allow(clippy::result_large_err)]

include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));

pub mod auth;
pub mod handlers;
pub mod http;
pub mod server;
pub mod shortid;
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
}
