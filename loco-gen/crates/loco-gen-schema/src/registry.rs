use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::Error;
use crate::instance;
use crate::types::TypeDef;

/// Thread-safe in-memory registry for instances of any TypeDef.
/// Works with generic type names — knows nothing about domain concepts
/// like "collection" or "field".
/// Outer key: type name. Inner key: instance namespace (BTreeMap for efficient prefix queries).
type InstanceMap = HashMap<String, BTreeMap<String, HashMap<String, String>>>;

pub struct SchemaRegistry {
    /// All instances: type_name → { namespace → field_values }
    instances: RwLock<InstanceMap>,
    instances_dir: PathBuf,
}

fn instance_values_to_strings(inst: &crate::types::Instance) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for (key, val) in &inst.values {
        fields.insert(key.clone(), field_value_to_string(val));
    }
    fields
}

fn field_value_to_string(val: &crate::types::FieldValue) -> String {
    use crate::types::FieldValue;
    match val {
        FieldValue::String(s) => s.clone(),
        FieldValue::Integer(i) => i.to_string(),
        FieldValue::Float(f) => f.to_string(),
        FieldValue::Boolean(b) => b.to_string(),
        FieldValue::List(items) => {
            let json_items: Vec<serde_json::Value> =
                items.iter().map(field_value_to_json).collect();
            serde_json::to_string(&json_items).unwrap_or_else(|_| "[]".to_string())
        }
    }
}

fn field_value_to_json(val: &crate::types::FieldValue) -> serde_json::Value {
    use crate::types::FieldValue;
    match val {
        FieldValue::String(s) => serde_json::Value::String(s.clone()),
        FieldValue::Integer(i) => serde_json::Value::Number((*i).into()),
        FieldValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        FieldValue::Boolean(b) => serde_json::Value::Bool(*b),
        FieldValue::List(items) => {
            serde_json::Value::Array(items.iter().map(field_value_to_json).collect())
        }
    }
}

impl SchemaRegistry {
    /// Load all instances from disk into the registry.
    pub fn load(
        instances_dir: &Path,
        type_defs: &[TypeDef],
    ) -> Result<Self, Error> {
        let scanned = instance::scan_all(instances_dir, type_defs)?;

        let mut instances: InstanceMap = HashMap::new();
        for inst in &scanned {
            let type_map = instances.entry(inst.type_name.to_lowercase()).or_default();
            type_map.insert(inst.namespace.clone(), instance_values_to_strings(inst));
        }

        Ok(SchemaRegistry {
            instances: RwLock::new(instances),
            instances_dir: instances_dir.to_path_buf(),
        })
    }

