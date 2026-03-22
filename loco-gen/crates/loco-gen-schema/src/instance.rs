use std::path::Path;

use crate::error::Error;
use crate::types::{FieldType, FieldValue, Instance, TypeDef};

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

/// Walk `instances_dir` recursively to find YAML files at `user/project/type/item.yaml`.
/// Matches the type folder name to a known TypeDef (case-insensitive).
/// Derives namespace as `user/project.item`.
pub fn scan_instances(
    instances_dir: &Path,
    type_defs: &[TypeDef],
) -> Result<Vec<Instance>, Error> {
    let mut instances = Vec::new();

    if !instances_dir.exists() {
        return Ok(instances);
    }

    // Walk: instances_dir / user / project / type_name / item.yaml
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

            for type_entry in std::fs::read_dir(&project_path)? {
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

                    let namespace = format!("{user}/{project}.{key}");
                    let yaml = std::fs::read_to_string(&item_path)?;
                    let instance = parse_instance(&yaml, type_def, &namespace)?;
                    instances.push(instance);
                }
            }
        }
    }

    // Sort for deterministic output
    instances.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    Ok(instances)
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
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_scan_instances_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create: base/ben/crm/collection/opportunity.yaml
        let collection_dir = base.join("ben").join("crm").join("collection");
        std::fs::create_dir_all(&collection_dir).unwrap();
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
        let instances = scan_instances(base, &[td]).unwrap();
        assert_eq!(instances.len(), 2);
        // Sorted alphabetically
        assert_eq!(instances[0].namespace, "ben/crm.contact");
        assert_eq!(instances[1].namespace, "ben/crm.opportunity");
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

        // Create nested: base/ben/crm/field/account/company.yaml
        let account_dir = base.join("ben").join("crm").join("field").join("account");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(
            account_dir.join("company.yaml"),
            "name: \"company\"\ncollection: \"account\"\n",
        )
        .unwrap();

        let contact_dir = base.join("ben").join("crm").join("field").join("contact");
        std::fs::create_dir_all(&contact_dir).unwrap();
        std::fs::write(
            contact_dir.join("first_name.yaml"),
            "name: \"first_name\"\ncollection: \"contact\"\n",
        )
        .unwrap();

        let td = field_type_def();
        let instances = scan_instances(base, &[td]).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].namespace, "ben/crm.account/company");
        assert_eq!(instances[1].namespace, "ben/crm.contact/first_name");
    }
}
