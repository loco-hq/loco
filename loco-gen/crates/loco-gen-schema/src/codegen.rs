use crate::types::{FieldType, Property, TypeDef};

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

fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Generate Rust source code for a single TypeDef.
pub fn generate(type_def: &TypeDef) -> String {
    let name = &type_def.name;
    let fields = type_def.all_fields();
    let mut out = String::new();

    out.push_str("#[derive(Debug, Clone, PartialEq, serde::Serialize)]\n");
    out.push_str(&format!("pub struct {name} {{\n"));
    for (field_name, field_type) in &fields {
        out.push_str(&format!(
            "    {}: {},\n",
            rust_ident(field_name),
            field_type.rust_type()
        ));
    }
    out.push_str("}\n\n");

    out.push_str(&format!("impl {name} {{\n"));
    generate_new(&mut out, type_def, &fields);
    for (field_name, field_type) in &fields {
        generate_accessor(&mut out, field_name, field_type);
    }
    generate_from_map(&mut out, type_def, &fields);
    generate_to_path(&mut out, type_def);
    generate_from_path(&mut out, type_def);
    out.push_str("}\n\n");

    generate_store_methods(&mut out, type_def);

    out
}

/// Template tokens: either a literal path segment or a `${var}` with a known slug segment count.
enum TplToken<'a> {
    Lit(&'a str),
    Var { name: &'a str, segments: u32 },
}

fn tokenize_template<'a>(type_def: &'a TypeDef) -> Vec<TplToken<'a>> {
    type_def
        .path_template
        .split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
                let segments = type_def
                    .properties
                    .iter()
                    .find_map(|p| match (&p.name, &p.field_type) {
                        (n, FieldType::Slug { segments }) if n == name => Some(*segments),
                        _ => None,
                    })
                    .expect("template var must be a declared slug property");
                TplToken::Var { name, segments }
            } else {
                TplToken::Lit(seg)
            }
        })
        .collect()
}

fn generate_to_path(out: &mut String, type_def: &TypeDef) {
    let tokens = tokenize_template(type_def);
    let vars = type_def.template_vars();
    let var_index: std::collections::HashMap<&str, usize> = vars
        .iter()
        .enumerate()
        .map(|(i, v)| (v.as_str(), i))
        .collect();

    let params: Vec<String> = vars
        .iter()
        .map(|v| format!("{}: &str", rust_ident(v)))
        .collect();
    out.push_str(&format!(
        "    pub fn to_path({}) -> String {{\n",
        params.join(", ")
    ));

    let fmt_parts: Vec<String> = tokens
        .iter()
        .map(|t| match t {
            TplToken::Var { name, .. } => format!("{{{}}}", var_index[*name]),
            TplToken::Lit(s) => (*s).to_string(),
        })
        .collect();
    let fmt_str = fmt_parts.join("/");
    let args: Vec<String> = vars.iter().map(|v| rust_ident(v)).collect();
    out.push_str(&format!(
        "        format!(\"{fmt_str}\", {})\n",
        args.join(", ")
    ));
    out.push_str("    }\n\n");
}

