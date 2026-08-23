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
    path_template: String,
    properties: IndexMap<String, PropertyRaw>,
}

#[derive(Deserialize, Clone)]
struct PropertyRaw {
    #[serde(rename = "type")]
    field_type: String,
    #[serde(default)]
    items: Option<ItemsRaw>,
    /// Generated Rust struct name. Required when `type` is `object`.
    name: Option<String>,
    #[serde(default)]
    properties: IndexMap<String, PropertyRaw>,
    #[serde(default, rename = "createOnly")]
    create_only: bool,
    #[serde(default)]
    segments: Option<u32>,
}

/// `items: string` or a nested property spec (`items: { type: object, ... }`).
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum ItemsRaw {
    Scalar(String),
    Nested(Box<PropertyRaw>),
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a YAML schema string into a `TypeDef`.
/// The `type_name` is derived from the filename by the caller.
pub fn parse_schema(yaml: &str, type_name: &str) -> Result<TypeDef, Error> {
    let raw: TypeDefRaw = serde_yaml::from_str(yaml)?;
    let type_name_pascal = to_pascal_case(type_name);

    let mut properties = Vec::new();
    for (name, prop) in raw.properties {
        properties.push(parse_property(name, prop, &type_name_pascal)?);
    }

    let type_def = TypeDef {
        name: type_name_pascal,
        description: raw.description,
        path_template: raw.path_template,
        properties,
    };

    // Every template variable must be declared as a createOnly: true, type: string property.
    for var in type_def.template_vars() {
        let prop = type_def.properties.iter().find(|p| p.name == var);
        match prop {
            None => {
                return Err(Error::TemplateVarNotDeclared {
                    type_name: type_def.name.clone(),
                    var,
                })
            }
            Some(p) if !p.create_only => {
                return Err(Error::TemplateVarNotCreateOnly {
                    type_name: type_def.name.clone(),
                    var,
                })
            }
            Some(p) if !matches!(p.field_type, FieldType::Slug { .. }) => {
                return Err(Error::TemplateVarNotSlug {
                    type_name: type_def.name.clone(),
                    var,
                })
            }
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

fn parse_property(name: String, raw: PropertyRaw, parent_type: &str) -> Result<Property, Error> {
    let create_only = raw.create_only;
    let field_type = parse_field_type(raw, parent_type, &name)?;
    Ok(Property {
        name,
        field_type,
        create_only,
    })
}

fn parse_field_type(
    raw: PropertyRaw,
    parent_type: &str,
    field_name: &str,
) -> Result<FieldType, Error> {
    match raw.field_type.as_str() {
        "list" => {
            let items = raw.items.ok_or(Error::MissingField("items"))?;
            let inner = parse_items(items, parent_type, field_name)?;
            if matches!(inner, FieldType::List(_)) {
                return Err(Error::InvalidFieldType("list".into()));
            }
            Ok(FieldType::List(Box::new(inner)))
        }
        "slug" => Ok(FieldType::Slug {
            segments: raw.segments.unwrap_or(1),
        }),
        "object" => parse_object(raw, parent_type, field_name),
        other => {
            FieldType::parse_scalar(other).ok_or_else(|| Error::InvalidFieldType(other.to_string()))
        }
    }
}

fn parse_items(items: ItemsRaw, parent_type: &str, field_name: &str) -> Result<FieldType, Error> {
    match items {
        ItemsRaw::Scalar(s) => FieldType::parse_scalar(&s).ok_or(Error::InvalidFieldType(s)),
        ItemsRaw::Nested(raw) => parse_field_type(*raw, parent_type, field_name),
    }
}

fn parse_object(raw: PropertyRaw, parent_type: &str, field_name: &str) -> Result<FieldType, Error> {
    let name = match raw.name {
        Some(n) if !n.is_empty() => to_pascal_case(&n),
        _ => {
            return Err(Error::ObjectMissingName {
                parent: parent_type.to_string(),
                field: field_name.to_string(),
            })
        }
    };
    let mut properties = Vec::new();
    for (pname, prop) in raw.properties {
        properties.push(parse_property(pname, prop, &name)?);
    }
    Ok(FieldType::Object { name, properties })
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
pathTemplate: "${namespace}/versions/${version}/collection/${name}"
properties:
  namespace:
    type: slug
    segments: 2
    createOnly: true
  version:
    type: slug
    createOnly: true
  name:
    type: slug
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
        assert_eq!(
            find("namespace").field_type,
            FieldType::Slug { segments: 2 }
        );
        assert!(find("namespace").create_only);
        assert_eq!(find("version").field_type, FieldType::Slug { segments: 1 });
        assert!(find("version").create_only);
        assert_eq!(find("name").field_type, FieldType::Slug { segments: 1 });
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
        assert_eq!(to_pascal_case("collection_grant"), "CollectionGrant");
        assert_eq!(to_pascal_case("a_b_c"), "ABC");
    }

    #[test]
    fn test_template_var_must_be_declared() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/${name}"
properties:
  name:
    type: slug
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
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
  name:
    type: slug
    createOnly: true
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::TemplateVarNotCreateOnly { .. }));
        assert!(err.to_string().contains("project"));
    }

    #[test]
    fn test_template_var_must_be_slug() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/${name}"
properties:
  project:
    type: integer
    createOnly: true
  name:
    type: slug
    createOnly: true
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::TemplateVarNotSlug { .. }));
        assert!(err.to_string().contains("project"));
    }

    #[test]
    fn test_parse_slug_field() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
    createOnly: true
