#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    List(Box<FieldType>),
}

impl FieldType {
    /// Parse a scalar type name. Lists require an `items` sub-key and are built
    /// by the parser, not this function.
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
            FieldType::String => "String".to_string(),
            FieldType::Integer => "i64".to_string(),
            FieldType::Float => "f64".to_string(),
            FieldType::Boolean => "bool".to_string(),
            FieldType::List(inner) => format!("Vec<{}>", inner.rust_type()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub field_type: FieldType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub description: String,
    pub file_path_template: String,
    pub properties: Vec<Property>,
}

impl TypeDef {
    /// Extract `${var}` names from `file_path_template` in order of first appearance.
    pub fn template_vars(&self) -> Vec<String> {
        let mut result = Vec::new();
        let mut rest = self.file_path_template.as_str();
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

    /// All generated struct fields in order: template variables that aren't
    /// shadowed by a declared property (always `String`), then declared
    /// properties in declaration order. A declared property with the same
    /// name as a template variable shadows it.
    pub fn all_fields(&self) -> Vec<(String, FieldType)> {
        let declared: std::collections::HashSet<&str> =
            self.properties.iter().map(|p| p.name.as_str()).collect();
        let mut fields: Vec<(String, FieldType)> = self
            .template_vars()
            .into_iter()
            .filter(|v| !declared.contains(v.as_str()))
            .map(|name| (name, FieldType::String))
            .collect();
        for prop in &self.properties {
            fields.push((prop.name.clone(), prop.field_type.clone()));
        }
        fields
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<FieldValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub type_name: String,
    pub namespace: String,
    pub values: Vec<(String, FieldValue)>,
}
