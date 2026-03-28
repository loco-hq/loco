use std::collections::HashSet;
use std::path::Path;

use crate::error::Error;
use crate::namespace::{self, NamespaceConfig};
use crate::types::{FieldType, FieldValue, Instance, TypeDef};

/// Result of scanning instances, including namespace configs.
#[derive(Debug)]
pub struct ScanResult {
    pub instances: Vec<Instance>,
    pub namespaces: Vec<ScannedNamespace>,
}

/// A namespace found during scanning.
#[derive(Debug)]
pub struct ScannedNamespace {
    pub user: String,
    pub project: String,
    pub version: String,
    pub config: NamespaceConfig,
}

/// Parse an instance YAML file, validating values against the type definition.
pub fn parse_instance(
    yaml: &str,
    type_def: &TypeDef,
    namespace: &str,
) -> Result<Instance, Error> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let mapping = value
        .as_mapping()
        .ok_or(Error::MissingField("root mapping"))?;

    let mut values = Vec::new();
    for prop in &type_def.properties {
        let key = serde_yaml::Value::String(prop.name.clone());
        let val = mapping
            .get(&key)
            .ok_or(Error::MissingField("instance field"))?;

        let field_value = match prop.field_type {
            FieldType::String => {
                let s = val.as_str().ok_or_else(|| {
                    Error::InvalidValue(format!("expected string for '{}'", prop.name))
                })?;
                FieldValue::String(s.to_string())
            }
            FieldType::Integer => {
                let i = val.as_i64().ok_or_else(|| {
                    Error::InvalidValue(format!("expected integer for '{}'", prop.name))
                })?;
                FieldValue::Integer(i)
            }
            FieldType::Float => {
                let f = val.as_f64().ok_or_else(|| {
                    Error::InvalidValue(format!("expected float for '{}'", prop.name))
                })?;
                FieldValue::Float(f)
            }
            FieldType::Boolean => {
                let b = val.as_bool().ok_or_else(|| {
                    Error::InvalidValue(format!("expected boolean for '{}'", prop.name))
                })?;
                FieldValue::Boolean(b)
            }
        };
        values.push((prop.name.clone(), field_value));
    }

    Ok(Instance {
        type_name: type_def.name.clone(),
        namespace: namespace.to_string(),
        values,
    })
}

/// Recursively collect all `.yaml` files under a directory.
fn collect_yaml_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, Error> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_yaml_files(&path)?);
        } else if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            files.push(path);
        }
    }
    Ok(files)
}

