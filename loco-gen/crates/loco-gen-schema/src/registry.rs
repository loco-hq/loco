use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::error::Error;
use crate::instance;
use crate::types::TypeDef;

/// Thread-safe in-memory registry for instances of any TypeDef.
/// Works with generic type names — knows nothing about domain concepts
/// like "collection" or "field".
type InstanceMap = HashMap<String, HashMap<String, HashMap<String, String>>>;

pub struct SchemaRegistry {
    /// type_name → { namespace → field_values }
    instances: RwLock<InstanceMap>,
    instances_dir: PathBuf,
}

impl SchemaRegistry {
    /// Load all instances from disk into the registry.
    pub fn load(instances_dir: &Path, type_defs: &[TypeDef]) -> Result<Self, Error> {
        let scan = instance::scan_instances(instances_dir, type_defs)?;

        let mut instances: HashMap<String, HashMap<String, HashMap<String, String>>> =
            HashMap::new();

        for inst in &scan.instances {
            let type_map = instances.entry(inst.type_name.to_lowercase()).or_default();
            let mut fields = HashMap::new();
            for (key, val) in &inst.values {
                let str_val = match val {
                    crate::types::FieldValue::String(s) => s.clone(),
                    crate::types::FieldValue::Integer(i) => i.to_string(),
                    crate::types::FieldValue::Float(f) => f.to_string(),
                    crate::types::FieldValue::Boolean(b) => b.to_string(),
                };
                fields.insert(key.clone(), str_val);
            }
            type_map.insert(inst.namespace.clone(), fields);
        }

        Ok(SchemaRegistry {
            instances: RwLock::new(instances),
            instances_dir: instances_dir.to_path_buf(),
        })
    }

    /// List all instances of a type, filtered by user/project namespace prefix.
    pub fn list_instances(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
    ) -> Vec<(String, HashMap<String, String>)> {
        let instances = self.instances.read().unwrap();
        let prefix = format!("{user}/{project}.");
        instances
            .get(type_name)
            .map(|type_map| {
                type_map
                    .iter()
                    .filter(|(ns, _)| ns.starts_with(&prefix))
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

    /// Create a new instance. Writes YAML to disk and updates in-memory state.
    pub fn create_instance(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
        version: &str,
        name: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        let namespace = format!("{user}/{project}.{name}");

        // Check for duplicates
        {
            let instances = self.instances.read().unwrap();
            if let Some(type_map) = instances.get(type_name) {
                if type_map.contains_key(&namespace) {
                    return Err(Error::AlreadyExists(namespace));
                }
            }
        }

        // Write to disk
        let file_path = self.instance_path(user, project, version, type_name, name);
        write_instance_yaml(&file_path, &fields)?;

        // Update in-memory state
        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace, fields.clone());

        Ok(fields)
    }

    /// Create a nested instance (e.g., fields organized under a parent).
    /// The name is the full relative path (e.g., "account/company").
    #[allow(clippy::too_many_arguments)]
    pub fn create_nested_instance(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
        version: &str,
        parent: &str,
        name: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        let namespace = format!("{user}/{project}.{parent}/{name}");

        // Check for duplicates
        {
            let instances = self.instances.read().unwrap();
            if let Some(type_map) = instances.get(type_name) {
                if type_map.contains_key(&namespace) {
                    return Err(Error::AlreadyExists(namespace));
                }
            }
        }

        // Write to disk — nested under parent directory
        let file_path = self
            .instances_dir
            .join(user)
            .join(project)
            .join(version)
            .join(type_name)
            .join(parent)
            .join(format!("{name}.yaml"));
        write_instance_yaml(&file_path, &fields)?;

        // Update in-memory state
        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace, fields.clone());

        Ok(fields)
    }

    /// Update an existing instance. Writes YAML to disk and updates in-memory state.
    pub fn update_instance(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
        version: &str,
        name: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        let namespace = format!("{user}/{project}.{name}");

        // Verify it exists and get current fields
        let current = {
            let instances = self.instances.read().unwrap();
            instances
                .get(type_name)
                .and_then(|type_map| type_map.get(&namespace).cloned())
                .ok_or_else(|| Error::NotFound(namespace.clone()))?
        };

        // Merge: new fields override, keep existing for unset fields
        let mut merged = current;
        for (k, v) in &fields {
            merged.insert(k.clone(), v.clone());
        }

        // Write to disk
        let file_path = self.instance_path(user, project, version, type_name, name);
        write_instance_yaml(&file_path, &merged)?;

        // Update in-memory state
        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace, merged.clone());

        Ok(merged)
    }

