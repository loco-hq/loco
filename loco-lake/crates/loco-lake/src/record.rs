use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::value::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
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

/// Caller-supplied data for creating a new record. The lake stamps id,
/// timestamps, and the user/created_by/owner fields.
pub struct InsertRequest {
    pub user: String,
    pub fields: HashMap<String, Value>,
}

/// Caller-supplied patch for updating a record. The lake preserves
/// id/created_*/owner from the existing record and stamps updated_*.
pub struct UpdatePatch {
    pub user: String,
    pub fields: HashMap<String, Value>,
}

impl Record {
    /// Build a fresh record from an insert request — generates the id and
    /// stamps all system fields.
    pub fn new_for_insert(dataset_id: &str, req: InsertRequest) -> Record {
        let now = chrono::Utc::now().to_rfc3339();
        Record {
            id: uuid::Uuid::new_v4().to_string(),
            dataset_id: Some(dataset_id.to_string()),
            created_at: now.clone(),
            created_by: req.user.clone(),
            updated_at: now,
            updated_by: req.user.clone(),
            owner: req.user,
            fields: req.fields,
        }
    }

    /// Apply a patch to an existing record — merges fields and stamps
    /// updated_at/updated_by. Preserves id, created_*, dataset_id, owner.
    pub fn apply_patch(mut self, patch: UpdatePatch) -> Record {
        for (k, v) in patch.fields {
            self.fields.insert(k, v);
        }
        self.updated_at = chrono::Utc::now().to_rfc3339();
        self.updated_by = patch.user;
        self
    }
}