/// Walk `instances_dir` recursively to find YAML files at `user/project/version/type/item.yaml`.
/// Reads `loco.yaml` from each version folder for namespace config.
/// Matches the type folder name to a known TypeDef (case-insensitive).
/// Derives namespace as `user/project.item`.
/// Auto-includes `loco/core` if not explicitly listed as a dependency.
/// Validates that all declared dependencies exist on disk.
pub fn scan_instances(
    instances_dir: &Path,
    type_defs: &[TypeDef],
) -> Result<ScanResult, Error> {
    let mut instances = Vec::new();
    let mut namespaces = Vec::new();

    if !instances_dir.exists() {
        return Ok(ScanResult { instances, namespaces });
    }

    // Walk: instances_dir / user / project / version / type_name / item.yaml
    for user_entry in std::fs::read_dir(instances_dir)? {
        let user_entry = user_entry?;
        let user_path = user_entry.path();
        if !user_path.is_dir() {
            continue;
        }
        let user = user_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        for project_entry in std::fs::read_dir(&user_path)? {
            let project_entry = project_entry?;
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }
            let project = project_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            for version_entry in std::fs::read_dir(&project_path)? {
                let version_entry = version_entry?;
                let version_path = version_entry.path();
                if !version_path.is_dir() {
                    continue;
                }
                let version = version_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                // Read loco.yaml if present
                let config_path = version_path.join("loco.yaml");
                let config = if config_path.exists() {
                    let yaml = std::fs::read_to_string(&config_path)?;
                    namespace::parse_namespace_config(&yaml)?
                } else {
                    NamespaceConfig {
                        name: project.clone(),
                        dependencies: Vec::new(),
                    }
                };

                namespaces.push(ScannedNamespace {
                    user: user.clone(),
                    project: project.clone(),
                    version: version.clone(),
                    config,
                });

                for type_entry in std::fs::read_dir(&version_path)? {
                    let type_entry = type_entry?;
                    let type_path = type_entry.path();
                    if !type_path.is_dir() {
                        continue;
                    }
                    let type_folder = type_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    // Find matching TypeDef (case-insensitive match on folder name)
                    let type_def = type_defs.iter().find(|td| {
                        td.name.to_lowercase() == type_folder.to_lowercase()
                    });
                    let type_def = match type_def {
                        Some(td) => td,
                        None => continue,
                    };

                    // Recursively find all .yaml files under the type folder
                    let yaml_files = collect_yaml_files(&type_path)?;
                    for item_path in yaml_files {
                        let rel = item_path
                            .strip_prefix(&type_path)
                            .unwrap_or(&item_path);
                        // Build key from relative path minus .yaml extension
                        let key = rel
                            .with_extension("")
                            .to_string_lossy()
                            .to_string();

                        let namespace_str = format!("{user}/{project}.{key}");
                        let yaml = std::fs::read_to_string(&item_path)?;
                        let instance = parse_instance(&yaml, type_def, &namespace_str)?;
                        instances.push(instance);
                    }
                }
            }
        }
    }

    // Auto-include loco/core for namespaces that don't explicitly list it
    let has_loco_core = namespaces.iter().any(|ns| ns.user == "loco" && ns.project == "core");
    if has_loco_core {
        let loco_core_version = namespaces
            .iter()
            .find(|ns| ns.user == "loco" && ns.project == "core")
            .map(|ns| ns.version.clone())
            .unwrap();

        for ns in &mut namespaces {
            if ns.user == "loco" && ns.project == "core" {
                continue;
            }
            let has_core_dep = ns.config.dependencies.iter().any(|d| d.starts_with("loco/core@"));
            if !has_core_dep {
                ns.config.dependencies.push(format!("loco/core@{loco_core_version}"));
            }
        }
    }

    // Validate that all declared dependencies exist
    let available: HashSet<String> = namespaces
        .iter()
        .map(|ns| format!("{}/{}@{}", ns.user, ns.project, ns.version))
        .collect();

    for ns in &namespaces {
        for dep in &ns.config.dependencies {
            if !available.contains(dep) {
                return Err(Error::MissingDependency(format!(
                    "{}/{}@{} requires {dep}, but it was not found",
                    ns.user, ns.project, ns.version
                )));
            }
        }
    }

    // Sort for deterministic output
    instances.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    namespaces.sort_by(|a, b| {
        let a_key = format!("{}/{}", a.user, a.project);
        let b_key = format!("{}/{}", b.user, b.project);
        a_key.cmp(&b_key)
    });

    Ok(ScanResult { instances, namespaces })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldType, Property, TypeDef};

    fn collection_type_def() -> TypeDef {
        TypeDef {
            name: "Collection".to_string(),
            description: "A named collection of items".to_string(),
            file_path_template: None,
            properties: vec![
                Property {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                },
                Property {
                    name: "label".to_string(),
                    field_type: FieldType::String,
                },
                Property {
                    name: "label_plural".to_string(),
                    field_type: FieldType::String,
                },
            ],
        }
    }

    #[test]
    fn test_parse_instance() {
        let yaml = r#"
name: "opportunity"
label: "Opportunity"
label_plural: "Opportunities"
"#;
        let td = collection_type_def();
        let inst = parse_instance(yaml, &td, "ben/crm.opportunity").unwrap();
        assert_eq!(inst.type_name, "Collection");
        assert_eq!(inst.namespace, "ben/crm.opportunity");
        assert_eq!(inst.values.len(), 3);
        assert_eq!(inst.values[0], ("name".to_string(), FieldValue::String("opportunity".to_string())));
        assert_eq!(inst.values[1], ("label".to_string(), FieldValue::String("Opportunity".to_string())));
    }

    #[test]
    fn test_parse_instance_missing_field() {
        let yaml = r#"
name: "opportunity"
label: "Opportunity"
"#;
        let td = collection_type_def();
        let result = parse_instance(yaml, &td, "ben/crm.opportunity");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_instance_wrong_type() {
        let yaml = r#"
name: 123
label: "Opportunity"
label_plural: "Opportunities"
"#;
        let td = collection_type_def();
        let result = parse_instance(yaml, &td, "ben/crm.opportunity");
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_instances_nonexistent_dir() {
        let result = scan_instances(Path::new("/nonexistent"), &[]);
        assert!(result.is_ok());
        let scan = result.unwrap();
        assert!(scan.instances.is_empty());
        assert!(scan.namespaces.is_empty());
    }

    #[test]
    fn test_scan_instances_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create: base/ben/crm/0.0.1-dev/collection/opportunity.yaml
        let version_dir = base.join("ben").join("crm").join("0.0.1-dev");
        let collection_dir = version_dir.join("collection");
        std::fs::create_dir_all(&collection_dir).unwrap();
        std::fs::write(version_dir.join("loco.yaml"), "name: crm\n").unwrap();
        std::fs::write(
            collection_dir.join("opportunity.yaml"),
            "name: \"opportunity\"\nlabel: \"Opportunity\"\nlabel_plural: \"Opportunities\"\n",
        )
        .unwrap();
        std::fs::write(
            collection_dir.join("contact.yaml"),
            "name: \"contact\"\nlabel: \"Contact\"\nlabel_plural: \"Contacts\"\n",
        )
        .unwrap();

        let td = collection_type_def();
        let scan = scan_instances(base, &[td]).unwrap();
        assert_eq!(scan.instances.len(), 2);
        // Sorted alphabetically
        assert_eq!(scan.instances[0].namespace, "ben/crm.contact");
        assert_eq!(scan.instances[1].namespace, "ben/crm.opportunity");
        assert_eq!(scan.namespaces.len(), 1);
        assert_eq!(scan.namespaces[0].config.name, "crm");
    }

    #[test]
    fn test_scan_nested_instances() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        fn field_type_def() -> TypeDef {
            TypeDef {
                name: "Field".to_string(),
                description: "A field belonging to a collection".to_string(),
                file_path_template: Some("${collection}/${name}".to_string()),
                properties: vec![
                    Property {
                        name: "name".to_string(),
                        field_type: FieldType::String,
                    },
                    Property {
                        name: "collection".to_string(),
                        field_type: FieldType::String,
                    },
                ],
            }
        }

        // Create nested: base/ben/crm/0.0.1-dev/field/account/company.yaml
        let version_dir = base.join("ben").join("crm").join("0.0.1-dev");
        let account_dir = version_dir.join("field").join("account");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(version_dir.join("loco.yaml"), "name: crm\n").unwrap();
        std::fs::write(
            account_dir.join("company.yaml"),
            "name: \"company\"\ncollection: \"account\"\n",
        )
        .unwrap();

        let contact_dir = version_dir.join("field").join("contact");
        std::fs::create_dir_all(&contact_dir).unwrap();
        std::fs::write(
            contact_dir.join("first_name.yaml"),
            "name: \"first_name\"\ncollection: \"contact\"\n",
        )
        .unwrap();

        let td = field_type_def();
        let scan = scan_instances(base, &[td]).unwrap();
        assert_eq!(scan.instances.len(), 2);
        assert_eq!(scan.instances[0].namespace, "ben/crm.account/company");
        assert_eq!(scan.instances[1].namespace, "ben/crm.contact/first_name");
    }

    #[test]
    fn test_auto_include_loco_core() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create loco/core
        let core_dir = base.join("loco").join("core").join("0.0.1-dev");
        std::fs::create_dir_all(&core_dir).unwrap();
        std::fs::write(core_dir.join("loco.yaml"), "name: core\n").unwrap();

        // Create ben/crm with no explicit loco/core dependency
        let crm_dir = base.join("ben").join("crm").join("0.0.1-dev");
        std::fs::create_dir_all(&crm_dir).unwrap();
        std::fs::write(crm_dir.join("loco.yaml"), "name: crm\n").unwrap();

        let scan = scan_instances(base, &[]).unwrap();
        let crm_ns = scan.namespaces.iter().find(|ns| ns.project == "crm").unwrap();
        assert!(crm_ns.config.dependencies.contains(&"loco/core@0.0.1-dev".to_string()));

        // loco/core itself should not have itself as a dependency
        let core_ns = scan.namespaces.iter().find(|ns| ns.project == "core").unwrap();
        assert!(core_ns.config.dependencies.is_empty());
    }

    #[test]
    fn test_missing_dependency_error() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let crm_dir = base.join("ben").join("crm").join("0.0.1-dev");
        std::fs::create_dir_all(&crm_dir).unwrap();
        std::fs::write(
            crm_dir.join("loco.yaml"),
            "name: crm\ndependencies:\n  - alice/billing@0.1.0\n",
        )
        .unwrap();

        let result = scan_instances(base, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("alice/billing@0.1.0"));
    }
}
