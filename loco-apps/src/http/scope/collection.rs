use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::http::authz::{forbidden, public_may_create, public_may_read};
use crate::server::AppState;
use crate::validation::{validate_record, validate_records, ValidationMode, ValidationReport};
use crate::PermissionSet;

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
    pub fn validate_records<'a, I>(&self, records: I, mode: ValidationMode) -> ValidationReport
    where
        I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
    {
        validate_records(&self.site.schema, &self.collection_name, records, mode)
    }

    /// Permission sets the site assigns to `public`, resolved against the
    /// pinned version (self + direct deps). Unknown names are skipped.
    fn public_sets(&self) -> Vec<Arc<PermissionSet>> {
        self.site
            .site
            .public_permission_sets()
            .iter()
            .filter_map(|name| self.site.schema.permission_set(name))
            .collect()
    }

    /// List/get: members with data access, or anyone when a stacked set
    /// grants `read` on this collection.
    pub fn require_can_read_data(&self) -> Result<(), Response> {
        if self.site.has_data_access()? {
            return Ok(());
        }
        let sets = self.public_sets();
        if public_may_read(sets.iter().map(|s| s.as_ref()), &self.collection_name) {
            return Ok(());
        }
        Err(forbidden())
    }

    /// Insert: members with data access, or the `public` principal when a
    /// stacked set grants `create`. Authenticated non-members cannot use
    /// the public-create hole.
    pub fn require_can_create_data(&self) -> Result<(), Response> {
        if self.site.has_data_access()? {
            return Ok(());
        }
        if !self.site.is_public() {
            return Err(forbidden());
        }
        let sets = self.public_sets();
        if public_may_create(sets.iter().map(|s| s.as_ref()), &self.collection_name) {
            return Ok(());
        }
        Err(forbidden())
    }

    /// Update/delete: members only. Public never mutates.
    pub fn require_can_write_data(&self) -> Result<(), Response> {
        self.site.require_can_write_data()
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
