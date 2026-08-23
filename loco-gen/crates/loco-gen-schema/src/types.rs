#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    List(Box<FieldType>),
    /// A path-safe identifier: `[a-z0-9_.-]+` per segment, `/`-separated.
    /// `segments` controls how many `/`-separated parts are expected (default 1).
    Slug {
        segments: u32,
    },
    /// Inline nested struct. Not an instance — no pathTemplate, no store.
    /// `name` is the generated Rust struct ident (must be unique in the crate).
    Object {
        name: String,
        properties: Vec<Property>,
    },
}

impl FieldType {
    /// Parse a scalar type name. Lists, slugs, and objects have sub-keys and
    /// are built by the parser, not this function.
    pub fn parse_scalar(s: &str) -> Option<Self> {
        match s {
            "string" => Some(FieldType::String),
            "integer" => Some(FieldType::Integer),
            "float" => Some(FieldType::Float),
            "boolean" => Some(FieldType::Boolean),
            _ => None,
        }
    }

    pub fn rust_type(&self) -> String {
        match self {
            FieldType::String | FieldType::Slug { .. } => "String".to_string(),
            FieldType::Integer => "i64".to_string(),
            FieldType::Float => "f64".to_string(),
            FieldType::Boolean => "bool".to_string(),
            FieldType::List(inner) => format!("Vec<{}>", inner.rust_type()),
            FieldType::Object { name, .. } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub field_type: FieldType,
    pub create_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub description: String,
    pub path_template: String,
    pub properties: Vec<Property>,
}

impl TypeDef {
    /// Extract `${var}` names from `path_template` in order of first appearance.
    pub fn template_vars(&self) -> Vec<String> {
        let mut result = Vec::new();
        let mut rest = self.path_template.as_str();
        while let Some(start) = rest.find("${") {
            let after = &rest[start + 2..];
            if let Some(end) = after.find('}') {
                let var = after[..end].to_string();
                if !result.contains(&var) {
                    result.push(var);
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
        result
    }

    /// All generated struct fields in declaration order.
    /// Template variables must be declared as properties (enforced at parse time).
    pub fn all_fields(&self) -> Vec<(String, FieldType)> {
        self.properties
            .iter()
            .map(|p| (p.name.clone(), p.field_type.clone()))
            .collect()
    }
}
