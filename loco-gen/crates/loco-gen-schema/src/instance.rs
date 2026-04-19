use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::Error;
use crate::types::{FieldType, FieldValue, Instance, TypeDef};

/// Parse an instance YAML file, validating values against the type definition.
/// `vars` supplies values for template variables extracted from the file path;
/// these become implicit `String` fields on the generated struct unless a
/// declared property shadows them.
pub fn parse_instance(
    yaml: &str,
    type_def: &TypeDef,
    namespace: &str,
    vars: &HashMap<String, String>,
) -> Result<Instance, Error> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let mapping = value
        .as_mapping()
        .ok_or(Error::MissingField("root mapping"))?;

    let template_vars = type_def.template_vars();
    let template_var_set: HashSet<&str> = template_vars.iter().map(|s| s.as_str()).collect();

    let mut values = Vec::new();
    for prop in &type_def.properties {
        let field_value = if template_var_set.contains(prop.name.as_str()) {
            // Template vars source their value from the file path, not the YAML body.
            FieldValue::String(vars.get(&prop.name).cloned().unwrap_or_default())
        } else {
            let key = serde_yaml::Value::String(prop.name.clone());
            // Loose on load: missing or mistyped fields fall back to type defaults.
            match mapping.get(&key) {
                Some(val) => coerce_value(val, &prop.field_type),
                None => default_value(&prop.field_type),
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

/// Loose-on-load coercion of a YAML value into a `FieldValue` of the expected type.
fn coerce_value(val: &serde_yaml::Value, field_type: &FieldType) -> FieldValue {
    match field_type {
        FieldType::String => FieldValue::String(val.as_str().map(|s| s.to_string()).unwrap_or_default()),
        FieldType::Integer => FieldValue::Integer(val.as_i64().unwrap_or(0)),
        FieldType::Float => FieldValue::Float(val.as_f64().unwrap_or(0.0)),
        FieldType::Boolean => FieldValue::Boolean(val.as_bool().unwrap_or(false)),
        FieldType::List(inner) => {
            let items = val
                .as_sequence()
                .map(|seq| seq.iter().map(|v| coerce_value(v, inner)).collect())
                .unwrap_or_default();
            FieldValue::List(items)
        }
    }
}

/// Default value for a `FieldType` when the field is missing from the YAML.
fn default_value(field_type: &FieldType) -> FieldValue {
    match field_type {
        FieldType::String => FieldValue::String(String::new()),
        FieldType::Integer => FieldValue::Integer(0),
        FieldType::Float => FieldValue::Float(0.0),
        FieldType::Boolean => FieldValue::Boolean(false),
        FieldType::List(_) => FieldValue::List(Vec::new()),
    }
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

/// Walk `instances_dir` recursively, matching every YAML file against the
/// provided type definitions using their `filePathTemplate`. Returns the
/// resulting instances, sorted by namespace for deterministic output.
///
/// Files that don't match any type template are silently ignored.
pub fn scan_all(
    instances_dir: &Path,
    type_defs: &[TypeDef],
) -> Result<Vec<Instance>, Error> {
    let mut instances = Vec::new();

    if !instances_dir.exists() {
        return Ok(instances);
    }

    for file_path in &collect_yaml_files(instances_dir)? {
        let rel = file_path
            .strip_prefix(instances_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        for type_def in type_defs {
            if let Some(vars) = extract_template_vars(&rel, &type_def.file_path_template) {
                let namespace_str = derive_namespace(&type_def.file_path_template, &vars, &rel);
                let yaml = std::fs::read_to_string(file_path)?;
                let instance = parse_instance(&yaml, type_def, &namespace_str, &vars)?;
                instances.push(instance);
                break;
            }
        }
    }

    instances.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    Ok(instances)
}

/// Derive instance namespace/key from extracted template variables.
/// For versioned types (template contains `${version}`): `{project}.{item_key}`
///   where item_key is the relative path after the version segment, minus `.yaml`.
/// For unversioned types: the relative path minus `.yaml`.
fn derive_namespace(
    template: &str,
    vars: &HashMap<String, String>,
    rel_path: &str,
) -> String {
    let ns = vars
        .get("project")
        .or_else(|| vars.get("namespace"))
        .cloned()
        .unwrap_or_default();
    if template.contains("${version}") {
        // Versioned: build item_key from path segments after versions/{version}/
        // e.g., "ben/crm/versions/0.0.1-dev/field/account/company.yaml" → "account/company"
        // The item_key is everything after the type folder minus .yaml
        let version = vars.get("version").cloned().unwrap_or_default();
        let version_prefix = format!("{ns}/versions/{version}/");
        let after_version = rel_path.strip_prefix(&version_prefix).unwrap_or(rel_path);
        // Strip the type folder (first segment) and .yaml extension
        let item_key = if let Some((_type_folder, rest)) = after_version.split_once('/') {
            rest.strip_suffix(".yaml").unwrap_or(rest)
        } else {
            after_version.strip_suffix(".yaml").unwrap_or(after_version)
        };
        format!("{ns}.{item_key}")
    } else {
        // Unversioned: use relative path minus .yaml
        rel_path.strip_suffix(".yaml").unwrap_or(rel_path).to_string()
    }
}

/// Fill in a filePathTemplate with the given variable values.
/// E.g., `fill_template("${namespace}/project.yaml", {"namespace": "ben/crm"})` → `"ben/crm/project.yaml"`.
pub fn fill_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

/// Extract variable bindings from a relative path matched against a filePathTemplate.
/// Template segments are either literal (e.g., "sites") or variable (e.g., "${namespace}").
/// Variables can match one or more path segments (e.g., `${namespace}` matches `ben/crm`).
/// Returns `None` if the path does not match the template.
pub fn extract_template_vars(rel_path: &str, template: &str) -> Option<HashMap<String, String>> {
    let path_parts: Vec<&str> = rel_path.split('/').collect();
    let tmpl_parts: Vec<&str> = template.split('/').collect();
    let mut vars = HashMap::new();
    if extract_segments(&path_parts, &tmpl_parts, &mut vars) {
        Some(vars)
    } else {
        None
    }
}

fn extract_segments(
    path: &[&str],
    tmpl: &[&str],
    vars: &mut HashMap<String, String>,
) -> bool {
    if tmpl.is_empty() {
        return path.is_empty();
    }
    let t = tmpl[0];
    if let Some(var_name) = t.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
        // Pure variable segment: try consuming 1..n path segments
        for consume in 1..=path.len() {
            let captured = path[..consume].join("/");
            let mut branch = vars.clone();
            branch.insert(var_name.to_string(), captured);
            if extract_segments(&path[consume..], &tmpl[1..], &mut branch) {
                *vars = branch;
                return true;
            }
        }
        false
    } else if let Some((var_expr, suffix)) = parse_var_with_suffix(t) {
        // Variable with literal suffix (e.g., "${name}.yaml")
        if path.is_empty() {
            return false;
        }
        let seg = path[0];
        if let Some(captured) = seg.strip_suffix(suffix) {
            if captured.is_empty() {
                return false;
            }
            let mut branch = vars.clone();
            branch.insert(var_expr.to_string(), captured.to_string());
            if extract_segments(&path[1..], &tmpl[1..], &mut branch) {
                *vars = branch;
                return true;
            }
        }
        false
    } else {
        // Literal: must match exactly
        if path.is_empty() || path[0] != t {
            return false;
        }
        extract_segments(&path[1..], &tmpl[1..], vars)
    }
}

/// Parse a template segment like "${name}.yaml" into ("name", ".yaml").
/// Returns None if the segment doesn't contain a variable-with-suffix pattern.
fn parse_var_with_suffix(segment: &str) -> Option<(&str, &str)> {
    let start = segment.find("${")?;
    let end = segment.find('}')?;
    if start != 0 {
        return None;
    }
    let var_name = &segment[start + 2..end];
    let suffix = &segment[end + 1..];
    if suffix.is_empty() {
        return None; // Pure variable, handled elsewhere
    }
    Some((var_name, suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldType, Property, TypeDef};

    fn collection_type_def() -> TypeDef {
        TypeDef {
            name: "Collection".to_string(),
            description: "A named collection of items".to_string(),
            file_path_template: "${namespace}/versions/${version}/collection/${name}.yaml".to_string(),
            properties: vec![
                Property { name: "namespace".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "label".to_string(), field_type: FieldType::String, create_only: false },
                Property { name: "label_plural".to_string(), field_type: FieldType::String, create_only: false },
            ],
        }
    }

    fn field_type_def() -> TypeDef {
        TypeDef {
            name: "Field".to_string(),
            description: "A field belonging to a collection".to_string(),
            file_path_template: "${namespace}/versions/${version}/field/${collection}/${name}.yaml".to_string(),
            properties: vec![
                Property { name: "namespace".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "collection".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::String, create_only: true },
            ],
        }
    }

    #[test]
    fn test_parse_instance() {
        let yaml = r#"
label: "Opportunity"
label_plural: "Opportunities"
"#;
        let td = collection_type_def();
        let mut vars = HashMap::new();
        vars.insert("namespace".to_string(), "ben/crm".to_string());
        vars.insert("version".to_string(), "0.0.1-dev".to_string());
        vars.insert("name".to_string(), "opportunity".to_string());
        let inst = parse_instance(yaml, &td, "ben/crm.opportunity", &vars).unwrap();
        assert_eq!(inst.type_name, "Collection");
        assert_eq!(inst.namespace, "ben/crm.opportunity");
        // 5 declared properties: namespace, version, name (template vars) + label, label_plural
        assert_eq!(inst.values.len(), 5);
        assert!(inst.values.iter().any(|v| v == &("name".to_string(), FieldValue::String("opportunity".to_string()))));
        assert!(inst.values.iter().any(|v| v == &("label".to_string(), FieldValue::String("Opportunity".to_string()))));
    }

    #[test]
    fn test_parse_instance_missing_field_defaults() {
        // Loose on load: missing declared fields fall back to type defaults.
        let yaml = r#"
name: "opportunity"
label: "Opportunity"
"#;
        let td = collection_type_def();
        let inst = parse_instance(yaml, &td, "ben/crm.opportunity", &HashMap::new()).unwrap();
        assert!(inst.values.iter().any(|v|
            v == &("label_plural".to_string(), FieldValue::String(String::new()))));
    }

    #[test]
    fn test_parse_instance_wrong_type_defaults() {
        // Loose on load: type mismatches fall back to type defaults.
        let yaml = r#"
name: 123
label: "Opportunity"
label_plural: "Opportunities"
"#;
        let td = collection_type_def();
        let inst = parse_instance(yaml, &td, "ben/crm.opportunity", &HashMap::new()).unwrap();
        assert!(inst.values.iter().any(|v|
            v == &("name".to_string(), FieldValue::String(String::new()))));
    }

    #[test]
    fn test_parse_instance_list_of_strings() {
        let td = TypeDef {
            name: "Manifest".to_string(),
            description: "".to_string(),
            file_path_template: "${project}/versions/${version}/manifest.yaml".to_string(),
            properties: vec![
                Property { name: "project".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "dependencies".to_string(), field_type: FieldType::List(Box::new(FieldType::String)), create_only: false },
            ],
        };
        let yaml = "dependencies:\n  - loco/core@0.0.1\n  - alice/billing@0.1.0\n";
        let inst = parse_instance(yaml, &td, "ben/crm/versions/0.0.1-dev/manifest", &HashMap::new()).unwrap();
        let deps = inst.values.iter().find(|(k, _)| k == "dependencies").unwrap();
        match &deps.1 {
            FieldValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], FieldValue::String("loco/core@0.0.1".to_string()));
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn test_parse_instance_missing_list_defaults_empty() {
        let td = TypeDef {
            name: "Manifest".to_string(),
            description: "".to_string(),
            file_path_template: "${project}/versions/${version}/manifest.yaml".to_string(),
            properties: vec![
                Property { name: "project".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "dependencies".to_string(), field_type: FieldType::List(Box::new(FieldType::String)), create_only: false },
            ],
        };
        let inst = parse_instance("{}", &td, "ns", &HashMap::new()).unwrap();
        let deps = inst.values.iter().find(|(k, _)| k == "dependencies").unwrap();
        assert_eq!(deps.1, FieldValue::List(vec![]));
    }

    #[test]
    fn test_parse_instance_non_sequence_list_defaults_empty() {
        let td = TypeDef {
            name: "Manifest".to_string(),
            description: "".to_string(),
            file_path_template: "${project}/versions/${version}/manifest.yaml".to_string(),
            properties: vec![
                Property { name: "project".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "version".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "dependencies".to_string(), field_type: FieldType::List(Box::new(FieldType::String)), create_only: false },
            ],
        };
        let inst = parse_instance("dependencies: not-a-list\n", &td, "ns", &HashMap::new()).unwrap();
        let deps = inst.values.iter().find(|(k, _)| k == "dependencies").unwrap();
        assert_eq!(deps.1, FieldValue::List(vec![]));
    }

    #[test]
    fn test_scan_all_nonexistent_dir() {
        let result = scan_all(Path::new("/nonexistent"), &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_scan_all_collections() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
        let collection_dir = version_dir.join("collection");
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
        let instances = scan_all(base, &[td]).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].namespace, "ben/crm.contact");
        assert_eq!(instances[1].namespace, "ben/crm.opportunity");
    }

    #[test]
    fn test_scan_all_nested_fields() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
        let account_dir = version_dir.join("field/account");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(
            account_dir.join("company.yaml"),
            "name: \"company\"\ncollection: \"account\"\n",
        )
        .unwrap();

        let contact_dir = version_dir.join("field/contact");
        std::fs::create_dir_all(&contact_dir).unwrap();
        std::fs::write(
            contact_dir.join("first_name.yaml"),
            "name: \"first_name\"\ncollection: \"contact\"\n",
        )
        .unwrap();

        let td = field_type_def();
        let instances = scan_all(base, &[td]).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].namespace, "ben/crm.account/company");
        assert_eq!(instances[1].namespace, "ben/crm.contact/first_name");
    }

    #[test]
    fn test_scan_all_unversioned_types() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let project_def = TypeDef {
            name: "Project".to_string(),
            description: "A project".to_string(),
            file_path_template: "${namespace}/project.yaml".to_string(),
            properties: vec![
                Property { name: "namespace".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::String, create_only: false },
                Property { name: "description".to_string(), field_type: FieldType::String, create_only: false },
            ],
        };

        // Create: base/ben/crm/project.yaml
        let project_dir = base.join("ben/crm");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.yaml"),
            "name: \"CRM\"\ndescription: \"Customer relationship management\"\n",
        ).unwrap();

        let instances = scan_all(base, &[project_def]).unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].namespace, "ben/crm/project");
        assert_eq!(instances[0].type_name, "Project");
    }

    #[test]
    fn test_scan_all_mixed_types() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let project_def = TypeDef {
            name: "Project".to_string(),
            description: "A project".to_string(),
            file_path_template: "${namespace}/project.yaml".to_string(),
            properties: vec![
                Property { name: "namespace".to_string(), field_type: FieldType::String, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::String, create_only: false },
                Property { name: "description".to_string(), field_type: FieldType::String, create_only: false },
            ],
        };

        let collection_def = collection_type_def();

        // Create project config
        let project_dir = base.join("ben/crm");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.yaml"),
            "name: \"CRM\"\ndescription: \"CRM app\"\n",
        ).unwrap();

        // Create versioned collection
        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
        let collection_dir = version_dir.join("collection");
        std::fs::create_dir_all(&collection_dir).unwrap();
        std::fs::write(
            collection_dir.join("account.yaml"),
            "name: \"account\"\nlabel: \"Account\"\nlabel_plural: \"Accounts\"\n",
        ).unwrap();

        let instances = scan_all(base, &[project_def, collection_def]).unwrap();
        assert_eq!(instances.len(), 2);
        let project_inst = instances.iter().find(|i| i.type_name == "Project").unwrap();
        assert_eq!(project_inst.namespace, "ben/crm/project");
        let collection_inst = instances.iter().find(|i| i.type_name == "Collection").unwrap();
        assert_eq!(collection_inst.namespace, "ben/crm.account");
    }

    #[test]
    fn test_scan_all_ignores_unmatched_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        let extra = base.join("random/place");
        std::fs::create_dir_all(&extra).unwrap();
        std::fs::write(extra.join("unrelated.yaml"), "hello: world\n").unwrap();

        let instances = scan_all(base, &[collection_type_def()]).unwrap();
        assert!(instances.is_empty());
    }

    #[test]
    fn test_extract_template_vars_simple() {
        let vars = extract_template_vars(
            "ben/crm/project.yaml",
            "${namespace}/project.yaml",
        ).unwrap();
        assert_eq!(vars.get("namespace").unwrap(), "ben/crm");
    }

    #[test]
    fn test_extract_template_vars_versioned() {
        let vars = extract_template_vars(
            "ben/crm/versions/0.0.1-dev/collection/account.yaml",
            "${namespace}/versions/${version}/collection/${name}.yaml",
        ).unwrap();
        assert_eq!(vars.get("namespace").unwrap(), "ben/crm");
        assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");
        assert_eq!(vars.get("name").unwrap(), "account");
    }

    #[test]
    fn test_extract_template_vars_nested() {
        let vars = extract_template_vars(
            "ben/crm/versions/0.0.1-dev/field/account/company.yaml",
            "${namespace}/versions/${version}/field/${collection}/${name}.yaml",
        ).unwrap();
        assert_eq!(vars.get("namespace").unwrap(), "ben/crm");
        assert_eq!(vars.get("version").unwrap(), "0.0.1-dev");
        assert_eq!(vars.get("collection").unwrap(), "account");
        assert_eq!(vars.get("name").unwrap(), "company");
    }

    #[test]
    fn test_extract_template_vars_datasets() {
        let vars = extract_template_vars(
            "ben/crm/datasets/acme.yaml",
            "${namespace}/datasets/${dataset_id}.yaml",
        ).unwrap();
        assert_eq!(vars.get("namespace").unwrap(), "ben/crm");
        assert_eq!(vars.get("dataset_id").unwrap(), "acme");
    }

    #[test]
    fn test_extract_template_vars_no_match() {
        assert!(extract_template_vars(
            "ben/crm/other/thing.yaml",
            "${namespace}/project.yaml",
        ).is_none());
    }

    #[test]
    fn test_extract_template_vars_literal_mismatch() {
        assert!(extract_template_vars(
            "ben/crm/sites/acme.yaml",
            "${namespace}/datasets/${dataset_id}.yaml",
        ).is_none());
    }

    #[test]
    fn test_fill_template() {
        let mut vars = HashMap::new();
        vars.insert("namespace".to_string(), "ben/crm".to_string());
        assert_eq!(fill_template("${namespace}/project.yaml", &vars), "ben/crm/project.yaml");

        vars.insert("dataset_id".to_string(), "acme".to_string());
        assert_eq!(
            fill_template("${namespace}/datasets/${dataset_id}.yaml", &vars),
            "ben/crm/datasets/acme.yaml"
        );
    }

    #[test]
    fn test_derive_namespace_versioned() {
        let template = "${namespace}/versions/${version}/collection/${name}.yaml";
        let mut vars = HashMap::new();
        vars.insert("namespace".to_string(), "ben/crm".to_string());
        vars.insert("version".to_string(), "0.0.1-dev".to_string());
        vars.insert("name".to_string(), "account".to_string());
        let rel_path = "ben/crm/versions/0.0.1-dev/collection/account.yaml";
        assert_eq!(derive_namespace(template, &vars, rel_path), "ben/crm.account");
    }

    #[test]
    fn test_derive_namespace_versioned_nested() {
        let template = "${namespace}/versions/${version}/field/${collection}/${name}.yaml";
        let mut vars = HashMap::new();
        vars.insert("namespace".to_string(), "ben/crm".to_string());
        vars.insert("version".to_string(), "0.0.1-dev".to_string());
        vars.insert("collection".to_string(), "account".to_string());
        vars.insert("name".to_string(), "company".to_string());
        let rel_path = "ben/crm/versions/0.0.1-dev/field/account/company.yaml";
        assert_eq!(derive_namespace(template, &vars, rel_path), "ben/crm.account/company");
    }

    #[test]
    fn test_derive_namespace_unversioned() {
        let template = "${namespace}/project.yaml";
        let mut vars = HashMap::new();
        vars.insert("namespace".to_string(), "ben/crm".to_string());
        let rel_path = "ben/crm/project.yaml";
        assert_eq!(derive_namespace(template, &vars, rel_path), "ben/crm/project");
    }
}
