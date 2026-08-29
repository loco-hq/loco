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
//! The dep set — and the public permission-set assignment that sits beside it
//! on the manifest — is snapshotted at construction so a request gets a
//! coherent view even if a concurrent write mutates the manifest mid-flight.
//!
//! Two construction modes:
//! - [`VersionSchema::new`] — writable. Used by `VersionScope` for `/schema`
//!   writes (developer + draft).
//! - [`VersionSchema::new_read_only`] — read-only. Used by `SiteScope` (data
//!   routes) and `VersionReadScope` (GET `/schema`).

use std::sync::Arc;

use crate::http::authz::is_draft_version;
use crate::{
    Bundle, Collection, CollectionUpdate, Field, FieldUpdate, Fieldset, FieldsetUpdate, Manifest,
    ManifestUpdate, PermissionSet, PermissionSetUpdate, SchemaStore,
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
    /// Self entry plus direct deps as `(project_id, version)` pairs. Self is
    /// always index 0.
    ///
    /// Order **is** significant. Every lookup below (`collection`, `field`,
    /// `fieldset`, `permission_set`) returns the first match walking this
    /// list, so self shadows a dep and an earlier dep shadows a later one —
    /// which also means a dep owning a name no one else uses is reachable by
    /// that bare name. That is not the intended semantic ("Name resolution"
    /// in CLAUDE.md: a bare name means self, deps must be qualified), but it
    /// is the behavior today; issue #28 changes it. Do not write new code
    /// that relies on the fall-through.
    dependencies: Vec<(String, String)>,
    /// Names of the permission sets this version's manifest assigns to
    /// `public`, snapshotted with `dependencies` for the same reason.
    ///
    /// Assignment lives on the manifest, not on the site: a site is a URL
    /// pointing at `(version, dataset)`, so two sites pinning one version
    /// share its public policy. Names resolve through
    /// [`Self::permission_set`], which is what lets a consuming version opt
    /// into a set a dependency ships.
    public_permission_sets: Vec<String>,
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
        let manifest = store
            .manifests()
            .get(&Manifest::to_path(&project_id, &version));
        let dependencies = direct_dependencies(&project_id, &version, manifest.as_deref());
        let public_permission_sets = manifest
            .map(|m| m.public_permission_sets().to_vec())
            .unwrap_or_default();
        Self {
            store,
            project_id,
            version,
            dependencies,
            public_permission_sets,
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

    /// Permission-set names this version's manifest assigns to `public`.
    /// Snapshotted at construction; unknown names stay inert because
    /// [`Self::permission_set`] simply won't resolve them.
    pub fn public_permission_sets(&self) -> &[String] {
        &self.public_permission_sets
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

    /// The collection owned by *this* version's own project, ignoring deps.
    ///
    /// `/data` resolves through this rather than [`Self::collection`] so record
    /// access already follows the rule a bare name means self ("Name
    /// resolution" in CLAUDE.md). A dependency's collection becomes reachable
    /// when #28 adds a way to name it qualified — not by falling through here
    /// in the meantime, which would make dep records addressable under a bare
    /// name and then take that away again.
    pub fn own_collection(&self, name: &str) -> Option<Arc<Collection>> {
        self.store
            .collections()
            .get(&Collection::to_path(&self.project_id, &self.version, name))
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

        let mut by_name: std::collections::HashMap<String, Arc<Field>> =
            raw.iter().map(|f| (f.name.clone(), f.clone())).collect();

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
                let prefix = format!("{project_id}/versions/{version}/fields/{collection}/");
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
                let prefix = format!("{project_id}/versions/{version}/fieldsets/{collection}/");
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

    /// Every permission set visible to this version, across self + direct deps.
    pub fn permission_sets(&self) -> Vec<Arc<PermissionSet>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/permission_sets/");
                self.store
                    .permission_sets()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, ps)| ps)
            })
            .collect()
    }

    /// First permission set with the given `name` across self + direct deps.
    /// Self wins on a name collision so a consumer can shadow a package set.
    pub fn permission_set(&self, name: &str) -> Option<Arc<PermissionSet>> {
        self.dependencies.iter().find_map(|(project_id, version)| {
            self.store
                .permission_sets()
                .get(&PermissionSet::to_path(project_id, version, name))
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

    /// `Ok` only when this view was built writable AND the version is a
    /// draft. Public so a handler can refuse a published version before it
    /// does expensive work on the request body.
    pub fn require_writable(&self) -> Result<(), VersionSchemaError> {
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

    // --- Bundle: the version's own static file tree ---
    //
    // Unlike collections and fields, the bundle is never read through a
    // dependency. A version ships its own frontend; installing a package does
    // not import the package's HTML.

    fn bundle_key(&self) -> String {
        Bundle::to_path(&self.project_id, &self.version)
    }

    /// The bundle tree for this version. `None` when the version has none —
    /// which is every version until something is uploaded.
    pub fn bundle(&self) -> Result<Option<loco_schema_runtime::FileTree>, VersionSchemaError> {
        Ok(self.store.bundles().read_tree(&self.bundle_key())?)
    }

    /// When the current bundle tree was written. `None` when there is none.
    pub fn bundle_uploaded_at(&self) -> Result<Option<std::time::SystemTime>, VersionSchemaError> {
        Ok(self.store.bundles().modified_at(&self.bundle_key())?)
    }

    /// Replace the whole bundle tree. Draft-only, like every other write here.
    pub fn put_bundle(
        &self,
        tree: &loco_schema_runtime::FileTree,
    ) -> Result<Arc<Bundle>, VersionSchemaError> {
        self.require_writable()?;
        Ok(self.store.bundles().put(&self.bundle_key(), tree)?)
    }

    /// Drop the bundle tree. `Error::NotFound` when the version has none.
    pub fn delete_bundle(&self) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        Ok(self.store.bundles().delete(&self.bundle_key())?)
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

    pub fn delete_field(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
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

    pub fn delete_fieldset(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().delete(&key)?)
    }

    pub fn create_permission_set(
        &self,
        mut input: PermissionSet,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        self.require_writable()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.permission_sets().create(input)?)
    }

    pub fn update_permission_set(
        &self,
        name: &str,
        patch: PermissionSetUpdate,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        self.require_writable()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().update(&key, patch)?)
    }

    pub fn delete_permission_set(&self, name: &str) -> Result<(), VersionSchemaError> {
        self.require_writable()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().delete(&key)?)
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
            let key = Fieldset::to_path(&self.project_id, &self.version, collection, &fs.name);
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
    project_id: &str,
    version: &str,
    manifest: Option<&Manifest>,
) -> Vec<(String, String)> {
    let mut deps = vec![(project_id.to_string(), version.to_string())];
    if let Some(manifest) = manifest {
        for child in manifest.dependencies() {
            if let Some((namespace, v)) = child.split_once('@') {
                deps.push((namespace.to_string(), v.to_string()));
            }
        }
    }
    deps
}
