//! Per-site, scoped view over the global `SchemaStore`.
//!
//! A handler that holds a `SiteSchema` can only see metadata from its own
//! `(project, version)` plus the closure of installed dependencies declared
//! in its `manifest.yaml`. Any lookup for a `Dependency` outside that closure
//! returns `None` (or an empty list) — the same shape as "doesn't exist".
//!
//! The dependency closure is snapshotted at construction and never re-read,
//! so a request gets a coherent view even if a concurrent write mutates a
//! manifest mid-flight.

use std::collections::HashSet;
use std::sync::Arc;

use crate::manifest::{resolve_dependency_tree, Dependency, ManifestError};
use crate::{Collection, Dataset, Field, Manifest, Project, SchemaStore, Site};

pub struct SiteSchema {
    store: Arc<SchemaStore>,
    project_id: String,
    version: String,
    dependencies: HashSet<Dependency>,
}

impl SiteSchema {
    /// Build a scoped view by walking the manifest closure rooted at
    /// `(project_id, version)`. The site's own dependency entry is always
    /// included as the first reachable node.
    pub fn new(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
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

    /// The site's own (project_id, version) — convenience for callers that
    /// need a `&Dependency` for the by-namespace APIs without rebuilding it.
    pub fn self_dep(&self) -> Dependency {
        Dependency::new(self.project_id.clone(), self.version.clone())
    }

    // --- Configs (site's own project only; configs are not versioned) ---

    pub fn project(&self) -> Option<Arc<Project>> {
        self.store.projects().get(&Project::to_path(&self.project_id))
    }

    pub fn dataset(&self, name: &str) -> Option<Arc<Dataset>> {
        self.store
            .datasets()
            .get(&Dataset::to_path(&self.project_id, name))
    }

    pub fn site(&self, name: &str) -> Option<Arc<Site>> {
        self.store
            .sites()
            .get(&Site::to_path(&self.project_id, name))
    }

    // --- Manifest (site's own only) ---

    pub fn manifest(&self) -> Option<Arc<Manifest>> {
        self.store
            .manifests()
            .get(&Manifest::to_path(&self.project_id, &self.version))
    }

    // --- Collections: uniform across every installed dependency ---

    /// Every collection visible to this site, across all installed
    /// dependencies (including the site's own project+version).
    pub fn collections(&self) -> Vec<Arc<Collection>> {
        self.dependencies
            .iter()
            .flat_map(|dep| self.collections_in(dep))
            .collect()
    }

    /// Every collection declared by `dep`. Empty if `dep` is not installed.
    pub fn collections_in(&self, dep: &Dependency) -> Vec<Arc<Collection>> {
        if !self.dependencies.contains(dep) {
            return Vec::new();
        }
        let prefix = format!("{}/versions/{}/collections/", dep.project_id, dep.version);
        self.store
            .collections()
            .list(&prefix)
            .into_iter()
            .map(|(_, c)| c)
            .collect()
    }

    /// A specific collection. `None` if `dep` is not installed or the
    /// collection doesn't exist.
    pub fn collection(&self, dep: &Dependency, name: &str) -> Option<Arc<Collection>> {
        if !self.dependencies.contains(dep) {
            return None;
        }
        self.store
            .collections()
            .get(&Collection::to_path(&dep.project_id, &dep.version, name))
    }

    // --- Fields: same three shapes, always scoped under a collection ---

    /// Every field across every installed dependency that targets a
    /// collection of the given `name`. This is the union view used by the
    /// validator: deps may declare fields that "extend" a collection owned
    /// by another dep, and those extensions are picked up here.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        self.dependencies
            .iter()
            .flat_map(|dep| self.fields_in(dep, collection))
            .collect()
    }

    /// Fields owned by `dep` that target the given collection name.
    /// Empty if `dep` is not installed.
    pub fn fields_in(&self, dep: &Dependency, collection: &str) -> Vec<Arc<Field>> {
        if !self.dependencies.contains(dep) {
            return Vec::new();
        }
        let prefix = format!(
            "{}/versions/{}/fields/{}/",
            dep.project_id, dep.version, collection
        );
        self.store
            .fields()
            .list(&prefix)
            .into_iter()
            .map(|(_, f)| f)
            .collect()
    }

    /// A specific field. `None` if `dep` is not installed or the field
    /// doesn't exist.
    pub fn field(&self, dep: &Dependency, collection: &str, name: &str) -> Option<Arc<Field>> {
        if !self.dependencies.contains(dep) {
            return None;
        }
        self.store
            .fields()
            .get(&Field::to_path(&dep.project_id, &dep.version, collection, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load_store() -> Arc<SchemaStore> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/instances");
        Arc::new(SchemaStore::load(&dir).expect("load schema fixtures"))
    }

    /// Find any `(project_id, version)` from the fixtures that has a
    /// manifest — that's the only triple we can build a `SiteSchema` for.
    fn pick_site(store: &SchemaStore) -> (String, String) {
        let (_, m) = store
            .manifests()
            .list_all()
            .into_iter()
            .next()
            .expect("fixtures must include at least one manifest");
        (m.project().to_string(), m.version().to_string())
    }

    #[test]
    fn self_dep_is_in_dependencies() {
        let store = load_store();
        let (project, version) = pick_site(&store);
        let schema = SiteSchema::new(store, &project, &version).unwrap();
        assert!(schema.dependencies().contains(&schema.self_dep()));
    }

    #[test]
    fn collections_in_self_dep_match_collections_for_site() {
        let store = load_store();
        let (project, version) = pick_site(&store);
        let schema = SiteSchema::new(store, &project, &version).unwrap();
        let from_self = schema.collections_in(&schema.self_dep());
        // Should be a subset of the union; equal if no other deps contribute.
        assert!(schema.collections().len() >= from_self.len());
    }

    #[test]
    fn out_of_scope_dep_returns_none_or_empty() {
        let store = load_store();
        let (project, version) = pick_site(&store);
        let schema = SiteSchema::new(store, &project, &version).unwrap();
        let nonexistent = Dependency::new("nobody/nothing", "0.0.0");
        assert!(schema.collection(&nonexistent, "anything").is_none());
        assert!(schema.collections_in(&nonexistent).is_empty());
        assert!(schema.field(&nonexistent, "x", "y").is_none());
        assert!(schema.fields_in(&nonexistent, "x").is_empty());
    }

    #[test]
    fn manifest_resolves_for_site() {
        let store = load_store();
        let (project, version) = pick_site(&store);
        let schema = SiteSchema::new(store, &project, &version).unwrap();
        let m = schema.manifest().expect("site must have a manifest");
        assert_eq!(m.project(), project);
        assert_eq!(m.version(), version);
    }
}
