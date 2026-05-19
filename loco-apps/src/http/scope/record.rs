use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::server::AppState;
use crate::validation::{ValidationMode, ValidationReport};

use loco_lake::Value;

use super::collection::CollectionScope;
use super::helpers::read_path_params;

#[derive(Deserialize)]
struct RecordPathParams {
    id: String,
}

/// A `CollectionScope` plus the `{id}` of a specific record. Use this for
/// record-level data routes (get/update/delete) so handlers don't need to
/// extract `Path` separately.
pub struct RecordScope {
    pub collection: CollectionScope,
    pub id: String,
}

impl RecordScope {
    pub fn dataset_id(&self) -> String {
        self.collection.dataset_id()
    }

    pub fn collection_key(&self) -> &str {
        &self.collection.collection_key
    }

    pub fn user(&self) -> &AuthUser {
        self.collection.user()
    }

    /// Validate this record's fields against its collection's schema.
    /// Convenience that forwards to [`CollectionScope::validate`].
    pub fn validate(
        &self,
        fields: &HashMap<String, Value>,
        mode: ValidationMode,
    ) -> ValidationReport {
        self.collection.validate(fields, mode)
    }
}

impl FromRequestParts<Arc<AppState>> for RecordScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let collection = CollectionScope::from_request_parts(parts, state).await?;
        let RecordPathParams { id } = read_path_params(parts, state).await?;
        Ok(RecordScope { collection, id })
    }
}
