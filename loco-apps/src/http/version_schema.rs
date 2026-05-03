//! Per-version, scoped view over the global `SchemaStore`.
//!
//! A handler that holds a `VersionSchema` can:
//! - **read** any metadata reachable from this `(project, version)`'s manifest
//!   closure (own version + every transitively installed dependency), and
//! - **write** only to its own `(project, version)`.
//!
//! Writes are also gated on the version being a draft. Together this means a
//! handler can never accidentally mutate inherited metadata or a published
//! version — the only mutation surface is `(self.project_id, self.version)`
//! while it carries a `-dev` suffix.
//!
//! The dependency closure is snapshotted at construction so a request gets a
//! coherent view even if a concurrent write mutates a manifest mid-flight.

use std::collections::HashSet;
use std::sync::Arc;

use crate::http::authz::is_draft_version;
use crate::manifest::{resolve_dependency_tree, Dependency, ManifestError};
use crate::{
    Collection, CollectionUpdate, Field, FieldUpdate, Manifest, SchemaStore,
};

#[derive(Debug)]
pub enum VersionSchemaError {
    /// The version is published; writes are refused.
    NotDraft(String),
    Schema(loco_schema_runtime::Error),
}

impl std::fmt::Display for VersionSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotDraft(v) => write!(f, "version {v} is published and read-only"),
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
    /// Iterated in resolve order, so name-collision tiebreaks are deterministic.
    dependencies: Vec<Dependency>,
    dependency_set: HashSet<Dependency>,
}

