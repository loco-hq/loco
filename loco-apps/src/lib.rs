// Handlers use `Result<_, axum::response::Response>` pervasively; boxing every Response is noise.
#![allow(clippy::result_large_err)]

include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));

pub mod actions;
pub mod auth;
pub mod bundle;
pub mod handlers;
pub mod http;
pub mod integrations;
pub mod query;
pub mod seed;
pub mod server;
pub mod validation;
pub mod values;

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
    fn secret_and_variable_path_roundtrip() {
        let secret = Secret::to_path("ben/crm", "0.0.1-dev", "consumer_key");
        assert_eq!(secret, "ben/crm/versions/0.0.1-dev/secrets/consumer_key");
        let vars = Secret::from_path(&secret).unwrap();
        assert_eq!(vars.get("name").unwrap(), "consumer_key");

        let variable = Variable::to_path("ben/crm", "0.0.1-dev", "api_base");
        assert_eq!(variable, "ben/crm/versions/0.0.1-dev/variables/api_base");
        let vars = Variable::from_path(&variable).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("name").unwrap(), "api_base");
    }

    #[test]
    fn variable_yaml_default() {
        let vars = std::collections::HashMap::from([
            ("project".into(), "alice/bricklink".into()),
            ("version".into(), "0.0.1-dev".into()),
            ("name".into(), "api_base".into()),
        ]);
        let variable = Variable::from_yaml(
            "label: API base\ndescription: BrickLink endpoint\nrequired: true\ndefault: https://example.test\n",
            &vars,
        )
        .unwrap();
        assert_eq!(variable.label(), "API base");
        assert!(variable.required());
        assert_eq!(variable.default(), "https://example.test");

        let omitted = Variable::from_yaml("label: API base\n", &vars).unwrap();
        assert_eq!(omitted.default(), "");
        assert!(!omitted.required());
    }

    #[test]
    fn action_yaml_params_roundtrip() {
        let path = Action::to_path("ben/crm", "0.0.1-dev", "sync_orders");
        assert_eq!(path, "ben/crm/versions/0.0.1-dev/actions/sync_orders");
        let vars = Action::from_path(&path).unwrap();
        assert_eq!(vars.get("project").unwrap(), "ben/crm");
        assert_eq!(vars.get("name").unwrap(), "sync_orders");

        let action = Action::from_yaml(
            "label: Echo\ndescription: Returns JSON\nparams:\n  - name: size\n    type: string\n    label: Size\n    required: true\n    options:\n      - value: s\n        label: Small\n  - name: qty\n    type: integer\n    label: Qty\n    required: true\n",
            &vars,
        )
        .unwrap();
        assert_eq!(action.label(), "Echo");
        assert_eq!(action.params().len(), 2);
        assert_eq!(action.params()[0].name(), "size");
        assert_eq!(action.params()[0].r#type(), "string");
        assert!(action.params()[0].required());
        assert_eq!(action.params()[0].options().len(), 1);
        assert_eq!(action.params()[0].options()[0].value(), "s");
        assert_eq!(action.params()[0].options()[0].label(), "Small");
        assert_eq!(action.params()[1].name(), "qty");
        assert_eq!(action.params()[1].r#type(), "integer");
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
            ("version".into(), "0-dev".into()),
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
            ("version".into(), "0-dev".into()),
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
            ("version".into(), "0-dev".into()),
        ]);
        let m = Manifest::from_yaml("dependencies: []\n", &vars).unwrap();
        assert!(m.public_permission_sets().is_empty());
    }

    /// Nested integration paths are their own templates. An ordinary
    /// collection, field, or action does not match them, and they do not
    /// match an ordinary path of the same name.
    #[test]
    fn integration_paths_do_not_match_ordinary_types() {
        let project = "ben/crm";
        let version = "0.0.1-dev";

        let type_collection = TypeCollection::to_path(project, version, "bricklink", "orders");
        assert_eq!(
            type_collection,
            "ben/crm/versions/0.0.1-dev/integration_types/bricklink/collections/orders"
        );
        assert!(Collection::from_path(&type_collection).is_none());
        assert!(TypeCollection::from_path(&type_collection).is_some());
        let ordinary = Collection::to_path(project, version, "orders");
        assert!(TypeCollection::from_path(&ordinary).is_none());
        assert!(Collection::from_path(&ordinary).is_some());

        let type_field = TypeField::to_path(project, version, "bricklink", "orders", "status");
        assert!(Field::from_path(&type_field).is_none());
        assert_eq!(
            TypeField::from_path(&type_field)
                .unwrap()
                .get("integration_type")
                .unwrap(),
            "bricklink"
        );
        let ordinary_field = Field::to_path(project, version, "orders", "status");
        assert!(TypeField::from_path(&ordinary_field).is_none());

        let type_action = TypeAction::to_path(project, version, "bricklink", "set_status");
        assert!(Action::from_path(&type_action).is_none());
        assert!(TypeAction::from_path(&type_action).is_some());
        let other_orders = TypeCollection::to_path(project, version, "warehouse", "orders");
        assert_ne!(type_collection, other_orders);

        let custom = IntegrationCollection::to_path(project, version, "store", "invoice");
        assert_eq!(
            custom,
            "ben/crm/versions/0.0.1-dev/integrations/store/collections/invoice"
        );
        assert!(Collection::from_path(&custom).is_none());
        assert!(TypeCollection::from_path(&custom).is_none());
        assert!(IntegrationCollection::from_path(&custom).is_some());

        let custom_field =
            IntegrationField::to_path(project, version, "store", "invoice", "amount");
        assert!(Field::from_path(&custom_field).is_none());
        assert!(TypeField::from_path(&custom_field).is_none());
        assert!(IntegrationField::from_path(&custom_field).is_some());

        let integration = Integration::to_path(project, version, "store");
        let integration_type = IntegrationType::to_path(project, version, "bricklink");
        assert!(Integration::from_path(&integration_type).is_none());
        assert!(IntegrationType::from_path(&integration).is_none());
        assert!(Integration::from_path(&integration).is_some());
        assert!(IntegrationType::from_path(&integration_type).is_some());

        let vars = std::collections::HashMap::from([
            ("project".into(), "alice/pkg".into()),
            ("version".into(), "0.0.1-dev".into()),
            ("name".into(), "bricklink".into()),
        ]);
        let parsed = IntegrationType::from_yaml(
            "label: BrickLink\nsecrets:\n  - name: consumer_key\n    label: Consumer key\n    required: true\nvariables:\n  - name: base_url\n    default: https://api.bricklink.com/api/store/v1\n",
            &vars,
        )
        .unwrap();
        assert_eq!(parsed.name(), "bricklink");
        assert_eq!(parsed.secrets()[0].name(), "consumer_key");
        assert!(parsed.secrets()[0].required());
        assert_eq!(
            parsed.variables()[0].default(),
            "https://api.bricklink.com/api/store/v1"
        );
    }
}