    /// List all instances of a type whose namespace starts with `prefix`.
    /// Uses a BTreeMap range scan — O(log n + results) rather than a full scan.
    pub fn list_instances(
        &self,
        type_name: &str,
        prefix: &str,
    ) -> Vec<(String, HashMap<String, String>)> {
        let instances = self.instances.read().unwrap();
        instances
            .get(type_name)
            .map(|type_map| {
                type_map
                    .range(prefix.to_string()..)
                    .take_while(|(ns, _)| ns.starts_with(prefix))
                    .map(|(ns, fields)| (ns.clone(), fields.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all instances of a type (no filtering).
    pub fn list_all_instances(
        &self,
        type_name: &str,
    ) -> Vec<(String, HashMap<String, String>)> {
        let instances = self.instances.read().unwrap();
        instances
            .get(type_name)
            .map(|type_map| {
                type_map
                    .iter()
                    .map(|(ns, fields)| (ns.clone(), fields.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get a single instance by type name and namespace.
    pub fn get_instance(
        &self,
        type_name: &str,
        namespace: &str,
    ) -> Option<HashMap<String, String>> {
        let instances = self.instances.read().unwrap();
        instances
            .get(type_name)
            .and_then(|type_map| type_map.get(namespace).cloned())
    }

    /// Check if an instance exists.
    pub fn has_instance(&self, type_name: &str, namespace: &str) -> bool {
        let instances = self.instances.read().unwrap();
        instances
            .get(type_name)
            .map(|type_map| type_map.contains_key(namespace))
            .unwrap_or(false)
    }

    /// Find an instance by matching a field value.
    /// Returns the first match (namespace, fields).
    pub fn find_instance(
        &self,
        type_name: &str,
        field_name: &str,
        field_value: &str,
    ) -> Option<(String, HashMap<String, String>)> {
        let instances = self.instances.read().unwrap();
        instances.get(type_name).and_then(|type_map| {
            type_map.iter().find(|(_, fields)| {
                fields.get(field_name).map(|v| v == field_value).unwrap_or(false)
            }).map(|(ns, fields)| (ns.clone(), fields.clone()))
        })
    }

    /// Create a new instance. Writes YAML to disk and updates in-memory state.
    pub fn create_instance(
        &self,
        type_name: &str,
        namespace: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        // Check for duplicates
        {
            let instances = self.instances.read().unwrap();
            if let Some(type_map) = instances.get(type_name) {
                if type_map.contains_key(namespace) {
                    return Err(Error::AlreadyExists(namespace.to_string()));
                }
            }
        }

        // Write to disk
        let file_path = self.resolve_file_path(namespace);
        write_instance_yaml(&file_path, &fields)?;

        // Update in-memory state
        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace.to_string(), fields.clone());

        Ok(fields)
    }

    /// Update an existing instance. Writes YAML to disk and updates in-memory state.
    pub fn update_instance(
        &self,
        type_name: &str,
        namespace: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        // Verify it exists and get current fields
        let current = {
            let instances = self.instances.read().unwrap();
            instances
                .get(type_name)
                .and_then(|type_map| type_map.get(namespace).cloned())
                .ok_or_else(|| Error::NotFound(namespace.to_string()))?
        };

        // Merge: new fields override, keep existing for unset fields
        let mut merged = current;
        for (k, v) in &fields {
            merged.insert(k.clone(), v.clone());
        }

        // Write to disk
        let file_path = self.resolve_file_path(namespace);
        write_instance_yaml(&file_path, &merged)?;

        // Update in-memory state
        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace.to_string(), merged.clone());

        Ok(merged)
    }

    /// Delete an instance. Removes YAML from disk and from in-memory state.
    pub fn delete_instance(
        &self,
        type_name: &str,
        namespace: &str,
    ) -> Result<(), Error> {
        // Verify it exists
        {
            let instances = self.instances.read().unwrap();
            let exists = instances
                .get(type_name)
                .map(|type_map| type_map.contains_key(namespace))
                .unwrap_or(false);
            if !exists {
                return Err(Error::NotFound(namespace.to_string()));
            }
        }

        // Delete from disk
        let file_path = self.resolve_file_path(namespace);
        delete_instance_yaml(&file_path)?;

        // Remove from in-memory state
        let mut instances = self.instances.write().unwrap();
        if let Some(type_map) = instances.get_mut(type_name) {
            type_map.remove(namespace);
        }

        Ok(())
    }

    /// Delete all instances of a type whose namespace starts with a given prefix.
    /// Also removes their YAML files from disk.
    pub fn delete_instances_by_prefix(
        &self,
        type_name: &str,
        namespace_prefix: &str,
    ) -> Result<Vec<String>, Error> {
        let to_delete: Vec<String> = {
            let instances = self.instances.read().unwrap();
            instances
                .get(type_name)
                .map(|type_map| {
                    type_map
                        .keys()
                        .filter(|ns| ns.starts_with(namespace_prefix))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        };

        // Delete files from disk — namespace IS the path relative to instances_dir
        for ns in &to_delete {
            let file_path = self.resolve_file_path(ns);
            let _ = delete_instance_yaml(&file_path);
        }

        // Remove from in-memory state
        let mut instances = self.instances.write().unwrap();
        if let Some(type_map) = instances.get_mut(type_name) {
            for ns in &to_delete {
                type_map.remove(ns);
            }
        }

        Ok(to_delete)
    }

    /// Resolve file path for an instance — namespace IS the path relative to instances_dir.
    fn resolve_file_path(&self, namespace: &str) -> PathBuf {
        self.instances_dir.join(format!("{namespace}.yaml"))
    }

}

fn write_instance_yaml(path: &Path, fields: &HashMap<String, String>) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // List fields are stored in the registry as JSON-encoded strings (e.g. `["a","b"]`).
    // Detect them and write proper YAML sequences so they round-trip correctly on re-read.
    let mut map = serde_yaml::Mapping::new();
    for (k, v) in fields {
        let yaml_val = if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(v) {
            serde_yaml::Value::Sequence(arr.into_iter().map(json_to_yaml_scalar).collect())
        } else {
            serde_yaml::Value::String(v.clone())
        };
        map.insert(serde_yaml::Value::String(k.clone()), yaml_val);
    }
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(map))?;
    std::fs::write(path, yaml)?;
    Ok(())
}

fn json_to_yaml_scalar(v: serde_json::Value) -> serde_yaml::Value {
    match v {
        serde_json::Value::String(s) => serde_yaml::Value::String(s),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_yaml::Value::Number(f.into())
            } else {
                serde_yaml::Value::Null
            }
        }
        serde_json::Value::Bool(b) => serde_yaml::Value::Bool(b),
        serde_json::Value::Array(arr) => {
            serde_yaml::Value::Sequence(arr.into_iter().map(json_to_yaml_scalar).collect())
        }
        _ => serde_yaml::Value::Null,
    }
}

fn delete_instance_yaml(path: &Path) -> Result<(), Error> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    // Clean up empty parent directories up to (but not including) the instances root
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.ends_with("instances") {
            break;
        }
        match std::fs::remove_dir(d) {
            Ok(()) => dir = d.parent(),  // was empty, keep climbing
            Err(_) => break,              // not empty or not removable, stop
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry(base: &Path) -> SchemaRegistry {
        SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: base.to_path_buf(),
        }
    }

    fn collection_ns(user: &str, project: &str, version: &str, name: &str) -> String {
        format!("{user}/{project}/versions/{version}/collection/{name}")
    }

    fn field_ns(user: &str, project: &str, version: &str, collection: &str, name: &str) -> String {
        format!("{user}/{project}/versions/{version}/field/{collection}/{name}")
    }

    #[test]
    fn test_create_and_get_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());
        fields.insert("label".to_string(), "Account".to_string());

        let ns = collection_ns("ben", "crm", "0.0.1-dev", "account");
        registry.create_instance("collection", &ns, fields).unwrap();

        // Verify in-memory
        let result = registry.get_instance("collection", &ns);
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("label").unwrap(), "Account");

        // Verify on disk
        let file_path = dir.path().join(format!("{ns}.yaml"));
        assert!(file_path.exists());
    }

    #[test]
    fn test_create_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        let ns = collection_ns("ben", "crm", "0.0.1-dev", "account");
        registry.create_instance("collection", &ns, fields.clone()).unwrap();
        let result = registry.create_instance("collection", &ns, fields);
        assert!(result.is_err());
    }

    #[test]
    fn test_has_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        let ns = collection_ns("ben", "crm", "0.0.1-dev", "account");
        registry.create_instance("collection", &ns, fields).unwrap();

        assert!(registry.has_instance("collection", &ns));
        assert!(!registry.has_instance("collection", &collection_ns("ben", "crm", "0.0.1-dev", "missing")));
    }

