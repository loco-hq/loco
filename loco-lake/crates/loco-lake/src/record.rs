use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::value::Value;

#[derive(Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    pub owner: String,
    pub fields: HashMap<String, Value>,
}
