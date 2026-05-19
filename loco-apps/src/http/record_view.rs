//! Wire shape for records. Lake's `Record.id` is a canonical hyphenated UUID;
//! the wire format uses base58 short-ids (see [`crate::shortid`]). Build a
//! `RecordView` from a `Record` before serializing it into a response.

use std::collections::HashMap;

use serde::Serialize;

use loco_lake::{Record, Value};

use crate::shortid;

#[derive(Serialize)]
pub struct RecordView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<String>,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    pub owner: String,
    pub fields: HashMap<String, Value>,
}

impl From<Record> for RecordView {
    fn from(r: Record) -> Self {
        RecordView {
            id: shortid::encode(&r.id),
            dataset_id: r.dataset_id,
            created_at: r.created_at,
            created_by: r.created_by,
            updated_at: r.updated_at,
            updated_by: r.updated_by,
            owner: r.owner,
            fields: r.fields,
        }
    }
}