"#;
        let type_def = parse_schema(yaml, "thing").unwrap();
        let find = |n: &str| type_def.properties.iter().find(|p| p.name == n).unwrap();
        assert_eq!(find("project").field_type, FieldType::Slug { segments: 2 });
        assert_eq!(find("name").field_type, FieldType::Slug { segments: 1 });
    }

    #[test]
    fn test_parse_list_field() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/versions/${version}/manifest"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  version:
    type: slug
    createOnly: true
  dependencies:
    type: list
    items: string
"#;
        let type_def = parse_schema(yaml, "manifest").unwrap();
        let prop = type_def
            .properties
            .iter()
            .find(|p| p.name == "dependencies")
            .unwrap();
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
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
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
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
    createOnly: true
  nested:
    type: list
    items: list
"#;
        assert!(parse_schema(yaml, "thing").is_err());
    }

    #[test]
    fn test_nested_list_via_items_object_rejected() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
    createOnly: true
  nested:
    type: list
    items:
      type: list
      items: string
"#;
        assert!(parse_schema(yaml, "thing").is_err());
    }

    #[test]
    fn test_parse_list_of_objects() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/versions/${version}/permission_sets/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  version:
    type: slug
    createOnly: true
  name:
    type: slug
    createOnly: true
  collections:
    type: list
    items:
      type: object
      name: collection_grant
      properties:
        collection:
          type: string
        read:
          type: boolean
        create:
          type: boolean
"#;
        let type_def = parse_schema(yaml, "permission_set").unwrap();
        let prop = type_def
            .properties
            .iter()
            .find(|p| p.name == "collections")
            .unwrap();
        match &prop.field_type {
            FieldType::List(inner) => match inner.as_ref() {
                FieldType::Object { name, properties } => {
                    assert_eq!(name, "CollectionGrant");
                    assert_eq!(properties.len(), 3);
                    assert_eq!(properties[0].name, "collection");
                    assert_eq!(properties[0].field_type, FieldType::String);
                    assert_eq!(properties[1].field_type, FieldType::Boolean);
                    assert_eq!(properties[2].field_type, FieldType::Boolean);
                }
                other => panic!("expected object item, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn test_object_requires_name() {
        let yaml = r#"
version: 1
pathTemplate: "${project}/${name}"
properties:
  project:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
    createOnly: true
  grants:
    type: list
    items:
      type: object
      properties:
        collection:
          type: string
"#;
        let err = parse_schema(yaml, "thing").unwrap_err();
        assert!(matches!(err, Error::ObjectMissingName { .. }));
        assert!(err.to_string().contains("grants"));
    }

    #[test]
    fn test_invalid_field_type() {
        let yaml = r#"
version: 1
pathTemplate: "${namespace}/${name}"
properties:
  namespace:
    type: slug
    segments: 2
    createOnly: true
  name:
    type: slug
    createOnly: true
  bad:
    type: datetime
"#;
        let err = parse_schema(yaml, "test").unwrap_err();
        assert!(err.to_string().contains("datetime"));
    }
}
