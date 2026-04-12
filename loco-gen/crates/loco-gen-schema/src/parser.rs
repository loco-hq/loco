use std::path::Path;

use crate::error::Error;
use crate::types::{FieldType, Property, Schema, TypeDef};

/// Parse a YAML schema file into a `Schema`.
/// The `type_name` is derived from the filename by the caller.
/// Uses `serde_yaml::Value` to preserve the insertion order of properties.
pub fn parse_schema(yaml: &str, type_name: &str) -> Result<Schema, Error> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let mapping = value
        .as_mapping()
        .ok_or(Error::MissingField("root mapping"))?;

    let version = mapping
        .get(serde_yaml::Value::String("version".into()))
        .and_then(|v| v.as_u64())
        .ok_or(Error::MissingField("version"))? as u32;

    let description = mapping
        .get(serde_yaml::Value::String("description".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let props_mapping = mapping
        .get(serde_yaml::Value::String("properties".into()))
        .and_then(|v| v.as_mapping())
        .ok_or(Error::MissingField("properties"))?;

    let file_path_template = mapping
        .get(serde_yaml::Value::String("filePathTemplate".into()))
        .and_then(|v| v.as_str())
        .ok_or(Error::MissingField("filePathTemplate"))?
        .to_string();

    let mut properties = Vec::new();
    for (key, val) in props_mapping {
        let name = key
            .as_str()
            .ok_or(Error::MissingField("property name"))?
            .to_string();
        let type_str = val
            .as_mapping()
            .and_then(|m| m.get(serde_yaml::Value::String("type".into())))
            .and_then(|v| v.as_str())
            .ok_or(Error::MissingField("type"))?;
        let field_type = FieldType::parse(type_str)
            .ok_or_else(|| Error::InvalidFieldType(type_str.to_string()))?;
        properties.push(Property { name, field_type });
    }

    let type_def = TypeDef {
        name: to_pascal_case(type_name),
        description,
        file_path_template,
        properties,
    };

    // Template variables become implicit fields — a declared property with the
    // same name would shadow (or conflict with) them, so reject at parse time.
    let template_vars = type_def.template_vars();
    for prop in &type_def.properties {
        if template_vars.iter().any(|v| v == &prop.name) {
            return Err(Error::PropertyTemplateCollision {
                type_name: type_def.name.clone(),
                property: prop.name.clone(),
            });
        }
    }

    Ok(Schema { version, type_def })
}

/// Parse a schema from a file path. The type name is derived from the filename.
pub fn parse_schema_file(path: &Path) -> Result<Schema, Error> {
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
        let schema = parse_schema(SAMPLE_YAML, "collection").unwrap();
        assert_eq!(schema.version, 1);
        assert_eq!(schema.type_def.name, "Collection");
        assert_eq!(schema.type_def.description, "A named collection of items");
        assert_eq!(schema.type_def.properties.len(), 4);
    }

    #[test]
    fn test_field_types() {
        let schema = parse_schema(SAMPLE_YAML, "collection").unwrap();
        let props = &schema.type_def.properties;

        let find = |name: &str| props.iter().find(|p| p.name == name).unwrap();
        assert_eq!(find("label").field_type, FieldType::String);
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
    fn test_property_template_collision() {
        let yaml = r#"
version: 1
filePathTemplate: "${project}/${name}.yaml"
properties:
  name:
    type: string
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::PropertyTemplateCollision { .. }));
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn test_invalid_field_type() {
        let yaml = r#"
version: 1
filePathTemplate: "${namespace}/${name}.yaml"
properties:
  bad:
    type: datetime
"#;
        let err = parse_schema(yaml, "test").unwrap_err();
        assert!(err.to_string().contains("datetime"));
    }
}