    #[test]
    fn test_list_instances() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut f1 = HashMap::new();
        f1.insert("name".to_string(), "account".to_string());
        let mut f2 = HashMap::new();
        f2.insert("name".to_string(), "contact".to_string());

        let ns1 = collection_ns("ben", "crm", "0.0.1-dev", "account");
        let ns2 = collection_ns("ben", "crm", "0.0.1-dev", "contact");
        let ns3 = collection_ns("ben", "cars", "0.0.1-dev", "vehicle");

        registry.create_instance("collection", &ns1, f1).unwrap();
        registry.create_instance("collection", &ns2, f2).unwrap();
        registry.create_instance("collection", &ns3, HashMap::new()).unwrap();

        let crm_list = registry.list_instances("collection", "ben/crm/");
        assert_eq!(crm_list.len(), 2);

        let cars_list = registry.list_instances("collection", "ben/cars/");
        assert_eq!(cars_list.len(), 1);
    }

    #[test]
    fn test_update_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());
        fields.insert("label".to_string(), "Account".to_string());

        let ns = collection_ns("ben", "crm", "0.0.1-dev", "account");
        registry.create_instance("collection", &ns, fields).unwrap();

        let mut updates = HashMap::new();
        updates.insert("label".to_string(), "Customer Account".to_string());

        let result = registry.update_instance("collection", &ns, updates).unwrap();

        assert_eq!(result.get("label").unwrap(), "Customer Account");
        assert_eq!(result.get("name").unwrap(), "account"); // unchanged field preserved
    }

    #[test]
    fn test_update_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let result = registry.update_instance(
            "collection",
            &collection_ns("ben", "crm", "0.0.1-dev", "missing"),
            HashMap::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        let ns = collection_ns("ben", "crm", "0.0.1-dev", "account");
        registry.create_instance("collection", &ns, fields).unwrap();

        registry.delete_instance("collection", &ns).unwrap();

        assert!(!registry.has_instance("collection", &ns));

        let file_path = dir.path().join(format!("{ns}.yaml"));
        assert!(!file_path.exists());
    }

    #[test]
    fn test_delete_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let result = registry.delete_instance("collection", &collection_ns("ben", "crm", "0.0.1-dev", "missing"));
        assert!(result.is_err());
    }

    #[test]
    fn test_create_nested_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "company".to_string());
        fields.insert("collection".to_string(), "account".to_string());
        fields.insert("type".to_string(), "string".to_string());

        let ns = field_ns("ben", "crm", "0.0.1-dev", "account", "company");
        registry.create_instance("field", &ns, fields).unwrap();

        let result = registry.get_instance("field", &ns);
        assert!(result.is_some());

        let file_path = dir.path().join(format!("{ns}.yaml"));
        assert!(file_path.exists());
    }

    #[test]
    fn test_delete_instances_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        // Create fields for account
        let mut f1 = HashMap::new();
        f1.insert("name".to_string(), "company".to_string());
        let ns1 = field_ns("ben", "crm", "0.0.1-dev", "account", "company");
        registry.create_instance("field", &ns1, f1).unwrap();

        let mut f2 = HashMap::new();
        f2.insert("name".to_string(), "active".to_string());
        let ns2 = field_ns("ben", "crm", "0.0.1-dev", "account", "active");
        registry.create_instance("field", &ns2, f2).unwrap();

        // Create a field for a different collection
        let mut f3 = HashMap::new();
        f3.insert("name".to_string(), "first_name".to_string());
        let ns3 = field_ns("ben", "crm", "0.0.1-dev", "contact", "first_name");
        registry.create_instance("field", &ns3, f3).unwrap();

        // Delete all account fields
        let account_prefix = format!("ben/crm/versions/0.0.1-dev/field/account/");
        let deleted = registry.delete_instances_by_prefix("field", &account_prefix).unwrap();
        assert_eq!(deleted.len(), 2);

        // Contact field should still exist
        assert!(registry.has_instance("field", &ns3));
        assert!(!registry.has_instance("field", &ns1));
    }

    #[test]
    fn test_config_crud() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("site_id".to_string(), "studio".to_string());
        fields.insert("name".to_string(), "Loco Studio".to_string());

        let ns = "loco/studio/sites/studio";

        // Create
        registry.create_instance("site", ns, fields).unwrap();
        assert!(registry.has_instance("site", ns));
        assert!(!registry.has_instance("site", "loco/studio/sites/other"));

        // Get
        let result = registry.get_instance("site", ns).unwrap();
        assert_eq!(result.get("name").unwrap(), "Loco Studio");

        // List
        let list = registry.list_all_instances("site");
        assert_eq!(list.len(), 1);

        // Find
        let found = registry.find_instance("site", "site_id", "studio");
        assert!(found.is_some());
        assert_eq!(found.unwrap().0, ns);

        // Update
        let mut updates = HashMap::new();
        updates.insert("name".to_string(), "Studio Updated".to_string());
        let updated = registry.update_instance("site", ns, updates).unwrap();
        assert_eq!(updated.get("name").unwrap(), "Studio Updated");
        assert_eq!(updated.get("site_id").unwrap(), "studio"); // preserved

        // Verify on disk
        let file_path = dir.path().join(format!("{ns}.yaml"));
        assert!(file_path.exists());

        // Delete
        registry.delete_instance("site", ns).unwrap();
        assert!(!registry.has_instance("site", ns));
        assert!(!file_path.exists());
    }

    #[test]
    fn test_config_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = test_registry(dir.path());

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "test".to_string());

        let ns = "loco/studio/sites/test";
        registry.create_instance("site", ns, fields.clone()).unwrap();
        assert!(registry.create_instance("site", ns, fields).is_err());
    }

    /// Regression test: list fields written via `create_instance` (or `update_instance`) must
    /// survive a registry reload. Previously, lists were serialized as JSON strings into the
    /// YAML file (e.g. `'["a","b"]'`), and `coerce_value` would then call `as_sequence()` on
    /// that string, get `None`, and silently default the field to an empty list.
    #[test]
    fn test_list_field_roundtrips_through_disk() {
        use crate::types::{FieldType, Property};

        let dir = tempfile::tempdir().unwrap();
        let manifest_type = TypeDef {
            name: "manifest".to_string(),
            description: "".to_string(),
            path_template: "${project}/versions/${version}/manifest".to_string(),
            properties: vec![
                Property { name: "project".to_string(), field_type: FieldType::Slug { segments: 2 }, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::Slug { segments: 1 }, create_only: true },
                Property { name: "dependencies".to_string(), field_type: FieldType::List(Box::new(FieldType::String)), create_only: false },
            ],
        };

        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        // The registry API stores list fields as JSON-encoded strings.
        let mut fields = HashMap::new();
        fields.insert(
            "dependencies".to_string(),
            r#"["loco/core@0.0.1-dev","alice/billing@0.1.0"]"#.to_string(),
        );

        // Namespace IS the raw file path (minus .yaml)
        let ns = "ben/crm/versions/0.0.1-dev/manifest";
        registry.create_instance("manifest", ns, fields).unwrap();

        // The YAML file must have a proper sequence, not the raw JSON string.
        let yaml_path = dir.path().join("ben/crm/versions/0.0.1-dev/manifest.yaml");
        let yaml_content = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(
            !yaml_content.contains('['),
            "YAML on disk should not contain a JSON array literal; got:\n{yaml_content}"
        );

        // Reload the registry from disk and verify both items are present.
        let reloaded = SchemaRegistry::load(dir.path(), &[manifest_type]).unwrap();
        let instance = reloaded
            .get_instance("manifest", ns)
            .expect("instance should survive reload");

        let raw = instance.get("dependencies").expect("dependencies field missing after reload");
        let deps: Vec<String> = serde_json::from_str(raw)
            .expect("dependencies should be a valid JSON array in the registry");
        assert_eq!(deps, vec!["loco/core@0.0.1-dev", "alice/billing@0.1.0"]);
    }
}
