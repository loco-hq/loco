//! Per-version, scoped view over the global `SchemaStore`.
//!
//! A `VersionSchema` is built around a `(project_id, version)` pair plus the
//! transitive dependency closure declared in that version's manifest. It can:
//!
//! - **read** any metadata reachable from the closure (own version + every
//!   transitively installed dependency), and
//! - **write** to its own `(project_id, version)` *only* when constructed
//!   writable AND the version is a draft (`-dev` suffix).
//!
//! The closure is snapshotted at construction so a request gets a coherent
//! view even if a concurrent write mutates a manifest mid-flight.
//!
//! Two construction modes:
//! - [`VersionSchema::new`] — writable. Used by `VersionScope` for /schema
//!   routes (gated to metadata-editor sites).
//! - [`VersionSchema::new_read_only`] — read-only. Used by `SiteScope` so
//!   data and auth routes can resolve the same metadata view without ever
//!   being able to mutate it.

use std::collections::HashSet;
use std::sync::Arc;

use crate::http::authz::is_draft_version;
use crate::manifest::{resolve_dependency_tree, Dependency, ManifestError};
use crate::{Collection, CollectionUpdate, Field, FieldUpdate, Manifest, SchemaStore};

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
    /// Manifest closure for `(project_id, version)`, including the self entry.
    /// Order is not significant — collections and fields will be fully
    /// qualified, so reads can't collide across deps.
    dependencies: HashSet<Dependency>,
    read_only: bool,
}

impl VersionSchema {
    /// Build a writable scoped view. Writes are still gated on the version
    /// being a draft (`-dev` suffix); see [`Self::require_writable`].
    pub fn new(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        Self::build(store, project_id, version, false)
    }

    /// Build a read-only scoped view. All write methods will return
    /// `VersionSchemaError::NotWritable` regardless of draft state.
    pub fn new_read_only(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        Self::build(store, project_id, version, true)
    }

    fn build(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
        read_only: bool,
    ) -> Result<Self, ManifestError> {
        let project_id = project_id.into();
        let version = version.into();
        let root = format!("{project_id}@{version}");
        let dependencies: HashSet<Dependency> =
            resolve_dependency_tree(&store, &root)?.into_iter().collect();
        Ok(Self {
            store,
            project_id,
            version,
            dependencies,
            read_only,
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dependencies(&self) -> &HashSet<Dependency> {
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

    /// Every collection visible to this version, across self + installed deps.
    pub fn collections(&self) -> Vec<Arc<Collection>> {
        self.dependencies
            .iter()
            .flat_map(|dep| {
                let prefix = format!("{}/versions/{}/collections/", dep.project_id, dep.version);
                self.store
                    .collections()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, c)| c)
            })
            .collect()
    }

    /// First collection with the given `name` across the closure.
    pub fn collection(&self, name: &str) -> Option<Arc<Collection>> {
        self.dependencies.iter().find_map(|dep| {
            self.store
                .collections()
                .get(&Collection::to_path(&dep.project_id, &dep.version, name))
        })
    }

    /// Every field across self + installed deps that targets the given
    /// collection name. Deps may declare fields that extend a collection
    /// owned by another dep; those extensions are picked up here.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        self.dependencies
            .iter()
            .flat_map(|dep| {
                let prefix = format!(
                    "{}/versions/{}/fields/{}/",
                    dep.project_id, dep.version, collection
                );
                self.store
                    .fields()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, f)| f)
            })
            .collect()
    }

    /// First field with the given `(collection, name)` across the closure.
    pub fn field(&self, collection: &str, name: &str) -> Option<Arc<Field>> {
        self.dependencies.iter().find_map(|dep| {
            self.store
                .fields()
                .get(&Field::to_path(&dep.project_id, &dep.version, collection, name))
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
