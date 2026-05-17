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
use crate::{
    Collection, CollectionUpdate, Field, Fieldset, FieldsetUpdate, FieldUpdate, Manifest,
    ManifestUpdate, SchemaStore,
};

/// Name of the fieldset auto-created when a collection is created. The boolean
/// `auto_add` flag — not this name — is what marks a fieldset as "new fields
/// land here"; the name is just a sensible default for the first one.
const DEFAULT_FIELDSET_NAME: &str = "default";

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
    ///
    /// Order is driven by `auto_add` fieldsets on this collection (concatenated
    /// in fieldset-name order, dedup'd). Fields not named in any auto_add set
    /// are appended in alphabetical-by-key fallback order. Unknown names in
    /// a fieldset are silently skipped — that's how cascade-delete drift, and
    /// in-flight half-applied writes, stay safe to read.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        let raw = self.fields_unordered(collection);

        let mut by_name: std::collections::HashMap<String, Arc<Field>> = raw
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
            .collect();

        let mut ordered: Vec<Arc<Field>> = Vec::with_capacity(raw.len());
        let mut seen = std::collections::HashSet::new();

        for fs in self.auto_add_fieldsets(collection) {
            for name in &fs.fields {
                if seen.insert(name.clone()) {
                    if let Some(f) = by_name.remove(name) {
                        ordered.push(f);
                    }
                }
            }
        }

        // Fields not mentioned in any auto_add set — alphabetical fallback by
        // the key the store already gave us (project, version, name).
        for f in raw {
            if !seen.contains(&f.name) {
                ordered.push(f);
            }
        }

        ordered
    }

    /// First field with the given `(collection, name)` across self + direct deps.
    pub fn field(&self, collection: &str, name: &str) -> Option<Arc<Field>> {
        self.dependencies.iter().find_map(|(project_id, version)| {
            self.store
                .fields()
                .get(&Field::to_path(project_id, version, collection, name))
        })
    }

    /// Raw field list (no fieldset ordering applied). Used as the input to
    /// `fields()` and internally by create/delete cascade logic.
    fn fields_unordered(&self, collection: &str) -> Vec<Arc<Field>> {
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

    /// All fieldsets across self + direct deps for a given collection.
    pub fn fieldsets(&self, collection: &str) -> Vec<Arc<Fieldset>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix =
                    format!("{project_id}/versions/{version}/fieldsets/{collection}/");
                self.store
                    .fieldsets()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, fs)| fs)
            })
            .collect()
    }

    /// First fieldset matching `(collection, name)` across self + direct deps.
    pub fn fieldset(&self, collection: &str, name: &str) -> Option<Arc<Fieldset>> {
        self.dependencies.iter().find_map(|(project_id, version)| {
            self.store
                .fieldsets()
                .get(&Fieldset::to_path(project_id, version, collection, name))
        })
    }

    /// Auto-add fieldsets only, scoped to this project+version (deps don't
    /// influence ordering — each project's fields land in its own sets).
    fn auto_add_fieldsets(&self, collection: &str) -> Vec<Arc<Fieldset>> {
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        self.store
            .fieldsets()
            .list(&prefix)
            .into_iter()
            .filter_map(|(_, fs)| if fs.auto_add { Some(fs) } else { None })
            .collect()
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
        let collection_name = input.name.clone();
        let collection = self.store.collections().create(input)?;
        // Eagerly materialize the default fieldset so the file is visible on
        // disk from the start, rather than appearing the first time a field
        // is added. Failures here are intentionally non-fatal — a missing
        // default just falls back to alphabetical ordering until repaired.
        let _ = self.store.fieldsets().create(Fieldset {
            project: self.project_id.clone(),
            version: self.version.clone(),
            collection: collection_name,
            name: DEFAULT_FIELDSET_NAME.to_string(),
            label: String::new(),
            fields: Vec::new(),
            auto_add: true,
        });
        Ok(collection)
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

    /// Deletes the collection AND every field + fieldset belonging to it
    /// (in this version). Field cascade matches the prior handler behavior;
    /// fieldset cascade prevents orphaned ordering metadata.
    pub fn delete_collection(&self, name: &str) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let field_prefix = format!(
            "{}/versions/{}/fields/{}/",
            self.project_id, self.version, name
        );
        let _ = self.store.fields().delete_by_prefix(&field_prefix);
        let fieldset_prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, name
        );
        let _ = self.store.fieldsets().delete_by_prefix(&fieldset_prefix);
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().delete(&key)?)
    }

    pub fn create_field(&self, mut input: Field) -> Result<Arc<Field>, VersionSchemaError> {
        self.require_writable()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        let collection = input.collection.clone();
        let name = input.name.clone();
        let field = self.store.fields().create(input)?;
        self.append_to_auto_add_sets(&collection, &name);
        Ok(field)
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
        self.store.fields().delete(&key)?;
        // Best-effort cascade. The read path already tolerates dangling names
        // by skipping unknowns, so a crash between these two writes leaves
        // the system in a safe (if mildly stale) state.
        self.remove_from_all_fieldsets(collection, name);
        Ok(())
    }

    pub fn create_fieldset(
        &self,
        mut input: Fieldset,
    ) -> Result<Arc<Fieldset>, VersionSchemaError> {
        self.require_writable()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.fieldsets().create(input)?)
    }

    pub fn update_fieldset(
        &self,
        collection: &str,
        name: &str,
        patch: FieldsetUpdate,
    ) -> Result<Arc<Fieldset>, VersionSchemaError> {
        self.require_writable()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().update(&key, patch)?)
    }

    pub fn delete_fieldset(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().delete(&key)?)
    }

    /// Append `field_name` to every `auto_add` fieldset in this project+version
    /// for the given collection. If none exists, lazy-create the default,
    /// seeded with every existing field in this project+version so collections
    /// that predate the feature don't end up with a partial default. The new
    /// `field_name` is appended last regardless.
    fn append_to_auto_add_sets(&self, collection: &str, field_name: &str) {
        let auto_sets = self.auto_add_fieldsets(collection);
        if auto_sets.is_empty() {
            let prefix = format!(
                "{}/versions/{}/fields/{}/",
                self.project_id, self.version, collection
            );
            let mut seed: Vec<String> = self
                .store
                .fields()
                .list(&prefix)
                .into_iter()
                .map(|(_, f)| f.name.clone())
                .filter(|n| n != field_name)
                .collect();
            seed.push(field_name.to_string());
            let _ = self.store.fieldsets().create(Fieldset {
                project: self.project_id.clone(),
                version: self.version.clone(),
                collection: collection.to_string(),
                name: DEFAULT_FIELDSET_NAME.to_string(),
                label: String::new(),
                fields: seed,
                auto_add: true,
            });
            return;
        }
        for fs in auto_sets {
            if fs.fields.iter().any(|n| n == field_name) {
                continue;
            }
            let mut next = fs.fields.clone();
            next.push(field_name.to_string());
            let key = Fieldset::to_path(
                &self.project_id,
                &self.version,
                collection,
                &fs.name,
            );
            let _ = self.store.fieldsets().update(
                &key,
                FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                },
            );
        }
    }

    /// Strip `field_name` from every fieldset in this project+version's view of
    /// the collection. Touches only sets that actually reference the name.
    fn remove_from_all_fieldsets(&self, collection: &str, field_name: &str) {
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        let sets = self.store.fieldsets().list(&prefix);
        for (key, fs) in sets {
            if !fs.fields.iter().any(|n| n == field_name) {
                continue;
            }
            let next: Vec<String> = fs
                .fields
                .iter()
                .filter(|n| *n != field_name)
                .cloned()
                .collect();
            let _ = self.store.fieldsets().update(
                &key,
                FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                },
            );
        }
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
