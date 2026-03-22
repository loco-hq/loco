use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    String(std::string::String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
}
