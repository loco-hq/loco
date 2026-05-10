//! Per-version, scoped view over the global `SchemaStore`.
//!
//! A `VersionSchema` is built around a `(project_id, version)` pair plus the
//! direct dependencies declared in that version's manifest. It can:
//!
//! - **read** any metadata in its own version OR a directly-declared
//!   dependency. Transitive deps are not visible — to use a piece of
//!   metadata, the project must depend on its owner directly.
//! - **write** to its own `(project_id, version)` *only* when constructed
//!   writable AND the version is a draft (`-dev` suffix).
//!
//! The dep set is snapshotted at construction so a request gets a coherent
//! view even if a concurrent write mutates the manifest mid-flight.
//!
//! Two construction modes:
//! - [`VersionSchema::new`] — writable. Used by `VersionScope` for /schema
//!   routes (gated to metadata-editor sites).
//! - [`VersionSchema::new_read_only`] — read-only. Used by `SiteScope` so
//!   data and auth routes can resolve the same metadata view without ever
//!   being able to mutate it.

use std::sync::Arc;

use crate::http::authz::is_draft_version;
use crate::{Collection, CollectionUpdate, Field, FieldUpdate, Manifest, ManifestUpdate, SchemaStore};

#[derive(Debug)]
pub enum VersionSchemaError {
    /// Writes refused because this schema was constructed read-only OR the
    /// version is published. The message distinguishes the two cases.
    NotWritable(String),
    Schema(loco_schema_runtime::Error),
}

impl std::fmt::Display for VersionSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable(msg) => write!(f, "{msg}"),
            Self::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VersionSchemaError {}

impl From<loco_schema_runtime::Error> for VersionSchemaError {
    fn from(e: loco_schema_runtime::Error) -> Self {
        Self::Schema(e)
    }
}

pub struct VersionSchema {
    store: Arc<SchemaStore>,
    project_id: String,
    version: String,
    /// Self entry plus direct deps as `(project_id, version)` pairs.
    /// Order is not significant — collections and fields are fully qualified,
    /// so reads can't collide across deps.
    dependencies: Vec<(String, String)>,
    read_only: bool,
}

impl VersionSchema {
    /// Build a writable scoped view. Writes are still gated on the version
    /// being a draft (`-dev` suffix); see [`Self::require_writable`].
    pub fn new(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self::build(store, project_id, version, false)
    }

    /// Build a read-only scoped view. All write methods will return
    /// `VersionSchemaError::NotWritable` regardless of draft state.
    pub fn new_read_only(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self::build(store, project_id, version, true)
    }

    fn build(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
        read_only: bool,
    ) -> Self {
        let project_id = project_id.into();
        let version = version.into();
        let dependencies = direct_dependencies(&store, &project_id, &version);
        Self {
            store,
            project_id,
            version,
            dependencies,
            read_only,
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dependencies(&self) -> &[(String, String)] {
        &self.dependencies
    }

    // --- Reads: union across self + every installed dependency ---

    /// Manifest for `(self.project_id, self.version)`. Manifests are
    /// inherently per-version; there's no cross-namespace flavor.
    pub fn manifest(&self) -> Option<Arc<Manifest>> {
        self.store
            .manifests()
            .get(&Manifest::to_path(&self.project_id, &self.version))
    }

    /// Every collection visible to this version, across self + direct deps.
    pub fn collections(&self) -> Vec<Arc<Collection>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/collections/");
                self.store
                    .collections()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, c)| c)
            })
            .collect()
    }

    /// First collection with the given `name` across self + direct deps.
    pub fn collection(&self, name: &str) -> Option<Arc<Collection>> {
        self.dependencies.iter().find_map(|(project_id, version)| {
            self.store
                .collections()
                .get(&Collection::to_path(project_id, version, name))
        })
    }

    /// Every field across self + direct deps that targets the given
    /// collection name. Deps may declare fields that extend a collection
    /// owned by another dep; those extensions are picked up here.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix =
                    format!("{project_id}/versions/{version}/fields/{collection}/");
                self.store
                    .fields()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, f)| f)
            })
            .collect()
    }

    /// First field with the given `(collection, name)` across self + direct deps.
    pub fn field(&self, collection: &str, name: &str) -> Option<Arc<Field>> {
        self.dependencies.iter().find_map(|(project_id, version)| {
            self.store
                .fields()
                .get(&Field::to_path(project_id, version, collection, name))
        })
    }

    // --- Writes: scoped to (self.project_id, self.version), draft-only ---

    fn require_writable(&self) -> Result<(), VersionSchemaError> {
        if self.read_only {
            return Err(VersionSchemaError::NotWritable(format!(
                "schema for {} is read-only in this scope",
                self.project_id
            )));
        }
        if !is_draft_version(&self.version) {
            return Err(VersionSchemaError::NotWritable(format!(
                "version {} is published and read-only",
                self.version
            )));
        }
        Ok(())
    }

    pub fn update_manifest(
        &self,
        patch: ManifestUpdate,
    ) -> Result<Arc<Manifest>, VersionSchemaError> {
        self.require_writable()?;
        let key = Manifest::to_path(&self.project_id, &self.version);
        Ok(self.store.manifests().update(&key, patch)?)
    }

    pub fn create_collection(
        &self,
        mut input: Collection,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        self.require_writable()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.collections().create(input)?)
    }

    pub fn update_collection(
        &self,
        name: &str,
        patch: CollectionUpdate,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        self.require_writable()?;
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().update(&key, patch)?)
    }

    /// Deletes the collection AND every field belonging to it (in this
    /// version). Field cascade matches the prior handler behavior.
    pub fn delete_collection(&self, name: &str) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let field_prefix = format!(
            "{}/versions/{}/fields/{}/",
            self.project_id, self.version, name
        );
        let _ = self.store.fields().delete_by_prefix(&field_prefix);
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().delete(&key)?)
    }

    pub fn create_field(&self, mut input: Field) -> Result<Arc<Field>, VersionSchemaError> {
        self.require_writable()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.fields().create(input)?)
    }

    pub fn update_field(
        &self,
        collection: &str,
        name: &str,
        patch: FieldUpdate,
    ) -> Result<Arc<Field>, VersionSchemaError> {
        self.require_writable()?;
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().update(&key, patch)?)
    }

    pub fn delete_field(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().delete(&key)?)
    }
}

/// Self entry plus direct deps from the manifest, as `(project_id, version)`
/// pairs. Dep strings are `user/project@version`; malformed entries are
/// skipped — they shouldn't reach the store once write-time validation lands.
fn direct_dependencies(
    store: &SchemaStore,
    project_id: &str,
    version: &str,
) -> Vec<(String, String)> {
    let mut deps = vec![(project_id.to_string(), version.to_string())];
    if let Some(manifest) = store
        .manifests()
        .get(&Manifest::to_path(project_id, version))
    {
        for child in manifest.dependencies() {
            if let Some((namespace, v)) = child.split_once('@') {
                deps.push((namespace.to_string(), v.to_string()));
            }
        }
    }
    deps
}