impl VersionSchema {
    pub fn new(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        let project_id = project_id.into();
        let version = version.into();
        let root = format!("{project_id}@{version}");
        let dependencies = resolve_dependency_tree(&store, &root)?;
        let dependency_set = dependencies.iter().cloned().collect();
        Ok(Self {
            store,
            project_id,
            version,
            dependencies,
            dependency_set,
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dependencies(&self) -> &HashSet<Dependency> {
        &self.dependency_set
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

    /// First collection with the given `name`. Self wins on collision, then
    /// dependencies in manifest-resolve order.
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

    /// First field with the given `(collection, name)`. Self wins on
    /// collision, then dependencies in manifest-resolve order.
    pub fn field(&self, collection: &str, name: &str) -> Option<Arc<Field>> {
        self.dependencies.iter().find_map(|dep| {
            self.store
                .fields()
                .get(&Field::to_path(&dep.project_id, &dep.version, collection, name))
        })
    }

    // --- Writes: scoped to (self.project_id, self.version), draft-only ---

    fn require_draft(&self) -> Result<(), VersionSchemaError> {
        if is_draft_version(&self.version) {
            Ok(())
        } else {
            Err(VersionSchemaError::NotDraft(self.version.clone()))
        }
    }

    pub fn create_collection(
        &self,
        name: String,
        label: String,
        label_plural: String,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        self.require_draft()?;
        let value = Collection::new(
            self.project_id.clone(),
            self.version.clone(),
            name,
            label,
            label_plural,
        );
        Ok(self.store.collections().create(value)?)
    }

    pub fn update_collection(
        &self,
        name: &str,
        patch: CollectionUpdate,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        self.require_draft()?;
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().update(&key, patch)?)
    }

    /// Deletes the collection AND every field belonging to it (in this
    /// version). Field cascade matches the prior handler behavior.
    pub fn delete_collection(&self, name: &str) -> Result<(), VersionSchemaError> {
        self.require_draft()?;
        let field_prefix = format!(
            "{}/versions/{}/fields/{}/",
            self.project_id, self.version, name
        );
        let _ = self.store.fields().delete_by_prefix(&field_prefix);
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().delete(&key)?)
    }

    pub fn create_field(
        &self,
        collection: String,
        name: String,
        ty: String,
    ) -> Result<Arc<Field>, VersionSchemaError> {
        self.require_draft()?;
        let value = Field::new(
            self.project_id.clone(),
            self.version.clone(),
            collection,
            name,
            ty,
        );
        Ok(self.store.fields().create(value)?)
    }

    pub fn update_field(
        &self,
        collection: &str,
        name: &str,
        patch: FieldUpdate,
    ) -> Result<Arc<Field>, VersionSchemaError> {
        self.require_draft()?;
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().update(&key, patch)?)
    }

    pub fn delete_field(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<(), VersionSchemaError> {
        self.require_draft()?;
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().delete(&key)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn copy_dir_all(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_all(&path, &dst_path);
            } else {
                fs::copy(&path, &dst_path).unwrap();
            }
        }
    }

    /// Writes mutate disk via the YAML adapter; each test gets its own
    /// fixture copy so parallel tests don't race on the real instances dir.
    fn isolated_store() -> (TempDir, Arc<SchemaStore>) {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/instances");
        let tmp = TempDir::new().unwrap();
        let dst: PathBuf = tmp.path().join("instances");
        copy_dir_all(&src, &dst);
        let store = Arc::new(SchemaStore::load(&dst).expect("load schema fixtures"));
        (tmp, store)
    }

    /// `loco/studio@0.0.1-dev` has `loco/core@0.0.1-dev` as a transitive dep,
    /// so it's a reliable fixture for both self-only and union assertions.
    fn studio() -> (String, String) {
        ("loco/studio".to_string(), "0.0.1-dev".to_string())
    }

    #[test]
    fn dependencies_include_self_and_transitive() {
        let (_tmp, store) = isolated_store();
        let (p, v) = studio();
        let schema = VersionSchema::new(store, &p, &v).unwrap();
        let deps = schema.dependencies();
        assert!(deps.contains(&Dependency::new("loco/studio", "0.0.1-dev")));
        assert!(deps.contains(&Dependency::new("loco/core", "0.0.1-dev")));
    }

    #[test]
    fn collections_union_spans_dependencies() {
        let (_tmp, store) = isolated_store();
        let (p, v) = studio();
        let schema = VersionSchema::new(store.clone(), &p, &v).unwrap();
        // Studio itself doesn't ship collections, but it inherits from core.
        let core_prefix = "loco/core/versions/0.0.1-dev/collections/";
        let core_count = store.collections().list(core_prefix).len();
        assert!(core_count > 0, "fixture must include core collections");
        assert!(schema.collections().len() >= core_count);
    }

    #[test]
    fn manifest_resolves_for_version() {
        let (_tmp, store) = isolated_store();
        let (p, v) = studio();
        let schema = VersionSchema::new(store, &p, &v).unwrap();
        let m = schema.manifest().expect("manifest must exist");
        assert_eq!(m.project(), p);
        assert_eq!(m.version(), v);
    }

    #[test]
    fn write_then_read_round_trips() {
        let (_tmp, store) = isolated_store();
        let (p, v) = studio();
        let schema = VersionSchema::new(store, &p, &v).unwrap();

        let created = schema
            .create_collection(
                "vs_round_trip".to_string(),
                "RoundTrip".to_string(),
                "RoundTrips".to_string(),
            )
            .expect("create");
        assert_eq!(created.name(), "vs_round_trip");

        let found = schema.collection("vs_round_trip").expect("read back");
        assert_eq!(found.name(), "vs_round_trip");
    }

    #[test]
    fn delete_collection_cascades_fields() {
        let (_tmp, store) = isolated_store();
        let (p, v) = studio();
        let schema = VersionSchema::new(store, &p, &v).unwrap();

        schema
            .create_collection(
                "vs_cascade".to_string(),
                "C".to_string(),
                "Cs".to_string(),
            )
            .unwrap();
        schema
            .create_field(
                "vs_cascade".to_string(),
                "leaf".to_string(),
                "string".to_string(),
            )
            .unwrap();
        assert!(schema.field("vs_cascade", "leaf").is_some());

        schema.delete_collection("vs_cascade").unwrap();
        assert!(schema.collection("vs_cascade").is_none());
        assert!(schema.field("vs_cascade", "leaf").is_none());
    }

    #[test]
    fn writes_refused_on_published_version() {
        let (_tmp, store) = isolated_store();
        // A published-looking version (no `-dev`). It need not exist in the
        // store — the draft check runs before any store lookup.
        let schema = VersionSchema::new(store, "loco/studio", "1.0.0").unwrap();
        let res = schema.create_collection(
            "x".to_string(),
            "X".to_string(),
            "Xs".to_string(),
        );
        assert!(matches!(res, Err(VersionSchemaError::NotDraft(_))));
    }
}
