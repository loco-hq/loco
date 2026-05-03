use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::server::AppState;
use crate::validation::{validate_record, validate_records, ValidationMode, ValidationReport};

use loco_lake::Value;

use super::helpers::read_path_params;
use super::site::SiteScope;

#[derive(Deserialize)]
struct CollectionPathParams {
    name: String,
}

/// A `SiteScope` plus a validated collection from the request path's `{name}`.
/// Use this for data routes scoped to a single collection — it pre-resolves
/// `collection_key` so handlers don't repeat the validation dance.
pub struct CollectionScope {
    pub site: SiteScope,
    pub collection_key: String,
    /// Bare collection name (e.g. "account") — needed for schema lookups
    /// where `collection_key` (`"user/project.name"`) is the wrong shape.
    pub collection_name: String,
}

impl CollectionScope {
    pub fn dataset_id(&self) -> String {
        self.site.dataset_id()
    }

    pub fn user(&self) -> &AuthUser {
        self.site.user()
    }

    pub fn project_id(&self) -> String {
        self.site.project.project_id()
    }

    /// Schema version pinned by the site this scope belongs to.
    pub fn version(&self) -> &str {
        self.site.site.version()
    }

    /// Validate a single record's fields against this collection's schema.
    /// Thin adapter over [`validate_record`] — keeps the validator pure and
    /// gives handlers a one-line call site.
    pub fn validate(
        &self,
        fields: &HashMap<String, Value>,
        mode: ValidationMode,
    ) -> ValidationReport {
        validate_record(&self.site.schema, &self.collection_name, fields, mode)
    }

    /// Validate every record in a list against this collection's schema.
    /// Diagnostics are aggregated and each path is prefixed with its record id.
    pub fn validate_records<'a, I>(
        &self,
        records: I,
        mode: ValidationMode,
    ) -> ValidationReport
    where
        I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
    {
        validate_records(&self.site.schema, &self.collection_name, records, mode)
    }
}

impl FromRequestParts<Arc<AppState>> for CollectionScope {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let site = SiteScope::from_request_parts(parts, state).await?;
        let CollectionPathParams { name } = read_path_params(parts, state).await?;
        let collection_key = site.require_collection(&name)?;
        Ok(CollectionScope {
            site,
            collection_key,
            collection_name: name,
        })
    }
}
