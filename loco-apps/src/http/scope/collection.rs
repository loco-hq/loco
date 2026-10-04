use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::http::authz::{forbidden, DataVerb};
use crate::http::paths::collection_key;
use crate::http::version_schema::{CollectionAddress, CollectionAddressKind};
use crate::server::AppState;
use crate::validation::{validate_record, validate_records, ValidationMode, ValidationReport};

use loco_lake::Value;

use super::helpers::read_path_params;
use super::site::SiteScope;

#[derive(Deserialize)]
struct CollectionPathParams {
    name: String,
}

/// A `SiteScope` plus the collection address from the request path's `{name}`.
///
/// `{name}` is one percent-encoded segment: bare for the site's own
/// collection, `{account}/{project}.{local}` for a dependency's, and
/// `{integration}:{name}` inside `local` for a connection.
/// `/data/acme%2Fcrm.contacts/list`, `/data/sf_east:account/list`. The router
/// matches on the raw path, so the `%2F` never splits the segment; `Path`
/// decodes it. `%3A` decodes to the same `:` the segment may contain as-is.
pub struct CollectionScope {
    pub site: SiteScope,
    pub address: CollectionAddress,
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

    /// Lake key for an ordinary collection. An integration address has no
    /// lake collection until a source exists (#125); callers answer 501
    /// instead of reading the lake. An ambiguous address has none either:
    /// both documents share the name, so the caller answers 409.
    pub fn lake_key(&self) -> Option<String> {
        match self.address.kind {
            CollectionAddressKind::Ordinary => {
                Some(collection_key(&self.address.owner, &self.address.name))
            }
            CollectionAddressKind::Standard { .. }
            | CollectionAddressKind::Custom
            | CollectionAddressKind::Ambiguous => None,
        }
    }

    /// How a client names this address from the site's version: bare for the
    /// running project, `{project}.{local}` for a dependency.
    pub fn canonical(&self) -> String {
        self.site
            .schema
            .reference(&self.address.project, &self.address.local)
    }

    /// Validate a single record's fields against this collection's schema.
    /// Thin adapter over [`validate_record`] — keeps the validator pure and
    /// gives handlers a one-line call site. Integration addresses do not
    /// reach this: a data verb returns 501 before validation.
    pub fn validate(
        &self,
        fields: &HashMap<String, Value>,
        mode: ValidationMode,
    ) -> ValidationReport {
        validate_record(
            &self.site.schema,
            &self.address.owner,
            &self.address.name,
            fields,
            mode,
        )
    }

    /// Validate every record in a list against this collection's schema.
    /// Diagnostics are aggregated and each path is prefixed with its record id.
    pub fn validate_records<'a, I>(&self, records: I, mode: ValidationMode) -> ValidationReport
    where
        I: IntoIterator<Item = (&'a str, &'a HashMap<String, Value>)>,
    {
        validate_records(
            &self.site.schema,
            &self.address.owner,
            &self.address.name,
            records,
            mode,
            None,
        )
    }

    fn public_allowed(&self, verb: DataVerb) -> bool {
        self.site
            .public_allowed(&self.address.local, &self.address.project, verb)
    }

    /// List/get: members with data access, or anyone when a stacked set
    /// grants `read` on this address. The grant names the local
    /// (`sf_east:account` or `orders`) and the address root, not the type
    /// that owns the fields.
    pub fn require_can_read_data(&self) -> Result<(), Response> {
        if self
            .site
            .may_read_collection(&self.address.local, &self.address.project)?
        {
            Ok(())
        } else {
            Err(forbidden())
        }
    }

    /// Insert: members with data access, or the `public` principal when a
    /// stacked set grants `create`. Authenticated non-members cannot use
    /// the public write hole.
    pub fn require_can_create_data(&self) -> Result<(), Response> {
        self.require_public_write(DataVerb::Create)
    }

    /// Update: members, or `public` when a stacked set grants `update`.
    pub fn require_can_update_data(&self) -> Result<(), Response> {
        self.require_public_write(DataVerb::Update)
    }

    /// Delete: members, or `public` when a stacked set grants `delete`.
    pub fn require_can_delete_data(&self) -> Result<(), Response> {
        self.require_public_write(DataVerb::Delete)
    }

    fn require_public_write(&self, verb: DataVerb) -> Result<(), Response> {
        if self.site.has_data_access()? {
            return Ok(());
        }
        if !self.site.is_public() {
            return Err(forbidden());
        }
        if self.public_allowed(verb) {
            return Ok(());
        }
        Err(forbidden())
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
        let address = site.require_collection(&name)?;
        Ok(CollectionScope { address, site })
    }
}