    /// Update a nested instance.
    #[allow(clippy::too_many_arguments)]
    pub fn update_nested_instance(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
        version: &str,
        parent: &str,
        name: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, Error> {
        let namespace = format!("{user}/{project}.{parent}/{name}");

        let current = {
            let instances = self.instances.read().unwrap();
            instances
                .get(type_name)
                .and_then(|type_map| type_map.get(&namespace).cloned())
                .ok_or_else(|| Error::NotFound(namespace.clone()))?
        };

        let mut merged = current;
        for (k, v) in &fields {
            merged.insert(k.clone(), v.clone());
        }

        let file_path = self
            .instances_dir
            .join(user)
            .join(project)
            .join(version)
            .join(type_name)
            .join(parent)
            .join(format!("{name}.yaml"));
        write_instance_yaml(&file_path, &merged)?;

        let mut instances = self.instances.write().unwrap();
        let type_map = instances.entry(type_name.to_string()).or_default();
        type_map.insert(namespace, merged.clone());

        Ok(merged)
    }

    /// Delete an instance. Removes YAML from disk and from in-memory state.
    pub fn delete_instance(
        &self,
        type_name: &str,
        user: &str,
        project: &str,
        version: &str,
        name: &str,
    ) -> Result<(), Error> {
        let namespace = format!("{user}/{project}.{name}");

        // Verify it exists
        {
            let instances = self.instances.read().unwrap();
            let exists = instances
                .get(type_name)
                .map(|type_map| type_map.contains_key(&namespace))
                .unwrap_or(false);
            if !exists {
                return Err(Error::NotFound(namespace));
            }
        }

        // Delete from disk
        let file_path = self.instance_path(user, project, version, type_name, name);
        delete_instance_yaml(&file_path)?;

        // Remove from in-memory state
        let mut instances = self.instances.write().unwrap();
        if let Some(type_map) = instances.get_mut(type_name) {
            type_map.remove(&namespace);
        }

        Ok(())
    }

    /// Delete all instances of a type whose namespace starts with a given prefix.
    /// Also removes their YAML files from disk.
    pub fn delete_instances_by_prefix(
        &self,
        type_name: &str,
        namespace_prefix: &str,
        version: &str,
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

        // Delete files from disk
        for ns in &to_delete {
            // Derive path from namespace: "user/project.parent/name" → file path
            let after_dot = ns.split_once('.').map(|(_, rest)| rest).unwrap_or("");
            let parts: Vec<&str> = ns.split_once('.').map(|(pre, _)| pre).unwrap_or("").split('/').collect();
            let (user, project) = if parts.len() >= 2 {
                (parts[0], parts[1])
            } else {
                continue;
            };
            let rel_path = after_dot.replace('/', std::path::MAIN_SEPARATOR_STR);
            let file_path = self
                .instances_dir
                .join(user)
                .join(project)
                .join(version)
                .join(type_name)
                .join(format!("{rel_path}.yaml"));
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

    fn instance_path(
        &self,
        user: &str,
        project: &str,
        version: &str,
        type_name: &str,
        name: &str,
    ) -> PathBuf {
        self.instances_dir
            .join(user)
            .join(project)
            .join(version)
            .join(type_name)
            .join(format!("{name}.yaml"))
    }
}

fn write_instance_yaml(path: &Path, fields: &HashMap<String, String>) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(fields)?;
    std::fs::write(path, yaml)?;
    Ok(())
}

fn delete_instance_yaml(path: &Path) -> Result<(), Error> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());
        fields.insert("label".to_string(), "Account".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields)
            .unwrap();

