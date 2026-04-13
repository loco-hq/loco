use crate::types::{FieldType, FieldValue, Instance, TypeDef};

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

fn rust_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// Generate Rust source code for a single TypeDef with its instances.
pub fn generate(type_def: &TypeDef, instances: &[Instance]) -> String {
    let name = &type_def.name;
    let fields = type_def.all_fields();
    let mut out = String::new();

    // Struct definition
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub struct {name} {{\n"));
    for (field_name, field_type) in &fields {
        out.push_str(&format!(
            "    {}: {},\n",
            rust_ident(field_name),
            field_type.rust_type()
        ));
    }
    out.push_str("}\n\n");

    // Impl block
    out.push_str(&format!("impl {name} {{\n"));

    // Constructor
    generate_new(&mut out, type_def, &fields);

    // Accessors
    for (field_name, field_type) in &fields {
        generate_accessor(&mut out, field_name, field_type);
    }

    // Cache methods
    generate_from_cache(&mut out, type_def, &fields);
    generate_to_cache(&mut out, type_def, &fields);

    // Instance loaders
    let type_instances: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.type_name == type_def.name)
        .collect();
    generate_load_instance(&mut out, type_def, &type_instances);
    generate_load_all_instances(&mut out, type_def, &type_instances);

    out.push_str("}\n");
    out
}

fn generate_new(out: &mut String, type_def: &TypeDef, fields: &[(String, FieldType)]) {
    let params: Vec<String> = fields
        .iter()
        .map(|(name, ft)| format!("{}: {}", rust_ident(name), ft.rust_type()))
        .collect();
    out.push_str(&format!(
        "    pub fn new({}) -> Self {{\n",
        params.join(", ")
    ));
    out.push_str(&format!("        {} {{\n", type_def.name));
    for (name, _) in fields {
        out.push_str(&format!("            {},\n", rust_ident(name)));
    }
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

fn generate_accessor(out: &mut String, field_name: &str, field_type: &FieldType) {
    let ident = rust_ident(field_name);
    let (return_type, body) = match field_type {
        FieldType::String => ("&str".to_string(), format!("&self.{ident}")),
        FieldType::Integer => ("i64".to_string(), format!("self.{ident}")),
        FieldType::Float => ("f64".to_string(), format!("self.{ident}")),
        FieldType::Boolean => ("bool".to_string(), format!("self.{ident}")),
        FieldType::List(inner) => (
            format!("&[{}]", inner.rust_type()),
            format!("&self.{ident}"),
        ),
    };
    out.push_str(&format!(
        "    pub fn {ident}(&self) -> {} {{\n        {}\n    }}\n\n",
        return_type, body
    ));
}

fn generate_from_cache(out: &mut String, type_def: &TypeDef, fields: &[(String, FieldType)]) {
    let name = &type_def.name;
    out.push_str(
        "    pub fn from_cache(cache: &loco_gen_runtime::TypedCache, key: &str) -> Option<Self> {\n",
    );
    out.push_str("        let map = cache.get(key)?;\n");

    // Extract each field from the map
    for (field_name, field_type) in fields {
        let ident = rust_ident(field_name);
        match field_type {
            FieldType::String => {
                out.push_str(&format!(
                    "        let {ident} = map.get(\"{field_name}\").and_then(|v| v.as_string())?.to_owned();\n"
                ));
            }
            FieldType::Integer => {
                out.push_str(&format!(
                    "        let {ident} = map.get(\"{field_name}\").and_then(|v| v.as_integer())?;\n"
                ));
            }
            FieldType::Float => {
                out.push_str(&format!(
                    "        let {ident} = map.get(\"{field_name}\").and_then(|v| v.as_float())?;\n"
                ));
            }
            FieldType::Boolean => {
                out.push_str(&format!(
                    "        let {ident} = map.get(\"{field_name}\").and_then(|v| v.as_boolean())?;\n"
                ));
            }
            FieldType::List(inner) => {
                let extractor = match inner.as_ref() {
                    // as_string() returns &str; map to owned String for collection
                    FieldType::String => "as_string().map(str::to_owned)",
                    FieldType::Integer => "as_integer()",
                    FieldType::Float => "as_float()",
                    FieldType::Boolean => "as_boolean()",
                    FieldType::List(_) => unreachable!("nested lists rejected at parse time"),
                };
                out.push_str(&format!(
                    "        let {ident}: {rust_ty} = map.get(\"{field_name}\")\n            .and_then(|v| v.as_list())?\n            .iter()\n            .filter_map(|v| v.{extractor})\n            .collect();\n",
                    rust_ty = field_type.rust_type()
                ));
            }
        }
    }

    out.push_str(&format!("        Some({name} {{\n"));
    for (field_name, _) in fields {
        out.push_str(&format!("            {},\n", rust_ident(field_name)));
    }
    out.push_str("        })\n");
    out.push_str("    }\n\n");
}

fn generate_to_cache(out: &mut String, _type_def: &TypeDef, fields: &[(String, FieldType)]) {
    out.push_str("    pub fn to_cache(&self, cache: &loco_gen_runtime::TypedCache, key: &str) {\n");
    out.push_str("        let mut map = std::collections::HashMap::new();\n");

    for (field_name, field_type) in fields {
        let ident = rust_ident(field_name);
        let conversion = match field_type {
            FieldType::String => format!("loco_gen_runtime::Value::String(self.{ident}.clone())"),
            FieldType::Integer => format!("loco_gen_runtime::Value::Integer(self.{ident})"),
            FieldType::Float => format!("loco_gen_runtime::Value::Float(self.{ident})"),
            FieldType::Boolean => format!("loco_gen_runtime::Value::Boolean(self.{ident})"),
            FieldType::List(inner) => {
                let wrap = match inner.as_ref() {
                    FieldType::String => "loco_gen_runtime::Value::String(v.clone())",
                    FieldType::Integer => "loco_gen_runtime::Value::Integer(*v)",
                    FieldType::Float => "loco_gen_runtime::Value::Float(*v)",
                    FieldType::Boolean => "loco_gen_runtime::Value::Boolean(*v)",
                    FieldType::List(_) => unreachable!("nested lists rejected at parse time"),
                };
                format!(
                    "loco_gen_runtime::Value::List(self.{ident}.iter().map(|v| {wrap}).collect())"
                )
            }
        };
        out.push_str(&format!(
            "        map.insert(\"{field_name}\".to_string(), {conversion});\n"
        ));
    }

    out.push_str("        cache.set(key, map);\n");
    out.push_str("    }\n");
}

fn generate_field_value_literal(value: &FieldValue) -> String {
    match value {
        FieldValue::String(s) => format!("\"{}\".to_string()", s.replace('\\', "\\\\").replace('"', "\\\"")),
        FieldValue::Integer(i) => format!("{i}_i64"),
        FieldValue::Float(f) => format!("{f}_f64"),
        FieldValue::Boolean(b) => format!("{b}"),
        FieldValue::List(items) => {
            let parts: Vec<String> = items.iter().map(generate_field_value_literal).collect();
            format!("vec![{}]", parts.join(", "))
        }
    }
}

fn generate_load_instance(out: &mut String, type_def: &TypeDef, instances: &[&Instance]) {
    let name = &type_def.name;
    out.push_str("    pub fn load_instance(namespace: &str) -> Option<Self> {\n");
    out.push_str("        match namespace {\n");
    for inst in instances {
        out.push_str(&format!("            \"{}\" => Some({name} {{\n", inst.namespace));
        for (field_name, field_value) in &inst.values {
            out.push_str(&format!(
                "                {}: {},\n",
                rust_ident(field_name),
                generate_field_value_literal(field_value)
            ));
        }
        out.push_str("            }),\n");
    }
    out.push_str("            _ => None,\n");
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

fn generate_load_all_instances(out: &mut String, type_def: &TypeDef, instances: &[&Instance]) {
    let name = &type_def.name;
    out.push_str("    pub fn load_all_instances() -> Vec<(&'static str, Self)> {\n");
    out.push_str("        vec![\n");
    for inst in instances {
        out.push_str(&format!("            (\"{}\", {name} {{\n", inst.namespace));
        for (field_name, field_value) in &inst.values {
            out.push_str(&format!(
                "                {}: {},\n",
                rust_ident(field_name),
                generate_field_value_literal(field_value)
            ));
        }
        out.push_str("            }),\n");
    }
    out.push_str("        ]\n");
    out.push_str("    }\n");
}

/// Generate a single file containing all type definitions.
pub fn generate_all(type_defs: &[TypeDef], instances: &[Instance]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by loco-gen-codegen-build. Do not edit.\n\n");
    for type_def in type_defs {
        out.push_str(&generate(type_def, instances));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldType, Property, TypeDef};

    fn sample_type_def() -> TypeDef {
        TypeDef {
            name: "Collection".to_string(),
            description: "A named collection".to_string(),
            file_path_template: "${namespace}/versions/${version}/collection/${name}.yaml".to_string(),
            properties: vec![
                Property {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                },
                Property {
                    name: "item_count".to_string(),
                    field_type: FieldType::Integer,
                },
                Property {
                    name: "average_rating".to_string(),
                    field_type: FieldType::Float,
                },
                Property {
                    name: "is_active".to_string(),
                    field_type: FieldType::Boolean,
                },
            ],
        }
    }

    #[test]
    fn test_generates_struct() {
        let code = generate(&sample_type_def(), &[]);
        assert!(code.contains("pub struct Collection {"));
        assert!(code.contains("name: String,"));
        assert!(code.contains("item_count: i64,"));
        assert!(code.contains("average_rating: f64,"));
        assert!(code.contains("is_active: bool,"));
    }

    #[test]
    fn test_generates_constructor() {
        let code = generate(&sample_type_def(), &[]);
        // Template vars `namespace` and `version` are prepended as implicit String fields;
        // `name` is shadowed by the declared `name` property.
        assert!(code.contains("pub fn new(namespace: String, version: String, name: String, item_count: i64, average_rating: f64, is_active: bool) -> Self"));
    }

    #[test]
    fn test_template_vars_become_implicit_fields() {
        let code = generate(&sample_type_def(), &[]);
        // Implicit fields from `${namespace}` and `${version}` in the template.
        assert!(code.contains("namespace: String,"));
        assert!(code.contains("version: String,"));
        assert!(code.contains("pub fn namespace(&self) -> &str"));
        assert!(code.contains("pub fn version(&self) -> &str"));
    }

    #[test]
    fn test_generates_accessors() {
        let code = generate(&sample_type_def(), &[]);
        assert!(code.contains("pub fn name(&self) -> &str"));
        assert!(code.contains("pub fn item_count(&self) -> i64"));
        assert!(code.contains("pub fn average_rating(&self) -> f64"));
        assert!(code.contains("pub fn is_active(&self) -> bool"));
    }

    #[test]
    fn test_generates_cache_methods() {
        let code = generate(&sample_type_def(), &[]);
        assert!(code.contains(
            "pub fn from_cache(cache: &loco_gen_runtime::TypedCache, key: &str) -> Option<Self>"
        ));
        assert!(
            code.contains("pub fn to_cache(&self, cache: &loco_gen_runtime::TypedCache, key: &str)")
        );
    }

    #[test]
    fn test_list_field_generation() {
        use crate::types::{FieldValue, Instance};
        let td = TypeDef {
            name: "Manifest".to_string(),
            description: "".to_string(),
            file_path_template: "${project}/versions/${version}/manifest.yaml".to_string(),
            properties: vec![Property {
                name: "dependencies".to_string(),
                field_type: FieldType::List(Box::new(FieldType::String)),
            }],
        };
        let instances = vec![Instance {
            type_name: "Manifest".to_string(),
            namespace: "ben/crm/versions/0.0.1-dev/manifest".to_string(),
            values: vec![
                ("project".to_string(), FieldValue::String("ben/crm".to_string())),
                ("version".to_string(), FieldValue::String("0.0.1-dev".to_string())),
                (
                    "dependencies".to_string(),
                    FieldValue::List(vec![FieldValue::String("loco/core@0.0.1-dev".to_string())]),
                ),
            ],
        }];
        let code = generate(&td, &instances);
        assert!(code.contains("dependencies: Vec<String>,"));
        assert!(code.contains("pub fn dependencies(&self) -> &[String]"));
        assert!(code.contains("vec![\"loco/core@0.0.1-dev\".to_string()]"));
    }

    #[test]
    fn test_generates_instance_loaders() {
        use crate::types::{FieldValue, Instance};
        let instances = vec![Instance {
            type_name: "Collection".to_string(),
            namespace: "ben/crm.opportunity".to_string(),
            values: vec![
                ("namespace".to_string(), FieldValue::String("ben/crm".to_string())),
                ("version".to_string(), FieldValue::String("0.0.1-dev".to_string())),
                ("name".to_string(), FieldValue::String("opportunity".to_string())),
                ("item_count".to_string(), FieldValue::Integer(10)),
                ("average_rating".to_string(), FieldValue::Float(4.5)),
                ("is_active".to_string(), FieldValue::Boolean(true)),
            ],
        }];
        let code = generate(&sample_type_def(), &instances);
        assert!(code.contains("pub fn load_instance(namespace: &str) -> Option<Self>"));
        assert!(code.contains("\"ben/crm.opportunity\""));
        assert!(code.contains("pub fn load_all_instances() -> Vec<(&'static str, Self)>"));
    }
}
