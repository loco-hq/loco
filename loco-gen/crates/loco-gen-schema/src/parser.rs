use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;

use crate::error::Error;
use crate::types::{FieldType, Property, TypeDef};

// ── Raw deserialization structs (serde shapes) ────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypeDefRaw {
    #[serde(default)]
    description: String,
    file_path_template: String,
    properties: IndexMap<String, PropertyRaw>,
}

#[derive(Deserialize)]
struct PropertyRaw {
    #[serde(rename = "type")]
    field_type: String,
    items: Option<String>,
    #[serde(default, rename = "createOnly")]
    create_only: bool,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a YAML schema string into a `TypeDef`.
/// The `type_name` is derived from the filename by the caller.
pub fn parse_schema(yaml: &str, type_name: &str) -> Result<TypeDef, Error> {
    let raw: TypeDefRaw = serde_yaml::from_str(yaml)?;

    let mut properties = Vec::new();
    for (name, prop) in raw.properties {
        let field_type = if prop.field_type == "list" {
            let items_str = prop.items.ok_or(Error::MissingField("items"))?;
            let item_type = FieldType::parse_scalar(&items_str)
                .ok_or_else(|| Error::InvalidFieldType(items_str.clone()))?;
            FieldType::List(Box::new(item_type))
        } else {
            FieldType::parse_scalar(&prop.field_type)
                .ok_or_else(|| Error::InvalidFieldType(prop.field_type.clone()))?
        };
        properties.push(Property { name, field_type, create_only: prop.create_only });
    }

    let type_def = TypeDef {
        name: to_pascal_case(type_name),
        description: raw.description,
        file_path_template: raw.file_path_template,
        properties,
    };

    // Every template variable must be declared as a createOnly: true, type: string property.
    for var in type_def.template_vars() {
        let prop = type_def.properties.iter().find(|p| p.name == var);
        match prop {
            None => return Err(Error::TemplateVarNotDeclared {
                type_name: type_def.name.clone(),
                var,
            }),
            Some(p) if !p.create_only => return Err(Error::TemplateVarNotCreateOnly {
                type_name: type_def.name.clone(),
                var,
            }),
            Some(p) if p.field_type != FieldType::String => return Err(Error::TemplateVarNotString {
                type_name: type_def.name.clone(),
                var,
            }),
            _ => {}
        }
    }

    Ok(type_def)
}

/// Parse a schema from a file path. The type name is derived from the filename.
pub fn parse_schema_file(path: &Path) -> Result<TypeDef, Error> {
    let type_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(Error::MissingField("filename"))?;
    let yaml = std::fs::read_to_string(path)?;
    parse_schema(&yaml, type_name)
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FieldType;

    const SAMPLE_YAML: &str = r#"
version: 1
description: "A named collection of items"
filePathTemplate: "${namespace}/versions/${version}/collection/${name}.yaml"
properties:
  namespace:
    type: string
    createOnly: true
  version:
    type: string
    createOnly: true
  name:
    type: string
    createOnly: true
  label:
    type: string
  item_count:
    type: integer
  average_rating:
    type: float
  is_active:
    type: boolean
"#;

    #[test]
    fn test_parse_schema() {
        let type_def = parse_schema(SAMPLE_YAML, "collection").unwrap();
        assert_eq!(type_def.name, "Collection");
        assert_eq!(type_def.description, "A named collection of items");
        assert_eq!(type_def.properties.len(), 7);
    }

    #[test]
    fn test_field_types() {
        let type_def = parse_schema(SAMPLE_YAML, "collection").unwrap();
        let props = &type_def.properties;

        let find = |name: &str| props.iter().find(|p| p.name == name).unwrap();
        assert_eq!(find("namespace").field_type, FieldType::String);
        assert!(find("namespace").create_only);
        assert_eq!(find("version").field_type, FieldType::String);
        assert!(find("version").create_only);
        assert_eq!(find("name").field_type, FieldType::String);
        assert!(find("name").create_only);
        assert_eq!(find("label").field_type, FieldType::String);
        assert!(!find("label").create_only);
        assert_eq!(find("item_count").field_type, FieldType::Integer);
        assert_eq!(find("average_rating").field_type, FieldType::Float);
        assert_eq!(find("is_active").field_type, FieldType::Boolean);
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(to_pascal_case("collection"), "Collection");
        assert_eq!(to_pascal_case("my_type"), "MyType");
        assert_eq!(to_pascal_case("a_b_c"), "ABC");
    }

    #[test]
    fn test_template_var_must_be_declared() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  name:
    type: string
    createOnly: true
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::TemplateVarNotDeclared { .. }));
        assert!(err.to_string().contains("project"));
    }

    #[test]
    fn test_template_var_must_be_create_only() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  project:
    type: string
  name:
    type: string
    createOnly: true
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::TemplateVarNotCreateOnly { .. }));
        assert!(err.to_string().contains("project"));
    }

    #[test]
    fn test_template_var_must_be_string() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  project:
    type: integer
    createOnly: true
  name:
    type: string
    createOnly: true
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::TemplateVarNotString { .. }));
        assert!(err.to_string().contains("project"));
    }

    #[test]
    fn test_parse_list_field() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/versions/${version}/manifest.yaml"
properties:
  project:
    type: string
    createOnly: true
  version:
    type: string
    createOnly: true
  dependencies:
    type: list
    items: string
"#;
        let type_def = parse_schema(yaml, "manifest").unwrap();
        let prop = type_def.properties.iter().find(|p| p.name == "dependencies").unwrap();
        assert_eq!(prop.name, "dependencies");
        assert_eq!(
            prop.field_type,
            FieldType::List(Box::new(FieldType::String))
        );
    }

    #[test]
    fn test_list_requires_items() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  project:
    type: string
    createOnly: true
  name:
    type: string
    createOnly: true
  tags:
    type: list
"#;
        assert!(parse_schema(yaml, "thing").is_err());
    }

    #[test]
    fn test_list_of_list_rejected() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  project:
    type: string
    createOnly: true
  name:
    type: string
    createOnly: true
  nested:
    type: list
    items: list
"#;
        assert!(parse_schema(yaml, "thing").is_err());
    }

    #[test]
    fn test_invalid_field_type() {
        let yaml = r#"
version: 1
filePathTemplate: "${namespace}/${name}.yaml"
properties:
  namespace:
    type: string
    createOnly: true
  name:
    type: string
    createOnly: true
  bad:
    type: datetime
"#;
        let err = parse_schema(yaml, "test").unwrap_err();
        assert!(err.to_string().contains("datetime"));
    }
}