        // Verify in-memory
        let result = registry.get_instance("collection", "ben/crm.account");
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("label").unwrap(), "Account");

        // Verify on disk
        let file_path = dir
            .path()
            .join("ben/crm/0.0.1-dev/collection/account.yaml");
        assert!(file_path.exists());
    }

    #[test]
    fn test_create_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields.clone())
            .unwrap();
        let result =
            registry.create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields);
        assert!(result.is_err());
    }

    #[test]
    fn test_has_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields)
            .unwrap();

        assert!(registry.has_instance("collection", "ben/crm.account"));
        assert!(!registry.has_instance("collection", "ben/crm.missing"));
    }

    #[test]
    fn test_list_instances() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut f1 = HashMap::new();
        f1.insert("name".to_string(), "account".to_string());
        let mut f2 = HashMap::new();
        f2.insert("name".to_string(), "contact".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", f1)
            .unwrap();
        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "contact", f2)
            .unwrap();
        registry
            .create_instance("collection", "ben", "cars", "0.0.1-dev", "vehicle", HashMap::new())
            .unwrap();

        let crm_list = registry.list_instances("collection", "ben", "crm");
        assert_eq!(crm_list.len(), 2);

        let cars_list = registry.list_instances("collection", "ben", "cars");
        assert_eq!(cars_list.len(), 1);
    }

    #[test]
    fn test_update_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());
        fields.insert("label".to_string(), "Account".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields)
            .unwrap();

        let mut updates = HashMap::new();
        updates.insert("label".to_string(), "Customer Account".to_string());

        let result = registry
            .update_instance("collection", "ben", "crm", "0.0.1-dev", "account", updates)
            .unwrap();

        assert_eq!(result.get("label").unwrap(), "Customer Account");
        assert_eq!(result.get("name").unwrap(), "account"); // unchanged field preserved
    }

    #[test]
    fn test_update_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let result = registry.update_instance(
            "collection",
            "ben",
            "crm",
            "0.0.1-dev",
            "missing",
            HashMap::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "account".to_string());

        registry
            .create_instance("collection", "ben", "crm", "0.0.1-dev", "account", fields)
            .unwrap();

        registry
            .delete_instance("collection", "ben", "crm", "0.0.1-dev", "account")
            .unwrap();

        assert!(!registry.has_instance("collection", "ben/crm.account"));

        let file_path = dir
            .path()
            .join("ben/crm/0.0.1-dev/collection/account.yaml");
        assert!(!file_path.exists());
    }

    #[test]
    fn test_delete_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let result =
            registry.delete_instance("collection", "ben", "crm", "0.0.1-dev", "missing");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_nested_instance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        let mut fields = HashMap::new();
        fields.insert("name".to_string(), "company".to_string());
        fields.insert("collection".to_string(), "account".to_string());
        fields.insert("type".to_string(), "string".to_string());

        registry
            .create_nested_instance("field", "ben", "crm", "0.0.1-dev", "account", "company", fields)
            .unwrap();

        let result = registry.get_instance("field", "ben/crm.account/company");
        assert!(result.is_some());

        let file_path = dir
            .path()
            .join("ben/crm/0.0.1-dev/field/account/company.yaml");
        assert!(file_path.exists());
    }

    #[test]
    fn test_delete_instances_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let registry = SchemaRegistry {
            instances: RwLock::new(HashMap::new()),
            instances_dir: dir.path().to_path_buf(),
        };

        // Create fields for account
        let mut f1 = HashMap::new();
        f1.insert("name".to_string(), "company".to_string());
        registry
            .create_nested_instance("field", "ben", "crm", "0.0.1-dev", "account", "company", f1)
            .unwrap();

        let mut f2 = HashMap::new();
        f2.insert("name".to_string(), "active".to_string());
        registry
            .create_nested_instance("field", "ben", "crm", "0.0.1-dev", "account", "active", f2)
            .unwrap();

        // Create a field for a different collection
        let mut f3 = HashMap::new();
        f3.insert("name".to_string(), "first_name".to_string());
        registry
            .create_nested_instance("field", "ben", "crm", "0.0.1-dev", "contact", "first_name", f3)
            .unwrap();

        // Delete all account fields
        let deleted = registry
            .delete_instances_by_prefix("field", "ben/crm.account/", "0.0.1-dev")
            .unwrap();
        assert_eq!(deleted.len(), 2);

        // Contact field should still exist
        assert!(registry.has_instance("field", "ben/crm.contact/first_name"));
        assert!(!registry.has_instance("field", "ben/crm.account/company"));
    }
}
