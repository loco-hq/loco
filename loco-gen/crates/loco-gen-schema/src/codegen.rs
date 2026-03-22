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
    let mut out = String::new();

    // Struct definition
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub struct {name} {{\n"));
    for prop in &type_def.properties {
        out.push_str(&format!(
            "    {}: {},\n",
            rust_ident(&prop.name),
            prop.field_type.rust_type()
        ));
    }
    out.push_str("}\n\n");

    // Impl block
    out.push_str(&format!("impl {name} {{\n"));

    // Constructor
    generate_new(&mut out, type_def);

    // Accessors
    for prop in &type_def.properties {
        generate_accessor(&mut out, prop);
    }

    // Cache methods
    generate_from_cache(&mut out, type_def);
    generate_to_cache(&mut out, type_def);

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

fn generate_new(out: &mut String, type_def: &TypeDef) {
    let params: Vec<String> = type_def
        .properties
        .iter()
        .map(|p| format!("{}: {}", rust_ident(&p.name), p.field_type.rust_type()))
        .collect();
    out.push_str(&format!(
        "    pub fn new({}) -> Self {{\n",
        params.join(", ")
    ));
    out.push_str(&format!("        {} {{\n", type_def.name));
    for prop in &type_def.properties {
        out.push_str(&format!("            {},\n", rust_ident(&prop.name)));
    }
    out.push_str("        }\n");
    out.push_str("    }\n\n");
}

fn generate_accessor(out: &mut String, prop: &crate::types::Property) {
    let ident = rust_ident(&prop.name);
    let return_type = match prop.field_type {
        FieldType::String => "&str",
        FieldType::Integer => "i64",
        FieldType::Float => "f64",
        FieldType::Boolean => "bool",
    };
    let body = match prop.field_type {
        FieldType::String => format!("&self.{ident}"),
        _ => format!("self.{ident}"),
    };
    out.push_str(&format!(
        "    pub fn {ident}(&self) -> {} {{\n        {}\n    }}\n\n",
        return_type, body
    ));
}

fn generate_from_cache(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    out.push_str(
        "    pub fn from_cache(cache: &loco_gen_runtime::TypedCache, key: &str) -> Option<Self> {\n",
    );
    out.push_str("        let map = cache.get(key)?;\n");

    // Extract each field from the map
    for prop in &type_def.properties {
        let getter = match prop.field_type {
            FieldType::String => "as_string",
            FieldType::Integer => "as_integer",
            FieldType::Float => "as_float",
            FieldType::Boolean => "as_boolean",
        };
        out.push_str(&format!(
            "        let {} = map.get(\"{}\").and_then(|v| v.{getter}())?;\n",
            rust_ident(&prop.name), prop.name
        ));
    }

    out.push_str(&format!("        Some({name} {{\n"));
    for prop in &type_def.properties {
        out.push_str(&format!("            {},\n", rust_ident(&prop.name)));
    }
    out.push_str("        })\n");
    out.push_str("    }\n\n");
}

fn generate_to_cache(out: &mut String, type_def: &TypeDef) {
    out.push_str("    pub fn to_cache(&self, cache: &loco_gen_runtime::TypedCache, key: &str) {\n");
    out.push_str("        let mut map = std::collections::HashMap::new();\n");

    for prop in &type_def.properties {
        let ident = rust_ident(&prop.name);
        let conversion = match prop.field_type {
            FieldType::String => format!("loco_gen_runtime::Value::String(self.{ident}.clone())"),
            FieldType::Integer => format!("loco_gen_runtime::Value::Integer(self.{ident})"),
            FieldType::Float => format!("loco_gen_runtime::Value::Float(self.{ident})"),
            FieldType::Boolean => format!("loco_gen_runtime::Value::Boolean(self.{ident})"),
        };
        out.push_str(&format!(
            "        map.insert(\"{}\".to_string(), {conversion});\n",
            prop.name
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
            file_path_template: None,
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
        assert!(code.contains("pub fn new(name: String, item_count: i64, average_rating: f64, is_active: bool) -> Self"));
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
    fn test_generates_instance_loaders() {
        use crate::types::{FieldValue, Instance};
        let instances = vec![Instance {
            type_name: "Collection".to_string(),
            namespace: "ben/crm.opportunity".to_string(),
            values: vec![
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
