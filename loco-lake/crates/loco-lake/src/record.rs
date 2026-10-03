use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::value::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub dataset_id: String,
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
        Self::with_id(
            dataset_id,
            &uuid::Uuid::new_v4().to_string(),
            &req.user,
            req.fields,
        )
    }

    /// A record whose id the caller chose. Same stamps as [`Self::new_for_insert`].
    ///
    /// Reserved lake collections (`$secrets`, `$variables`) use this so the id
    /// is the declaration name and a later write replaces that row.
    pub fn with_id(
        dataset_id: &str,
        id: &str,
        user: &str,
        fields: HashMap<String, Value>,
    ) -> Record {
        let now = chrono::Utc::now().to_rfc3339();
        Record {
            id: id.to_string(),
            dataset_id: dataset_id.to_string(),
            created_at: now.clone(),
            created_by: user.to_string(),
            updated_at: now,
            updated_by: user.to_string(),
            owner: user.to_string(),
            fields,
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
