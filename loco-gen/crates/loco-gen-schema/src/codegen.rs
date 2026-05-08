use crate::types::{FieldType, TypeDef};

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

fn plural_field(name: &str) -> String {
    rust_ident(&format!("{}s", to_snake_case(name)))
}

/// Generate Rust source code for a single TypeDef.
pub fn generate(type_def: &TypeDef) -> String {
    let name = &type_def.name;
    let fields = type_def.all_fields();
    let mut out = String::new();

    out.push_str("#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]\n");
    out.push_str(&format!("pub struct {name} {{\n"));
    for (field_name, field_type) in &fields {
        out.push_str("    #[serde(default)]\n");
        out.push_str(&format!(
            "    pub {}: {},\n",
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
    generate_to_map(&mut out, &fields);
    generate_to_path_static(&mut out, type_def);
    generate_from_path(&mut out, type_def);
    generate_from_yaml(&mut out, type_def);
    out.push_str("}\n\n");

    generate_update_struct(&mut out, type_def);
    generate_schema_instance_impl(&mut out, type_def);
    generate_store_alias(&mut out, type_def);

    out
}

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

fn generate_to_path_static(out: &mut String, type_def: &TypeDef) {
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
        "    pub fn from_map(fields: &std::collections::HashMap<String, String>) -> Self {\n",
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
    out.push_str("    }\n\n");
}

fn to_map_insert_expr(field_name: &str, field_type: &FieldType, owner: &str) -> String {
    let ident = rust_ident(field_name);
    let access = format!("{owner}.{ident}");
    match field_type {
        FieldType::String | FieldType::Slug { .. } => format!(
            "m.insert(\"{field_name}\".to_string(), {access}.clone());"
        ),
        FieldType::Integer | FieldType::Float | FieldType::Boolean => format!(
            "m.insert(\"{field_name}\".to_string(), {access}.to_string());"
        ),
        FieldType::List(_) => format!(
            "m.insert(\"{field_name}\".to_string(), serde_json::to_string(&{access}).unwrap_or_default());"
        ),
    }
}

fn generate_to_map(out: &mut String, fields: &[(String, FieldType)]) {
    out.push_str(
        "    pub fn to_map(&self) -> std::collections::HashMap<String, String> {\n",
    );
    out.push_str("        let mut m = std::collections::HashMap::new();\n");
    for (field_name, field_type) in fields {
        out.push_str(&format!(
            "        {}\n",
            to_map_insert_expr(field_name, field_type, "self")
        ));
    }
    out.push_str("        m\n");
    out.push_str("    }\n\n");
}

fn generate_from_yaml(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    let template_vars = type_def.template_vars();
    let template_var_set: std::collections::HashSet<&str> =
        template_vars.iter().map(|s| s.as_str()).collect();

    out.push_str(
        "    pub fn from_yaml(yaml: &str, vars: &std::collections::HashMap<String, String>) -> Result<Self, loco_schema_runtime::Error> {\n"
    );
    out.push_str("        let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;\n");
    out.push_str("        let mapping = value.as_mapping();\n");
    out.push_str(&format!("        Ok({name} {{\n"));
    for (field_name, field_type) in &type_def.all_fields() {
        let ident = rust_ident(field_name);
        let expr = if template_var_set.contains(field_name.as_str()) {
            format!("vars.get(\"{field_name}\").cloned().unwrap_or_default()")
        } else {
            yaml_coerce_expr(field_name, field_type)
        };
        out.push_str(&format!("            {ident}: {expr},\n"));
    }
    out.push_str("        })\n");
    out.push_str("    }\n");
}

fn yaml_coerce_expr(field_name: &str, field_type: &FieldType) -> String {
    let lookup = format!("mapping.and_then(|m| m.get(\"{field_name}\"))");
    match field_type {
        FieldType::String | FieldType::Slug { .. } => format!(
            "{lookup}.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default()"
        ),
        FieldType::Integer => format!("{lookup}.and_then(|v| v.as_i64()).unwrap_or(0)"),
        FieldType::Float => format!("{lookup}.and_then(|v| v.as_f64()).unwrap_or(0.0)"),
        FieldType::Boolean => format!("{lookup}.and_then(|v| v.as_bool()).unwrap_or(false)"),
        FieldType::List(inner) => {
            let item_expr = yaml_item_coerce_expr(inner);
            format!(
                "{lookup}.and_then(|v| v.as_sequence()).map(|seq| seq.iter().filter_map(|item| {item_expr}).collect()).unwrap_or_default()"
            )
        }
    }
}

fn yaml_item_coerce_expr(field_type: &FieldType) -> String {
    match field_type {
        FieldType::String | FieldType::Slug { .. } => {
            "item.as_str().map(|s| s.to_string())".to_string()
        }
        FieldType::Integer => "item.as_i64()".to_string(),
        FieldType::Float => "item.as_f64()".to_string(),
        FieldType::Boolean => "item.as_bool()".to_string(),
        FieldType::List(_) => unreachable!("nested lists are rejected at parse time"),
    }
}

fn mutable_fields(type_def: &TypeDef) -> Vec<(String, FieldType)> {
    type_def
        .properties
        .iter()
        .filter(|p| !p.create_only)
        .map(|p| (p.name.clone(), p.field_type.clone()))
        .collect()
}

fn generate_update_struct(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    let update_name = format!("{name}Update");
    let fields = mutable_fields(type_def);

    out.push_str("#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]\n");
    out.push_str(&format!("pub struct {update_name} {{\n"));
    for (field_name, field_type) in &fields {
        out.push_str("    #[serde(default)]\n");
        out.push_str(&format!(
            "    pub {}: Option<{}>,\n",
            rust_ident(field_name),
            field_type.rust_type()
        ));
    }
    out.push_str("}\n\n");

    out.push_str(&format!("impl {update_name} {{\n"));

    // from_map
    out.push_str(
        "    pub fn from_map(fields: &std::collections::HashMap<String, String>) -> Self {\n",
    );
    out.push_str(&format!("        {update_name} {{\n"));
    for (field_name, field_type) in &fields {
        let ident = rust_ident(field_name);
        let expr = match field_type {
            FieldType::String | FieldType::Slug { .. } => {
                format!("fields.get(\"{field_name}\").cloned()")
            }
            FieldType::Integer | FieldType::Float | FieldType::Boolean => {
                format!("fields.get(\"{field_name}\").and_then(|v| v.parse().ok())")
            }
            FieldType::List(_) => {
                format!("fields.get(\"{field_name}\").and_then(|v| serde_json::from_str(v).ok())")
            }
        };
        out.push_str(&format!("            {ident}: {expr},\n"));
    }
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // to_map
    out.push_str(
        "    pub fn to_map(&self) -> std::collections::HashMap<String, String> {\n",
    );
    out.push_str("        let mut m = std::collections::HashMap::new();\n");
    for (field_name, field_type) in &fields {
        let ident = rust_ident(field_name);
        let insert = match field_type {
            FieldType::String | FieldType::Slug { .. } => format!(
                "m.insert(\"{field_name}\".to_string(), v.clone());"
            ),
            FieldType::Integer | FieldType::Float | FieldType::Boolean => format!(
                "m.insert(\"{field_name}\".to_string(), v.to_string());"
            ),
            FieldType::List(_) => format!(
                "m.insert(\"{field_name}\".to_string(), serde_json::to_string(v).unwrap_or_default());"
            ),
        };
        out.push_str(&format!(
            "        if let Some(v) = &self.{ident} {{ {insert} }}\n"
        ));
    }
    out.push_str("        m\n");
    out.push_str("    }\n\n");

    // apply: merge Some() fields into a mutable target
    out.push_str(&format!(
        "    pub fn apply(&self, target: &mut {name}) {{\n"
    ));
    for (field_name, field_type) in &fields {
        let ident = rust_ident(field_name);
        let assign = match field_type {
            FieldType::String | FieldType::Slug { .. } | FieldType::List(_) => {
                format!("target.{ident} = v.clone();")
            }
            FieldType::Integer | FieldType::Float | FieldType::Boolean => {
                format!("target.{ident} = *v;")
            }
        };
        out.push_str(&format!(
            "        if let Some(v) = &self.{ident} {{ {assign} }}\n"
        ));
    }
    out.push_str("    }\n");

    out.push_str("}\n\n");
}

fn generate_schema_instance_impl(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    let update_name = format!("{name}Update");
    let vars = type_def.template_vars();
    let to_path_args: Vec<String> = vars
        .iter()
        .map(|v| format!("&self.{}", rust_ident(v)))
        .collect();

    out.push_str(&format!(
        "impl loco_schema_runtime::SchemaInstance for {name} {{\n"
    ));
    out.push_str(&format!("    type Update = {update_name};\n"));
    out.push_str("    fn to_path(&self) -> String {\n");
    out.push_str(&format!(
        "        {name}::to_path({})\n",
        to_path_args.join(", ")
    ));
    out.push_str("    }\n");
    out.push_str(&format!(
        "    fn apply_update(&mut self, patch: &{update_name}) {{\n"
    ));
    out.push_str("        patch.apply(self);\n");
    out.push_str("    }\n");
    // Delegate to the inherent `from_path` / `from_yaml`. Inherent methods
    // take precedence over trait methods at concrete types, so this is not
    // recursive.
    out.push_str(
        "    fn from_path(path: &str) -> Option<std::collections::HashMap<String, String>> {\n",
    );
    out.push_str(&format!("        {name}::from_path(path)\n"));
    out.push_str("    }\n");
    out.push_str(
        "    fn from_yaml(yaml: &str, vars: &std::collections::HashMap<String, String>) -> Result<Self, loco_schema_runtime::Error> {\n"
    );
    out.push_str(&format!("        {name}::from_yaml(yaml, vars)\n"));
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

fn generate_store_alias(out: &mut String, type_def: &TypeDef) {
    let name = &type_def.name;
    let plural = plural_field(name);
    let store = format!("{name}Store");

    out.push_str(&format!(
        "pub type {store} = loco_schema_runtime::InstanceStore<{name}>;\n\n"
    ));

    out.push_str("impl SchemaStore {\n");
    out.push_str(&format!("    pub fn {plural}(&self) -> &{store} {{\n"));
    out.push_str(&format!("        &self.{plural}\n"));
    out.push_str("    }\n");
    out.push_str("}\n");
}

fn generate_preamble(type_defs: &[TypeDef]) -> String {
    let mut out = String::new();

    out.push_str("pub struct SchemaStore {\n");
    for td in type_defs {
        let plural = plural_field(&td.name);
        let store = format!("{}Store", td.name);
        out.push_str(&format!("    {plural}: {store},\n"));
    }
    out.push_str("}\n\n");

    out.push_str("impl SchemaStore {\n");
    out.push_str("    pub fn load(instances_dir: &std::path::Path) -> Result<Self, loco_schema_runtime::Error> {\n");
    for td in type_defs {
        let type_name = &td.name;
        let plural = plural_field(type_name);
        let store = format!("{type_name}Store");
        out.push_str(&format!(
            "        let {plural}_adapter: std::sync::Arc<dyn loco_schema_runtime::SchemaPersistence<{type_name}>> = std::sync::Arc::new(loco_schema_runtime::YamlFsAdapter::<{type_name}>::new(instances_dir.to_path_buf()));\n"
        ));
        out.push_str(&format!(
            "        let {plural} = {store}::new({plural}_adapter.clone());\n"
        ));
        out.push_str(&format!(
            "        for (key, inst) in {plural}_adapter.load_all()? {{\n"
        ));
        out.push_str(&format!(
            "            {plural}.insert_loaded(key, std::sync::Arc::new(inst));\n"
        ));
        out.push_str("        }\n");
    }
    out.push_str("        Ok(SchemaStore {\n");
    for td in type_defs {
        let plural = plural_field(&td.name);
        out.push_str(&format!("            {plural},\n"));
    }
    out.push_str("        })\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

/// Generate a single file containing all type definitions.
pub fn generate_all(type_defs: &[TypeDef]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by loco-gen-schema. Do not edit.\n\n");
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
        assert!(code.contains("pub name: String,"));
        assert!(code.contains("pub item_count: i64,"));
        assert!(code.contains("pub average_rating: f64,"));
        assert!(code.contains("pub is_active: bool,"));
        // Main struct derives Default + Serialize + Deserialize so it can round-trip
        // through YAML and accept partial JSON inputs (with `#[serde(default)]` per
        // field) when the route layer fills in scope-provided fields.
        assert!(code.contains("#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]"));
        assert!(code.contains("#[serde(default)]\n    pub item_count: i64,"));
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
        assert!(code.contains("pub namespace: String,"));
        assert!(code.contains("pub version: String,"));
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
        assert!(code.contains("pub fn from_map(fields: &std::collections::HashMap<String, String>) -> Self"));
        assert!(code.contains(r#"fields.get("name").cloned().unwrap_or_default()"#));
        assert!(code.contains(r#"fields.get("item_count").and_then(|v| v.parse().ok()).unwrap_or(0)"#));
    }

    #[test]
    fn test_generates_from_yaml() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub fn from_yaml(yaml: &str, vars: &std::collections::HashMap<String, String>) -> Result<Self, loco_schema_runtime::Error>"));
        // Template vars sourced from `vars`, not from the YAML body.
        assert!(code.contains(r#"name: vars.get("name").cloned().unwrap_or_default(),"#));
        // Non-template-var fields coerce out of the YAML mapping.
        assert!(code.contains("mapping.and_then(|m| m.get(\"item_count\"))"));
    }

    #[test]
    fn test_generates_store_alias_and_accessor() {
        let code = generate(&sample_type_def());
        // Per-type store is a type alias over the generic InstanceStore.
        assert!(code.contains("pub type CollectionStore = loco_schema_runtime::InstanceStore<Collection>;"));
        // SchemaStore gets an accessor returning a borrowed store.
        assert!(code.contains("impl SchemaStore {"));
        assert!(code.contains("pub fn collections(&self) -> &CollectionStore"));
        assert!(code.contains("&self.collections"));
        // No handwritten Store wrapper struct anymore.
        assert!(!code.contains("pub struct CollectionStore<'a>"));
        // No references to loco_gen_schema at runtime.
        assert!(!code.contains("loco_gen_schema"));
    }

    #[test]
    fn test_generates_schema_instance_impl() {
        let code = generate(&sample_type_def());
        assert!(code.contains("impl loco_schema_runtime::SchemaInstance for Collection {"));
        assert!(code.contains("type Update = CollectionUpdate;"));
        assert!(code.contains("fn to_path(&self) -> String"));
        assert!(code.contains("Collection::to_path(&self.namespace, &self.version, &self.name)"));
        assert!(code.contains("fn apply_update(&mut self, patch: &CollectionUpdate)"));
        assert!(code.contains("patch.apply(self);"));
        // Trait impl forwards from_path / from_yaml to the inherent methods.
        assert!(code.contains("fn from_path(path: &str) -> Option<std::collections::HashMap<String, String>>"));
        assert!(code.contains("Collection::from_path(path)"));
        assert!(code.contains("fn from_yaml(yaml: &str, vars: &std::collections::HashMap<String, String>) -> Result<Self, loco_schema_runtime::Error>"));
        assert!(code.contains("Collection::from_yaml(yaml, vars)"));
    }

    #[test]
    fn test_generates_to_map() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub fn to_map(&self) -> std::collections::HashMap<String, String>"));
        assert!(code.contains(r#"m.insert("name".to_string(), self.name.clone());"#));
        assert!(code.contains(r#"m.insert("item_count".to_string(), self.item_count.to_string());"#));
        assert!(code.contains(r#"m.insert("is_active".to_string(), self.is_active.to_string());"#));
    }

    #[test]
    fn test_generates_update_struct() {
        let code = generate(&sample_type_def());
        assert!(code.contains("pub struct CollectionUpdate {"));
        assert!(code.contains("pub item_count: Option<i64>,"));
        assert!(code.contains("pub average_rating: Option<f64>,"));
        assert!(code.contains("pub is_active: Option<bool>,"));
        // createOnly template vars must NOT appear in the update struct.
        assert!(!code.contains("pub namespace: Option"));
        assert!(!code.contains("pub version: Option"));
        // Bare `pub name: Option` would match, but name is createOnly so it's excluded.
        assert!(!code.contains("pub name: Option"));

        assert!(code.contains(
            r#"if let Some(v) = &self.item_count { m.insert("item_count".to_string(), v.to_string()); }"#
        ));
        assert!(code.contains(
            "pub fn from_map(fields: &std::collections::HashMap<String, String>) -> Self"
        ));
        assert!(code.contains(r#"fields.get("item_count").and_then(|v| v.parse().ok())"#));

        // apply merges only the Some-fields into a mutable target.
        assert!(code.contains("pub fn apply(&self, target: &mut Collection)"));
        assert!(code.contains("if let Some(v) = &self.item_count { target.item_count = *v; }"));
    }

    #[test]
    fn test_generate_all_preamble_is_registry_free() {
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
        // SchemaStore owns per-type InstanceStores directly — no SchemaRegistry.
        assert!(code.contains("pub struct SchemaStore {"));
        assert!(code.contains("sites: SiteStore,"));
        assert!(code.contains("pub fn load(instances_dir: &std::path::Path) -> Result<Self, loco_schema_runtime::Error>"));
        // Each type gets its own YAML/FS adapter, wrapped in an Arc and shared
        // with the InstanceStore.
        assert!(code.contains("loco_schema_runtime::YamlFsAdapter::<Site>::new(instances_dir.to_path_buf())"));
        assert!(code.contains("loco_schema_runtime::SchemaPersistence<Site>"));
        assert!(code.contains("SiteStore::new(sites_adapter.clone())"));
        // Loading is per-type via adapter.load_all().
        assert!(code.contains("sites_adapter.load_all()?"));
        assert!(code.contains("sites.insert_loaded(key, std::sync::Arc::new(inst))"));

        // The shared FS walk that used to live in the preamble is gone.
        assert!(!code.contains("loco_schema_runtime::io::"));
        assert!(!code.contains("collect_yaml_files"));
        assert!(!code.contains("rel_no_ext"));

        // None of these should be emitted anymore.
        assert!(!code.contains("SchemaRegistry"));
        assert!(!code.contains("schema_type_defs"));
        assert!(!code.contains("loco_gen_schema"));
        assert!(!code.contains("registry: "));

        // Generic, type_name-keyed methods on SchemaStore must NOT be generated.
        assert!(!code.contains("pub fn list_all(&self, type_name:"));
        assert!(!code.contains("pub fn list(&self, type_name:"));
        assert!(!code.contains("pub fn get(&self, type_name:"));
        assert!(!code.contains("pub fn create(&self, type_name:"));
        assert!(!code.contains("pub fn update(&self, type_name:"));
        assert!(!code.contains("pub fn delete(&self, type_name:"));

        // Per-type Update struct is still generated.
        assert!(code.contains("pub struct SiteUpdate {"));
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
        assert!(code.contains("pub dependencies: Vec<String>,"));
        assert!(code.contains("pub fn dependencies(&self) -> &[String]"));
        // Loose-on-load list coercion lives in from_yaml now.
        assert!(code.contains("v.as_sequence()"));
        assert!(code.contains("item.as_str().map(|s| s.to_string())"));
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
        assert!(code.contains("segs[i..i+2].join(\"/\")"));
        assert!(code.contains(r#"if segs.get(i) != Some(&"versions")"#));
        assert!(code.contains(r#"if segs.get(i) != Some(&"collection")"#));
    }

    #[test]
    fn test_rust_keyword_path_var() {
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
