#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
}

impl FieldType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "string" => Some(FieldType::String),
            "integer" => Some(FieldType::Integer),
            "float" => Some(FieldType::Float),
            "boolean" => Some(FieldType::Boolean),
            _ => None,
        }
    }

    pub fn rust_type(&self) -> &'static str {
        match self {
            FieldType::String => "String",
            FieldType::Integer => "i64",
            FieldType::Float => "f64",
            FieldType::Boolean => "bool",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub field_type: FieldType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    /// Namespaced and versioned instances (user/project/version path)
    Namespaced,
    /// Global config instances (flat, no namespace or version)
    Global,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "namespaced" => Some(Scope::Namespaced),
            "global" => Some(Scope::Global),
            _ => None,
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, Scope::Global)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub description: String,
    pub scope: Scope,
    pub file_path_template: Option<String>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub version: u32,
    pub type_def: TypeDef,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub type_name: String,
    pub namespace: String,
    pub values: Vec<(String, FieldValue)>,
}
