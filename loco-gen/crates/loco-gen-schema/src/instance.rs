use std::collections::{HashMap, HashSet};
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
#[derive(Debug, Clone)]
pub struct ScannedNamespace {
    pub user: String,
    pub project: String,
    pub version: String,
    pub config: NamespaceConfig,
}

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

    let declared: HashSet<&str> =
        type_def.properties.iter().map(|p| p.name.as_str()).collect();

    let mut values = Vec::new();
    for var_name in type_def.template_vars() {
        if declared.contains(var_name.as_str()) {
            continue;
        }
        let val = vars.get(&var_name).cloned().unwrap_or_default();
        values.push((var_name, FieldValue::String(val)));
    }

    for prop in &type_def.properties {
        let key = serde_yaml::Value::String(prop.name.clone());
        // Loose on load: missing or mistyped fields fall back to type defaults.
        let field_value = match mapping.get(&key) {
            Some(val) => match prop.field_type {
                FieldType::String => FieldValue::String(
                    val.as_str().map(|s| s.to_string()).unwrap_or_default(),
                ),
                FieldType::Integer => FieldValue::Integer(val.as_i64().unwrap_or(0)),
                FieldType::Float => FieldValue::Float(val.as_f64().unwrap_or(0.0)),
                FieldType::Boolean => FieldValue::Boolean(val.as_bool().unwrap_or(false)),
            },
            None => match prop.field_type {
                FieldType::String => FieldValue::String(String::new()),
                FieldType::Integer => FieldValue::Integer(0),
                FieldType::Float => FieldValue::Float(0.0),
                FieldType::Boolean => FieldValue::Boolean(false),
            },
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

/// Walk `instances_dir` recursively to find all YAML files and match them against
/// type definitions using their `filePathTemplate`. Also discovers `loco.yaml`
/// namespace config files.
///
/// Derives instance namespace from extracted template variables.
/// Auto-includes `loco/core` if not explicitly listed as a dependency.
/// Validates that all declared dependencies exist on disk.
pub fn scan_all(
    instances_dir: &Path,
    type_defs: &[TypeDef],
) -> Result<ScanResult, Error> {
    let mut instances = Vec::new();
    let mut namespaces = Vec::new();

    if !instances_dir.exists() {
        return Ok(ScanResult { instances, namespaces });
    }

    let all_files = collect_yaml_files(instances_dir)?;

    for file_path in &all_files {
        let rel = file_path
            .strip_prefix(instances_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        // Detect loco.yaml namespace configs: {project}/versions/{version}/loco.yaml
        if rel.ends_with("/loco.yaml") || rel == "loco.yaml" {
            if let Some(vars) = extract_template_vars(
                &rel,
                "${project}/versions/${version}/loco.yaml",
            ) {
                let project_path = vars.get("project").cloned().unwrap_or_default();
                let version = vars.get("version").cloned().unwrap_or_default();
                let (user, project) = project_path.split_once('/').unwrap_or(("", &project_path));

                let yaml = std::fs::read_to_string(file_path)?;
                let config = namespace::parse_namespace_config(&yaml)?;

                namespaces.push(ScannedNamespace {
                    user: user.to_string(),
                    project: project.to_string(),
                    version,
                    config,
                });
            }
            continue;
        }

        // Try matching against each type's filePathTemplate
        for type_def in type_defs {
            if let Some(vars) = extract_template_vars(&rel, &type_def.file_path_template) {
                let namespace_str = derive_namespace(&type_def.file_path_template, &vars, &rel);
                let yaml = std::fs::read_to_string(file_path)?;
                let instance = parse_instance(&yaml, type_def, &namespace_str, &vars)?;
                instances.push(instance);
                break; // matched — don't try other templates
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

    fn field_type_def() -> TypeDef {
        TypeDef {
            name: "Field".to_string(),
            description: "A field belonging to a collection".to_string(),
            file_path_template: "${namespace}/versions/${version}/field/${collection}/${name}.yaml".to_string(),
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

    #[test]
    fn test_parse_instance() {
        let yaml = r#"
name: "opportunity"
label: "Opportunity"
label_plural: "Opportunities"
"#;
        let td = collection_type_def();
        let inst = parse_instance(yaml, &td, "ben/crm.opportunity", &HashMap::new()).unwrap();
        assert_eq!(inst.type_name, "Collection");
        assert_eq!(inst.namespace, "ben/crm.opportunity");
        // 3 declared properties + 2 non-shadowed template vars (namespace, version);
        // `name` is shadowed by the declared `name` property.
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
    fn test_scan_all_nonexistent_dir() {
        let result = scan_all(Path::new("/nonexistent"), &[]);
        assert!(result.is_ok());
        let scan = result.unwrap();
        assert!(scan.instances.is_empty());
        assert!(scan.namespaces.is_empty());
    }

    #[test]
    fn test_scan_all_collections() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create: base/ben/crm/versions/0.0.1-dev/collection/opportunity.yaml
        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
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
        let scan = scan_all(base, &[td]).unwrap();
        assert_eq!(scan.instances.len(), 2);
        // Sorted alphabetically
        assert_eq!(scan.instances[0].namespace, "ben/crm.contact");
        assert_eq!(scan.instances[1].namespace, "ben/crm.opportunity");
        assert_eq!(scan.namespaces.len(), 1);
        assert_eq!(scan.namespaces[0].config.name, "crm");
    }

    #[test]
    fn test_scan_all_nested_fields() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create nested: base/ben/crm/versions/0.0.1-dev/field/account/company.yaml
        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
        let account_dir = version_dir.join("field/account");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(version_dir.join("loco.yaml"), "name: crm\n").unwrap();
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
        let scan = scan_all(base, &[td]).unwrap();
        assert_eq!(scan.instances.len(), 2);
        assert_eq!(scan.instances[0].namespace, "ben/crm.account/company");
        assert_eq!(scan.instances[1].namespace, "ben/crm.contact/first_name");
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
                Property { name: "name".to_string(), field_type: FieldType::String },
                Property { name: "namespace".to_string(), field_type: FieldType::String },
                Property { name: "description".to_string(), field_type: FieldType::String },
            ],
        };

        // Create: base/ben/crm/project.yaml
        let project_dir = base.join("ben/crm");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.yaml"),
            "name: \"CRM\"\nnamespace: \"ben/crm\"\ndescription: \"Customer relationship management\"\n",
        ).unwrap();

        let scan = scan_all(base, &[project_def]).unwrap();
        assert_eq!(scan.instances.len(), 1);
        assert_eq!(scan.instances[0].namespace, "ben/crm/project");
        assert_eq!(scan.instances[0].type_name, "Project");
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
                Property { name: "name".to_string(), field_type: FieldType::String },
                Property { name: "namespace".to_string(), field_type: FieldType::String },
                Property { name: "description".to_string(), field_type: FieldType::String },
            ],
        };

        let collection_def = collection_type_def();

        // Create project config
        let project_dir = base.join("ben/crm");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("project.yaml"),
            "name: \"CRM\"\nnamespace: \"ben/crm\"\ndescription: \"CRM app\"\n",
        ).unwrap();

        // Create versioned collection
        let version_dir = base.join("ben/crm/versions/0.0.1-dev");
        let collection_dir = version_dir.join("collection");
        std::fs::create_dir_all(&collection_dir).unwrap();
        std::fs::write(version_dir.join("loco.yaml"), "name: crm\n").unwrap();
        std::fs::write(
            collection_dir.join("account.yaml"),
            "name: \"account\"\nlabel: \"Account\"\nlabel_plural: \"Accounts\"\n",
        ).unwrap();

        let scan = scan_all(base, &[project_def, collection_def]).unwrap();
        assert_eq!(scan.instances.len(), 2);
        // Check both types are present
        let project_inst = scan.instances.iter().find(|i| i.type_name == "Project").unwrap();
        assert_eq!(project_inst.namespace, "ben/crm/project");
        let collection_inst = scan.instances.iter().find(|i| i.type_name == "Collection").unwrap();
        assert_eq!(collection_inst.namespace, "ben/crm.account");
    }

    #[test]
    fn test_auto_include_loco_core() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create loco/core
        let core_dir = base.join("loco/core/versions/0.0.1-dev");
        std::fs::create_dir_all(&core_dir).unwrap();
        std::fs::write(core_dir.join("loco.yaml"), "name: core\n").unwrap();

        // Create ben/crm with no explicit loco/core dependency
        let crm_dir = base.join("ben/crm/versions/0.0.1-dev");
        std::fs::create_dir_all(&crm_dir).unwrap();
        std::fs::write(crm_dir.join("loco.yaml"), "name: crm\n").unwrap();

        let scan = scan_all(base, &[]).unwrap();
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

        let crm_dir = base.join("ben/crm/versions/0.0.1-dev");
        std::fs::create_dir_all(&crm_dir).unwrap();
        std::fs::write(
            crm_dir.join("loco.yaml"),
            "name: crm\ndependencies:\n  - alice/billing@0.1.0\n",
        )
        .unwrap();

        let result = scan_all(base, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("alice/billing@0.1.0"));
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