fn generate_from_path(out: &mut String, type_def: &TypeDef) {
    let tokens = tokenize_template(type_def);
    out.push_str(
        "    pub fn from_path(path: &str) -> Option<std::collections::HashMap<String, String>> {\n",
    );
    out.push_str("        let segs: Vec<&str> = path.split('/').collect();\n");
    out.push_str("        let mut i = 0usize;\n");
    out.push_str("        let mut vars = std::collections::HashMap::new();\n");

    for tok in &tokens {
        match tok {
            TplToken::Var { name, segments } => {
                out.push_str(&format!(
                    "        if i + {segments} > segs.len() {{ return None; }}\n"
                ));
                if *segments == 1 {
                    out.push_str(&format!(
                        "        vars.insert(\"{name}\".to_string(), segs[i].to_string());\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "        vars.insert(\"{name}\".to_string(), segs[i..i+{segments}].join(\"/\"));\n"
                    ));
                }
                out.push_str(&format!("        i += {segments};\n"));
            }
            TplToken::Lit(s) => {
                out.push_str(&format!(
                    "        if segs.get(i) != Some(&\"{s}\") {{ return None; }}\n"
                ));
                out.push_str("        i += 1;\n");
            }
        }
    }

    out.push_str("        if i != segs.len() { return None; }\n");
    out.push_str("        Some(vars)\n");
    out.push_str("    }\n\n");
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
        FieldType::String | FieldType::Slug { .. } => ("&str".to_string(), format!("&self.{ident}")),
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

fn generate_from_map(out: &mut String, type_def: &TypeDef, fields: &[(String, FieldType)]) {
    let name = &type_def.name;
    out.push_str(
        "    fn from_map(fields: &std::collections::HashMap<String, String>) -> Self {\n",
    );
    out.push_str(&format!("        {name} {{\n"));
    for (field_name, field_type) in fields {
        let ident = rust_ident(field_name);
        let expr = match field_type {
            FieldType::String | FieldType::Slug { .. } => {
                format!("fields.get(\"{field_name}\").cloned().unwrap_or_default()")
            }
            FieldType::Integer => {
                format!("fields.get(\"{field_name}\").and_then(|v| v.parse().ok()).unwrap_or(0)")
            }
            FieldType::Float => {
                format!(
                    "fields.get(\"{field_name}\").and_then(|v| v.parse().ok()).unwrap_or(0.0)"
                )
            }
            FieldType::Boolean => {
                format!(
                    "fields.get(\"{field_name}\").and_then(|v| v.parse().ok()).unwrap_or(false)"
                )
            }
            FieldType::List(_) => {
                format!("fields.get(\"{field_name}\").and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()")
            }
        };
        out.push_str(&format!("            {ident}: {expr},\n"));
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
}

fn generate_store_methods(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    let snake = to_snake_case(name);
    let plural = format!("{snake}s");
    let store = format!("{name}Store");
    let registry_key = name.to_lowercase();
    let err = "loco_gen_schema::error::Error";
    let map = "std::collections::HashMap<String, String>";

    out.push_str(&format!(
        "pub struct {store}<'a> {{\n    registry: &'a loco_gen_schema::registry::SchemaRegistry,\n}}\n\n"
    ));

    out.push_str(&format!("impl<'a> {store}<'a> {{\n"));

    out.push_str(&format!(
        "    pub fn get(&self, key: &str) -> Option<{name}> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.get_instance(\"{registry_key}\", key).map(|f| {name}::from_map(&f))\n"
    ));
    out.push_str("    }\n\n");

    out.push_str("    pub fn has(&self, key: &str) -> bool {\n");
    out.push_str(&format!(
        "        self.registry.has_instance(\"{registry_key}\", key)\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn list(&self, prefix: &str) -> Vec<(String, {name})> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.list_instances(\"{registry_key}\", prefix).into_iter().map(|(k, v)| (k, {name}::from_map(&v))).collect()\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn list_all(&self) -> Vec<(String, {name})> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.list_all_instances(\"{registry_key}\").into_iter().map(|(k, v)| (k, {name}::from_map(&v))).collect()\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn create(&self, key: &str, fields: {map}) -> Result<{map}, {err}> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.create_instance(\"{registry_key}\", key, fields)\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn update(&self, key: &str, fields: {map}) -> Result<{map}, {err}> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.update_instance(\"{registry_key}\", key, fields)\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn delete(&self, key: &str) -> Result<(), {err}> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.delete_instance(\"{registry_key}\", key)\n"
    ));
    out.push_str("    }\n\n");

    out.push_str(&format!(
        "    pub fn delete_by_prefix(&self, prefix: &str) -> Result<Vec<String>, {err}> {{\n"
    ));
    out.push_str(&format!(
        "        self.registry.delete_instances_by_prefix(\"{registry_key}\", prefix)\n"
    ));
    out.push_str("    }\n");

    out.push_str("}\n\n");

    out.push_str("impl SchemaStore {\n");
    out.push_str(&format!(
        "    pub fn {plural}(&self) -> {store}<'_> {{\n"
    ));
    out.push_str(&format!(
        "        {store} {{ registry: &self.registry }}\n"
    ));
    out.push_str("    }\n");
    out.push_str("}\n");
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn field_type_literal(ft: &FieldType) -> String {
    match ft {
        FieldType::String => "loco_gen_schema::types::FieldType::String".to_string(),
        FieldType::Integer => "loco_gen_schema::types::FieldType::Integer".to_string(),
        FieldType::Float => "loco_gen_schema::types::FieldType::Float".to_string(),
        FieldType::Boolean => "loco_gen_schema::types::FieldType::Boolean".to_string(),
        FieldType::Slug { segments } => {
            format!("loco_gen_schema::types::FieldType::Slug {{ segments: {segments} }}")
        }
        FieldType::List(inner) => {
            format!(
                "loco_gen_schema::types::FieldType::List(Box::new({}))",
                field_type_literal(inner)
            )
        }
    }
}

fn property_literal(p: &Property) -> String {
    let name = &p.name;
    let ft = field_type_literal(&p.field_type);
    let co = p.create_only;
    format!(
        "loco_gen_schema::types::Property {{ name: \"{name}\".to_string(), field_type: {ft}, create_only: {co} }}"
    )
}

fn type_def_literal(td: &TypeDef) -> String {
    let name = esc(&td.name);
    let desc = esc(&td.description);
    let tmpl = esc(&td.path_template);
    let props_joined = td.properties.iter()
        .map(|p| format!("                {},", property_literal(p)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut s = String::new();
    s.push_str("        loco_gen_schema::types::TypeDef {\n");
    s.push_str(&format!("            name: \"{name}\".to_string(),\n"));
    s.push_str(&format!("            description: \"{desc}\".to_string(),\n"));
    s.push_str(&format!("            path_template: \"{tmpl}\".to_string(),\n"));
    s.push_str("            properties: vec![\n");
    s.push_str(&props_joined);
    s.push('\n');
    s.push_str("            ],\n");
    s.push_str("        }");
    s
}

fn generate_preamble(type_defs: &[TypeDef]) -> String {
    let type_def_literals = type_defs
        .iter()
        .map(type_def_literal)
        .collect::<Vec<_>>()
        .join(",\n");
    let err = "loco_gen_schema::error::Error";

    let mut out = String::new();
    out.push_str("pub struct SchemaStore {\n");
    out.push_str("    registry: loco_gen_schema::registry::SchemaRegistry,\n");
    out.push_str("}\n\n");
    out.push_str("impl SchemaStore {\n");
    out.push_str(&format!(
        "    pub fn load(instances_dir: &std::path::Path) -> Result<Self, {err}> {{\n"
    ));
    out.push_str("        let registry = loco_gen_schema::registry::SchemaRegistry::load(instances_dir, &schema_type_defs())?;\n");
    out.push_str("        Ok(SchemaStore { registry })\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("fn schema_type_defs() -> Vec<loco_gen_schema::types::TypeDef> {\n");
    out.push_str("    vec![\n");
    out.push_str(&type_def_literals);
    out.push_str(",\n");
    out.push_str("    ]\n");
    out.push_str("}\n");
    out
}

/// Generate a single file containing all type definitions.
pub fn generate_all(type_defs: &[TypeDef]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by loco-gen-codegen-build. Do not edit.\n\n");
    out.push_str(&generate_preamble(type_defs));
    out.push('\n');
    for type_def in type_defs {
        out.push_str(&generate(type_def));
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
            path_template: "${namespace}/versions/${version}/collection/${name}"
                .to_string(),
            properties: vec![
                Property {
                    name: "namespace".to_string(),
                    field_type: FieldType::Slug { segments: 2 },
                    create_only: true,
                },
                Property {
                    name: "version".to_string(),
                    field_type: FieldType::Slug { segments: 1 },
                    create_only: true,
                },
                Property {
                    name: "name".to_string(),
                    field_type: FieldType::Slug { segments: 1 },
                    create_only: true,
                },
                Property {
                    name: "item_count".to_string(),
                    field_type: FieldType::Integer,
                    create_only: false,
                },
                Property {
                    name: "average_rating".to_string(),
                    field_type: FieldType::Float,
                    create_only: false,
                },
                Property {
                    name: "is_active".to_string(),
                    field_type: FieldType::Boolean,
                    create_only: false,
                },
            ],
        }
    }

    #[test]
    fn test_generates_struct() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub struct Collection {"));
        assert!(code.contains("name: String,"));
        assert!(code.contains("item_count: i64,"));
        assert!(code.contains("average_rating: f64,"));
        assert!(code.contains("is_active: bool,"));
    }

    #[test]
    fn test_generates_constructor() {
        let code = generate(&sample_type_def());
        assert!(code.contains(
            "pub fn new(namespace: String, version: String, name: String, item_count: i64, average_rating: f64, is_active: bool) -> Self"
        ));
    }

    #[test]
    fn test_template_vars_generate_as_declared_fields() {
        let code = generate(&sample_type_def());
        assert!(code.contains("namespace: String,"));
        assert!(code.contains("version: String,"));
        assert!(code.contains("pub fn namespace(&self) -> &str"));
        assert!(code.contains("pub fn version(&self) -> &str"));
    }

    #[test]
    fn test_generates_accessors() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub fn name(&self) -> &str"));
        assert!(code.contains("pub fn item_count(&self) -> i64"));
        assert!(code.contains("pub fn average_rating(&self) -> f64"));
        assert!(code.contains("pub fn is_active(&self) -> bool"));
    }

    #[test]
    fn test_generates_from_map() {
        let code = generate(&sample_type_def());
        assert!(code.contains("fn from_map(fields: &std::collections::HashMap<String, String>) -> Self"));
        assert!(code.contains(r#"fields.get("name").cloned().unwrap_or_default()"#));
        assert!(code.contains(r#"fields.get("item_count").and_then(|v| v.parse().ok()).unwrap_or(0)"#));
        assert!(code.contains(r#"fields.get("average_rating").and_then(|v| v.parse().ok()).unwrap_or(0.0)"#));
        assert!(code.contains(r#"fields.get("is_active").and_then(|v| v.parse().ok()).unwrap_or(false)"#));
    }

    #[test]
    fn test_generates_store_methods() {
        let code = generate(&sample_type_def());
        // Per-type store wrapper borrows the registry
        assert!(code.contains("pub struct CollectionStore<'a> {"));
        assert!(code.contains("registry: &'a loco_gen_schema::registry::SchemaRegistry,"));
        assert!(code.contains("impl<'a> CollectionStore<'a> {"));
        assert!(code.contains("pub fn get(&self, key: &str) -> Option<Collection>"));
        assert!(code.contains("pub fn has(&self, key: &str) -> bool"));
        assert!(code.contains("pub fn list(&self, prefix: &str)"));
        assert!(code.contains("pub fn list_all(&self)"));
        assert!(code.contains("pub fn create(&self, key: &str,"));
        assert!(code.contains("pub fn update(&self, key: &str,"));
        assert!(code.contains("pub fn delete(&self, key: &str)"));
        assert!(code.contains("pub fn delete_by_prefix(&self, prefix: &str)"));
        assert!(code.contains(r#"self.registry.get_instance("collection", key)"#));
        assert!(code.contains(r#"self.registry.list_all_instances("collection")"#));
        // Accessor on SchemaStore
        assert!(code.contains("impl SchemaStore {"));
        assert!(code.contains("pub fn collections(&self) -> CollectionStore<'_>"));
        assert!(code.contains("CollectionStore { registry: &self.registry }"));
    }

    #[test]
    fn test_generate_all_includes_preamble() {
        use crate::types::Property;
        let td = TypeDef {
            name: "Site".to_string(),
            description: "A site".to_string(),
            path_template: "${project}/sites/${name}".to_string(),
            properties: vec![
                Property { name: "project".to_string(), field_type: FieldType::Slug { segments: 2 }, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::Slug { segments: 1 }, create_only: true },
            ],
        };
        let code = generate_all(&[td]);
        assert!(code.contains("pub struct SchemaStore {"));
        assert!(code.contains("pub fn load(instances_dir: &std::path::Path)"));
        assert!(code.contains("fn schema_type_defs()"));
        assert!(!code.contains("static SCHEMA"));
        assert!(!code.contains("init_schema"));
        assert!(!code.contains("with_schema"));
        assert!(code.contains("\"Site\".to_string()"));

        // Generic, type_name-keyed methods on SchemaStore must NOT be generated —
        // callers must dispatch to the per-type stores (e.g. `schema.sites()`) explicitly.
        assert!(!code.contains("pub fn list_all(&self, type_name:"));
        assert!(!code.contains("pub fn list(&self, type_name:"));
        assert!(!code.contains("pub fn get(&self, type_name:"));
        assert!(!code.contains("pub fn create(&self, type_name:"));
        assert!(!code.contains("pub fn update(&self, type_name:"));
        assert!(!code.contains("pub fn delete(&self, type_name:"));
    }

    #[test]
    fn test_list_field_generation() {
        let td = TypeDef {
            name: "Manifest".to_string(),
            description: "".to_string(),
            path_template: "${project}/versions/${version}/manifest".to_string(),
            properties: vec![
                Property {
                    name: "project".to_string(),
                    field_type: FieldType::Slug { segments: 2 },
                    create_only: true,
                },
                Property {
                    name: "version".to_string(),
                    field_type: FieldType::Slug { segments: 1 },
                    create_only: true,
                },
                Property {
                    name: "dependencies".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    create_only: false,
                },
            ],
        };
        let code = generate(&td);
        assert!(code.contains("dependencies: Vec<String>,"));
        assert!(code.contains("pub fn dependencies(&self) -> &[String]"));
        assert!(code.contains(r#"serde_json::from_str(v).ok()).unwrap_or_default()"#));
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("Collection"), "collection");
        assert_eq!(to_snake_case("MyType"), "my_type");
        assert_eq!(to_snake_case("DataSet"), "data_set");
    }

    #[test]
    fn test_generates_to_path() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub fn to_path(namespace: &str, version: &str, name: &str) -> String"));
        assert!(code.contains(r#"format!("{0}/versions/{1}/collection/{2}", namespace, version, name)"#));
    }

    #[test]
    fn test_generates_from_path() {
        let code = generate(&sample_type_def());
        assert!(code.contains(
            "pub fn from_path(path: &str) -> Option<std::collections::HashMap<String, String>>"
        ));
        // multi-segment slug joins
        assert!(code.contains("segs[i..i+2].join(\"/\")"));
        // literal segments are checked by exact match
        assert!(code.contains(r#"if segs.get(i) != Some(&"versions")"#));
        assert!(code.contains(r#"if segs.get(i) != Some(&"collection")"#));
    }

    #[test]
    fn test_to_path_and_from_path_roundtrip_via_eval() {
        // Sanity-check the emitted format string actually produces what we expect.
        // (We can't exec generated Rust from a test, so we just verify the emitted
        // format string is correct for a known shape.)
        let code = generate(&sample_type_def());
        assert!(code.contains(r#"format!("{0}/versions/{1}/collection/{2}""#));
    }

    #[test]
    fn test_rust_keyword_path_var() {
        // Template vars may collide with Rust keywords — ensure we escape them.
        let td = TypeDef {
            name: "Thing".to_string(),
            description: "".to_string(),
            path_template: "${type}/items/${name}".to_string(),
            properties: vec![
                Property { name: "type".to_string(), field_type: FieldType::Slug { segments: 1 }, create_only: true },
                Property { name: "name".to_string(), field_type: FieldType::Slug { segments: 1 }, create_only: true },
            ],
        };
        let code = generate(&td);
        assert!(code.contains("pub fn to_path(r#type: &str, name: &str) -> String"));
        assert!(code.contains(r#"format!("{0}/items/{1}", r#type, name)"#));
    }
}
