//! Per-version, scoped view over the global `SchemaStore`.
//!
//! A `VersionSchema` is built around a `(project_id, version)` pair plus the
//! direct dependencies declared in that version's manifest. It can:
//!
//! - **read** any metadata in its own version OR a directly-declared
//!   dependency. Transitive deps are not visible — to use a piece of
//!   metadata, the project must depend on its owner directly. A bare name
//!   means this version's own project; a dependency's must be written
//!   `{account}/{project}.{name}` ("Name resolution" in CLAUDE.md).
//! - **write** to its own `(project_id, version)` *only* when constructed
//!   writable AND the version is a draft (`-dev` suffix) AND it exists — has
//!   a manifest, which only `/config` creates. Each write holds `PINS` from
//!   that check to its last store write, so a version delete cannot slip in
//!   between and leave the write's files behind as an orphan tree.
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

use std::collections::HashSet;
use std::sync::{Arc, MutexGuard};

use crate::http::authz::is_draft_version;
use crate::http::project_config::lock_pins;
use crate::validation::FIELD_TYPES;
use crate::{
    Action, ActionParam, ActionUpdate, Bundle, Collection, CollectionUpdate, Field, FieldUpdate,
    Fieldset, FieldsetUpdate, Manifest, ManifestUpdate, PermissionSet, PermissionSetUpdate,
    SchemaStore, Secret, SecretUpdate, Variable, VariableUpdate,
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
    /// A manifest `dependencies` entry is malformed, names this project,
    /// repeats a project, or names a version that does not exist.
    InvalidDependency(String),
    /// A field `type` outside [`crate::validation::FIELD_TYPES`].
    InvalidFieldType(String),
    /// A secret or variable declaration the version cannot store: a secret
    /// body carrying a value, or a secret and a variable sharing a name.
    InvalidDeclaration(String),
    /// A collection name outside the slug charset. `$` is reserved for the
    /// lake collections that hold secret and variable values.
    InvalidName(String),
    /// The version has no manifest: it was never created through `/config`,
    /// or it has since been deleted.
    UnknownVersion(String),
    /// `delete_collection` attempted its child prefixes.
    /// These keys are still present. The parent record was left in place, so
    /// a recreate cannot inherit a leftover child and the delete can be
    /// retried.
    LeftBehind(Vec<String>),
    Schema(loco_schema_runtime::Error),
}

impl std::fmt::Display for VersionSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable(msg)
            | Self::InvalidDependency(msg)
            | Self::InvalidFieldType(msg)
            | Self::InvalidDeclaration(msg)
            | Self::InvalidName(msg)
            | Self::UnknownVersion(msg) => write!(f, "{msg}"),
            Self::LeftBehind(keys) => write!(f, "delete left behind: {}", keys.join(", ")),
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

#[derive(Clone)]
pub struct VersionSchema {
    store: Arc<SchemaStore>,
    project_id: String,
    version: String,
    /// Self entry plus direct deps as `(project_id, version)` pairs. Self is
    /// always index 0. Lookups find a project here by name; none walks the
    /// list for a match, so its order carries no meaning.
    dependencies: Vec<(String, String)>,
    /// Names of the permission sets this version's manifest assigns to
    /// `public`, snapshotted with `dependencies` for the same reason.
    ///
    /// Assignment lives on the manifest, not on the site: a site is a URL
    /// pointing at `(version, dataset)`, so two sites pinning one version
    /// share its public policy. Names resolve through
    /// [`Self::permission_set`]: a bare name is this project's own set, and a
    /// consuming version opts into a set a dependency ships by naming it
    /// qualified (`acme/crm.public_contacts`).
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

    // --- Reads: self + direct dependencies, resolved strictly ---
    //
    // Every lookup below takes a name as a client writes it and resolves it
    // by the rule in CLAUDE.md ("Name resolution"): a bare name means this
    // version's own project, and a dependency's must be written
    // `{account}/{project}.{name}`. Nothing walks the dependency list looking
    // for a match, so installing a dependency never changes what a bare name
    // means, and two dependencies that share a name are both addressable.

    /// Manifest for `(self.project_id, self.version)`. Manifests are
    /// inherently per-version; there's no cross-namespace flavor.
    pub fn manifest(&self) -> Option<Arc<Manifest>> {
        self.store
            .manifests()
            .get(&Manifest::to_path(&self.project_id, &self.version))
    }

    /// The version of `project` this view sees: the running version when
    /// `project` is self, the pinned version when it is a **direct**
    /// dependency, otherwise `None`.
    pub fn visible_version(&self, project: &str) -> Option<&str> {
        self.dependencies
            .iter()
            .find(|(p, _)| p == project)
            .map(|(_, v)| v.as_str())
    }

    /// `name` as `(owning project, bare name)`: this project for a bare name,
    /// the named project for a qualified one. The project need not be visible
    /// — see [`Self::resolve`] for that.
    pub fn split<'a>(&'a self, name: &'a str) -> (&'a str, &'a str) {
        split_qualified(name).unwrap_or((&self.project_id, name))
    }

    /// `name` as `(project, version, bare name)` when its project is self or
    /// a direct dependency; `None` otherwise (a transitive or unknown
    /// project is not found, never an error of another kind).
    fn resolve<'a>(&'a self, name: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
        let (project, bare) = self.split(name);
        let version = self.visible_version(project)?;
        Some((project, version, bare))
    }

    /// How a client names `name` owned by `project` from this version: bare
    /// for this project's own, qualified for a dependency's.
    pub fn reference(&self, project: &str, name: &str) -> String {
        if project == self.project_id {
            name.to_string()
        } else {
            format!("{project}.{name}")
        }
    }

    /// Every collection visible to this version, across self + direct deps.
    /// Each carries its `project`, from which a client builds the qualified
    /// name of a dependency's.
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

    /// The collection `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn collection(&self, name: &str) -> Option<Arc<Collection>> {
        let (project, bare) = self.split(name);
        self.collection_in(project, bare)
    }

    /// The collection `name` owned by `project`, when `project` is self or a
    /// direct dependency.
    pub fn collection_in(&self, project: &str, name: &str) -> Option<Arc<Collection>> {
        // `$secrets` and `$variables` are lake collections, not schema. A
        // bare name containing `$` does not resolve, even if a file was
        // written under it. `/data` and `/data/query` both come through here.
        if name.contains('$') {
            return None;
        }
        let version = self.visible_version(project)?;
        self.store
            .collections()
            .get(&Collection::to_path(project, version, name))
    }

    /// The field `name` that `project` declares on the collection
    /// `collection` owned by `owner`. A collection's fields are its owner's:
    /// `None` unless `project` is `owner` and is self or a direct dependency.
    ///
    /// Nobody else's declarations under the same collection name apply —
    /// not another dependency's, which would let installing it change what
    /// `owner`'s collection accepts, and not the running project's, which
    /// would make its own `contacts` fields leak onto every dependency's
    /// `contacts`. Whether a project may extend a dependency's collection is
    /// open (docs/query.md, open question 1).
    pub fn field_in(
        &self,
        owner: &str,
        collection: &str,
        project: &str,
        name: &str,
    ) -> Option<Arc<Field>> {
        if project != owner {
            return None;
        }
        let version = self.visible_version(owner)?;
        self.store
            .fields()
            .get(&Field::to_path(owner, version, collection, name))
    }

    /// The fields of the collection `collection` names (bare or qualified),
    /// which are the ones its owner declares ([`Self::field_in`]). Empty when
    /// the name's project is not visible.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        let (owner, bare) = self.split(collection);
        self.fields_of(owner, bare)
    }

    /// [`Self::fields`] for the collection `collection` owned by `owner`.
    ///
    /// Order is driven by the owner's `auto_add` fieldsets on this collection
    /// (concatenated in fieldset-name order, dedup'd). Fields not named in
    /// any auto_add set are appended in alphabetical-by-key fallback order.
    /// Unknown names in a fieldset are silently skipped — that's how
    /// cascade-delete drift, and in-flight half-applied writes, stay safe to
    /// read.
    pub fn fields_of(&self, owner: &str, collection: &str) -> Vec<Arc<Field>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/fields/{collection}/");
        let raw: Vec<Arc<Field>> = self
            .store
            .fields()
            .list(&prefix)
            .into_iter()
            .map(|(_, f)| f)
            .collect();

        let mut by_name: std::collections::HashMap<String, Arc<Field>> =
            raw.iter().map(|f| (f.name.clone(), f.clone())).collect();

        let mut ordered: Vec<Arc<Field>> = Vec::with_capacity(raw.len());
        let mut seen = std::collections::HashSet::new();

        for fs in auto_add_fieldsets(&self.store, owner, version, collection) {
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

    /// The fieldsets on the collection `collection` names (bare or
    /// qualified) — its owner's, like its fields.
    pub fn fieldsets(&self, collection: &str) -> Vec<Arc<Fieldset>> {
        let Some((owner, version, bare)) = self.resolve(collection) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/fieldsets/{bare}/");
        self.store
            .fieldsets()
            .list(&prefix)
            .into_iter()
            .map(|(_, fs)| fs)
            .collect()
    }

    /// The fieldset `name` on the collection `collection` names. Both follow
    /// the rule, so a dependency's set on its own collection is
    /// `acme/crm.contacts` + `acme/crm.summary`; a bare set name means this
    /// project's, which only its own collections have.
    pub fn fieldset(&self, collection: &str, name: &str) -> Option<Arc<Fieldset>> {
        let (owner, version, bare_collection) = self.resolve(collection)?;
        let (project, bare) = self.split(name);
        if project != owner {
            return None;
        }
        self.store
            .fieldsets()
            .get(&Fieldset::to_path(owner, version, bare_collection, bare))
    }

    /// Every permission set visible to this version, across self + direct
    /// deps. Each carries its `project`, like [`Self::collections`].
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

    /// The permission set `name` names: this project's for a bare name, a
    /// direct dependency's for `{account}/{project}.{name}`. A consumer's own
    /// set of the same name as a dependency's is simply a different set.
    pub fn permission_set(&self, name: &str) -> Option<Arc<PermissionSet>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .permission_sets()
            .get(&PermissionSet::to_path(project, version, bare))
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

    /// Whether this version exists — has a manifest. A version is created
    /// only through `/config`, which writes its manifest, so a version name
    /// that has one was checked there (#95).
    pub fn exists(&self) -> bool {
        self.store
            .manifests()
            .has(&Manifest::to_path(&self.project_id, &self.version))
    }

    /// The gate every write goes through: [`Self::require_writable`], then
    /// `PINS`, then [`Self::exists`]. The caller holds the returned guard
    /// until its last store write.
    ///
    /// `VersionScope` already refused a missing version, but that was before
    /// the request body was read. A version delete cascades under `PINS`, so
    /// a write racing it either holds the lock first, finishes, and is swept
    /// by the cascade, or waits and then finds no manifest here. It can never
    /// land after the cascade and leave an orphan tree.
    ///
    /// The cost is that `/schema` writes, bundle uploads included, serialize
    /// with each other and with `/config` pin changes. All are rare
    /// developer operations; a per-version lock would be finer but would
    /// need the delete to take it too, for no case we have.
    fn write_guard(&self) -> Result<MutexGuard<'static, ()>, VersionSchemaError> {
        self.require_writable()?;
        let pins = lock_pins();
        if !self.exists() {
            return Err(VersionSchemaError::UnknownVersion(unknown_version(
                &self.project_id,
                &self.version,
            )));
        }
        Ok(pins)
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
        let _pins = self.write_guard()?;
        Ok(self.store.bundles().put(&self.bundle_key(), tree)?)
    }

    /// Drop the bundle tree. `Error::NotFound` when the version has none.
    pub fn delete_bundle(&self) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        Ok(self.store.bundles().delete(&self.bundle_key())?)
    }

    /// Update this version's manifest. A dependency the stored manifest does
    /// not already name must be one `may_read` allows: the caller may only
    /// declare a project it can read (#86). One it already names was allowed
    /// when it was written, so re-sending it — as a client that PUTs the
    /// whole list does — declares nothing new, and a teammate without access
    /// to it can still edit the rest of the manifest.
    pub fn update_manifest(
        &self,
        patch: ManifestUpdate,
        may_read: impl Fn(&str) -> bool,
    ) -> Result<Arc<Manifest>, VersionSchemaError> {
        // Under `PINS`, like a site pin: the check reads other versions'
        // manifests and a version delete checks this one, so without it a
        // delete could slip between the check and the write.
        let _pins = self.write_guard()?;
        let key = Manifest::to_path(&self.project_id, &self.version);
        if let Some(deps) = &patch.dependencies {
            let current = self.store.manifests().get(&key);
            let declared = |dep: &str| {
                current
                    .as_deref()
                    .is_some_and(|m| m.dependencies().iter().any(|d| d == dep))
            };
            check_dependencies(&self.store, &self.project_id, deps, |dep, project| {
                declared(dep) || may_read(project)
            })
            .map_err(VersionSchemaError::InvalidDependency)?;
        }
        Ok(self.store.manifests().update(&key, patch)?)
    }

    pub fn create_collection(
        &self,
        mut input: Collection,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        if !collection_name_ok(&input.name) {
            return Err(VersionSchemaError::InvalidName(format!(
                "collection name {:?} must be 1 or more of a-z, 0-9, '_', '.', and '-'; \
                 '$' is reserved",
                input.name
            )));
        }
        let _pins = self.write_guard()?;
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
        let _pins = self.write_guard()?;
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().update(&key, patch)?)
    }

    /// Deletes the collection and every field and fieldset belonging to it
    /// in this version.
    ///
    /// Both prefixes are attempted even when the first fails, and the error
    /// names every key that could not be removed. A leftover fieldset is the
    /// worst of those: it is the ordering a collection recreated at this name
    /// would inherit, because creating the collection again ignores a default
    /// fieldset that is already there. The collection record stays until both
    /// prefixes are gone, so that cannot happen and the delete can be retried.
    pub fn delete_collection(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let field_prefix = format!(
            "{}/versions/{}/fields/{}/",
            self.project_id, self.version, name
        );
        let fieldset_prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, name
        );
        let mut left = Vec::new();
        if self.store.fields().delete_by_prefix(&field_prefix).is_err() {
            left.extend(
                self.store
                    .fields()
                    .list(&field_prefix)
                    .into_iter()
                    .map(|(key, _)| key),
            );
        }
        if self
            .store
            .fieldsets()
            .delete_by_prefix(&fieldset_prefix)
            .is_err()
        {
            left.extend(
                self.store
                    .fieldsets()
                    .list(&fieldset_prefix)
                    .into_iter()
                    .map(|(key, _)| key),
            );
        }
        let key = Collection::to_path(&self.project_id, &self.version, name);
        if left.is_empty() {
            match self.store.collections().delete(&key) {
                Ok(()) | Err(loco_schema_runtime::Error::NotFound(_)) => {}
                Err(_) => left.push(key),
            }
        }
        if left.is_empty() {
            Ok(())
        } else {
            left.sort();
            left.dedup();
            Err(VersionSchemaError::LeftBehind(left))
        }
    }

    pub fn create_field(&self, mut input: Field) -> Result<Arc<Field>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        check_field_type(&input.r#type)?;
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
        let _pins = self.write_guard()?;
        if let Some(ty) = &patch.r#type {
            check_field_type(ty)?;
        }
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().update(&key, patch)?)
    }

    pub fn delete_field(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
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
        let _pins = self.write_guard()?;
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
        let _pins = self.write_guard()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().update(&key, patch)?)
    }

    pub fn delete_fieldset(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().delete(&key)?)
    }

    pub fn create_permission_set(
        &self,
        mut input: PermissionSet,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.permission_sets().create(input)?)
    }

    pub fn update_permission_set(
        &self,
        name: &str,
        patch: PermissionSetUpdate,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().update(&key, patch)?)
    }

    pub fn delete_permission_set(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().delete(&key)?)
    }

    /// Every secret declaration visible to this version, across self + direct
    /// deps. Each carries its `project`, like [`Self::collections`].
    pub fn secrets(&self) -> Vec<Arc<Secret>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/secrets/");
                self.store
                    .secrets()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, secret)| secret)
            })
            .collect()
    }

    /// The secret `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn secret(&self, name: &str) -> Option<Arc<Secret>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .secrets()
            .get(&Secret::to_path(project, version, bare))
    }

    /// Secrets `owner` declares on the version this view sees.
    ///
    /// `owner` is a project id (`alice/pkg`). An owner that is not self or a
    /// direct dependency yields an empty list. This does not go through
    /// [`Self::split`]: the running project's secret of the same bare name is
    /// absent unless `owner` is that project.
    pub fn secrets_of(&self, owner: &str) -> Vec<Arc<Secret>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/secrets/");
        self.store
            .secrets()
            .list(&prefix)
            .into_iter()
            .map(|(_, secret)| secret)
            .collect()
    }

    /// The secret `name` that `owner` declares on the version this view sees.
    ///
    /// `name` is bare. A qualified string does not match, and [`Self::split`]
    /// is not used, so the running project's own `name` is a different secret.
    pub fn secret_of(&self, owner: &str, name: &str) -> Option<Arc<Secret>> {
        let version = self.visible_version(owner)?;
        self.store
            .secrets()
            .get(&Secret::to_path(owner, version, name))
    }

    pub fn create_secret(&self, mut input: Secret) -> Result<Arc<Secret>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        self.reject_shared_declaration_name(&input.name, true)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.secrets().create(input)?)
    }

    pub fn update_secret(
        &self,
        name: &str,
        patch: SecretUpdate,
    ) -> Result<Arc<Secret>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Secret::to_path(&self.project_id, &self.version, name);
        Ok(self.store.secrets().update(&key, patch)?)
    }

    pub fn delete_secret(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Secret::to_path(&self.project_id, &self.version, name);
        Ok(self.store.secrets().delete(&key)?)
    }

    /// Every variable declaration visible to this version, across self +
    /// direct deps. Each carries its `project`, like [`Self::collections`].
    pub fn variables(&self) -> Vec<Arc<Variable>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/variables/");
                self.store
                    .variables()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, variable)| variable)
            })
            .collect()
    }

    /// The variable `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn variable(&self, name: &str) -> Option<Arc<Variable>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .variables()
            .get(&Variable::to_path(project, version, bare))
    }

    /// Variables `owner` declares on the version this view sees.
    ///
    /// Same owner rule as [`Self::secrets_of`].
    pub fn variables_of(&self, owner: &str) -> Vec<Arc<Variable>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/variables/");
        self.store
            .variables()
            .list(&prefix)
            .into_iter()
            .map(|(_, variable)| variable)
            .collect()
    }

    /// The variable `name` that `owner` declares on the version this view sees.
    ///
    /// Same bare-name rule as [`Self::secret_of`].
    pub fn variable_of(&self, owner: &str, name: &str) -> Option<Arc<Variable>> {
        let version = self.visible_version(owner)?;
        self.store
            .variables()
            .get(&Variable::to_path(owner, version, name))
    }

    pub fn create_variable(
        &self,
        mut input: Variable,
    ) -> Result<Arc<Variable>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        self.reject_shared_declaration_name(&input.name, false)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.variables().create(input)?)
    }

    pub fn update_variable(
        &self,
        name: &str,
        patch: VariableUpdate,
    ) -> Result<Arc<Variable>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Variable::to_path(&self.project_id, &self.version, name);
        Ok(self.store.variables().update(&key, patch)?)
    }

    pub fn delete_variable(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Variable::to_path(&self.project_id, &self.version, name);
        Ok(self.store.variables().delete(&key)?)
    }

    /// Every action visible to this version, across self + direct deps.
    /// Each carries its `project`, like [`Self::collections`].
    pub fn actions(&self) -> Vec<Arc<Action>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/actions/");
                self.store
                    .actions()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, action)| action)
            })
            .collect()
    }

    /// The action `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn action(&self, name: &str) -> Option<Arc<Action>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .actions()
            .get(&Action::to_path(project, version, bare))
    }

    pub fn create_action(&self, mut input: Action) -> Result<Arc<Action>, VersionSchemaError> {
        check_action_params(input.params())?;
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.actions().create(input)?)
    }

    /// `patch.params` replaces the whole list. Omitting it leaves the stored
    /// params in place.
    pub fn update_action(
        &self,
        name: &str,
        patch: ActionUpdate,
    ) -> Result<Arc<Action>, VersionSchemaError> {
        if let Some(params) = &patch.params {
            check_action_params(params)?;
        }
        let _pins = self.write_guard()?;
        let key = Action::to_path(&self.project_id, &self.version, name);
        Ok(self.store.actions().update(&key, patch)?)
    }

    pub fn delete_action(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Action::to_path(&self.project_id, &self.version, name);
        Ok(self.store.actions().delete(&key)?)
    }

    /// A secret and a variable of one bare name in this version would make a
    /// later lookup ambiguous. The other store is only read, and that read
    /// lock drops before this store's `create` takes its writer. `PINS`,
    /// already held by `write_guard`, is what makes the check and the create
    /// one step — the two stores do not lock each other.
    fn reject_shared_declaration_name(
        &self,
        name: &str,
        creating_secret: bool,
    ) -> Result<(), VersionSchemaError> {
        let taken = if creating_secret {
            self.store
                .variables()
                .has(&Variable::to_path(&self.project_id, &self.version, name))
        } else {
            self.store
                .secrets()
                .has(&Secret::to_path(&self.project_id, &self.version, name))
        };
        if taken {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "secret and variable share the name '{name}'"
            )));
        }
        Ok(())
    }

    /// Append `field_name` to every `auto_add` fieldset in this project+version
    /// for the given collection. If none exists, lazy-create the default,
    /// seeded with every existing field in this project+version so collections
    /// that predate the feature don't end up with a partial default. The new
    /// `field_name` is appended last regardless.
    ///
    /// Each append is a [`InstanceStore::update_with`] on the fieldset store,
    /// so the list it extends is the current one, not a snapshot a concurrent
    /// `create_field` has since changed. Only the fieldset store is locked
    /// there; the field store is read beforehand, never inside it.
    ///
    /// [`InstanceStore::update_with`]: loco_schema_runtime::InstanceStore::update_with
    fn append_to_auto_add_sets(&self, collection: &str, field_name: &str) {
        let auto_sets =
            auto_add_fieldsets(&self.store, &self.project_id, &self.version, collection);
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
            match self.store.fieldsets().create(Fieldset {
                project: self.project_id.clone(),
                version: self.version.clone(),
                collection: collection.to_string(),
                name: DEFAULT_FIELDSET_NAME.to_string(),
                label: String::new(),
                fields: seed,
                auto_add: true,
            }) {
                // A concurrent create_field made the default first, maybe
                // from a field list that predates ours: append to it instead.
                Err(loco_schema_runtime::Error::AlreadyExists(_)) => {}
                _ => return,
            }
        }
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        for (key, _) in self.store.fieldsets().list(&prefix) {
            let _ = self.store.fieldsets().update_with(&key, |fs| {
                if !fs.auto_add || fs.fields.iter().any(|n| n == field_name) {
                    return None;
                }
                let mut next = fs.fields.clone();
                next.push(field_name.to_string());
                Some(FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                })
            });
        }
    }

    /// Strip `field_name` from every fieldset in this project+version's view of
    /// the collection. Touches only sets that actually reference the name,
    /// each under the fieldset store's write lock like the append above.
    fn remove_from_all_fieldsets(&self, collection: &str, field_name: &str) {
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        for (key, _) in self.store.fieldsets().list(&prefix) {
            let _ = self.store.fieldsets().update_with(&key, |fs| {
                if !fs.fields.iter().any(|n| n == field_name) {
                    return None;
                }
                let next: Vec<String> = fs
                    .fields
                    .iter()
                    .filter(|n| *n != field_name)
                    .cloned()
                    .collect();
                Some(FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                })
            });
        }
    }
}

/// Collection names are the slug charset (`[a-z0-9_.-]+`). `$` is outside
/// it, which is what keeps `$secrets` and `$variables` from being declared.
fn collection_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// The message for a `/schema` request to a version that does not exist.
pub fn unknown_version(project_id: &str, version: &str) -> String {
    format!("unknown version: {project_id}@{version}")
}

/// `{account}/{project}.{name}` → `(project, name)`. `None` for a bare name —
/// one with no `/`, since every project id has one and no name does.
pub fn split_qualified(name: &str) -> Option<(&str, &str)> {
    if !name.contains('/') {
        return None;
    }
    name.rsplit_once('.')
}

/// The `auto_add` fieldsets `project` declares on `collection` in `version`.
fn auto_add_fieldsets(
    store: &SchemaStore,
    project: &str,
    version: &str,
    collection: &str,
) -> Vec<Arc<Fieldset>> {
    let prefix = format!("{project}/versions/{version}/fieldsets/{collection}/");
    store
        .fieldsets()
        .list(&prefix)
        .into_iter()
        .filter_map(|(_, fs)| if fs.auto_add { Some(fs) } else { None })
        .collect()
}

/// Split a manifest dependency, `{account}/{project}@{version}`, into
/// `(project_id, version)`. `None` unless it has exactly that shape with no
/// empty part.
pub(crate) fn parse_dependency(dep: &str) -> Option<(&str, &str)> {
    let (project_id, version) = dep.split_once('@')?;
    let (account, project) = project_id.split_once('/')?;
    let segment = |s: &str| !s.is_empty() && !s.contains(['/', '@']);
    (segment(account) && segment(project) && segment(version)).then_some((project_id, version))
}

/// A secret body is a declaration. `default` and `value` are a store's
/// setting of it, and a package must not ship either. Presence of the key is
/// enough — `null` counts. A body that is not an object is left for the
/// deserializer.
pub fn reject_secret_value(body: &serde_json::Value) -> Result<(), VersionSchemaError> {
    let Some(obj) = body.as_object() else {
        return Ok(());
    };
    let present: Vec<&str> = ["default", "value"]
        .into_iter()
        .filter(|key| obj.contains_key(*key))
        .collect();
    if present.is_empty() {
        return Ok(());
    }
    let listed = present
        .iter()
        .map(|key| format!("'{key}'"))
        .collect::<Vec<_>>()
        .join(" and ");
    Err(VersionSchemaError::InvalidDeclaration(format!(
        "a secret declares a name, not a value: remove {listed}"
    )))
}

/// A field may declare only a type the validator enforces and the lake can
/// store. Checked on write only: a field loaded from disk with another type
/// still boots.
fn check_field_type(ty: &str) -> Result<(), VersionSchemaError> {
    if FIELD_TYPES.contains(&ty) {
        return Ok(());
    }
    Err(VersionSchemaError::InvalidFieldType(format!(
        "unknown field type '{ty}': expected one of {}",
        FIELD_TYPES.join(", ")
    )))
}

/// Params are part of the action document. Each `name` is one slug segment
/// (`[a-z0-9_.-]+`) and unique in the list, each `type` is a [`FIELD_TYPES`]
/// entry, and `options` are only stored on a string param. Checked on write
/// only: an action loaded from disk is left as it is, and a version copy
/// does not run this again.
fn check_action_params(params: &[ActionParam]) -> Result<(), VersionSchemaError> {
    let mut seen = HashSet::new();
    for param in params {
        if !param_name_ok(param.name()) {
            return Err(VersionSchemaError::InvalidName(format!(
                "param name {:?} must be a slug: one or more of a-z, 0-9, '_', '.', and '-'",
                param.name()
            )));
        }
        if !seen.insert(param.name()) {
            return Err(VersionSchemaError::InvalidName(format!(
                "param name '{}' is declared more than once",
                param.name()
            )));
        }
        check_field_type(param.r#type())?;
        if param.r#type() != "string" && !param.options().is_empty() {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "param '{}' declares options, which are only meaningful for type string",
                param.name()
            )));
        }
    }
    Ok(())
}

/// One segment of the schema slug charset. A param name is not a path, so a
/// `/` is not a separator here — it is simply not a slug character.
fn param_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// Why `deps` may not be the dependency list of a version of `project_id`,
/// or `Ok` when it may. Every entry must parse, name another project, name a
/// project no other entry names, and name a version whose manifest exists
/// and that `visible(entry, project)` allows.
///
/// An entry `visible` refuses gets the same error as one that does not
/// exist, word for word, so a manifest write cannot be used to learn whether
/// a project the caller cannot read has a given version (#86).
///
/// One version per project, because a qualified name
/// (`{account}/{project}.{name}`) carries no version: two versions of one
/// dependency could not both be addressed. Not this project, for the same
/// reason — its own names already resolve to self.
///
/// Callers hold `PINS`, so a version this finds cannot be deleted before
/// they write.
pub(crate) fn check_dependencies(
    store: &SchemaStore,
    project_id: &str,
    deps: &[String],
    visible: impl Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let mut seen = Vec::new();
    for dep in deps {
        let Some((dep_project, dep_version)) = parse_dependency(dep) else {
            return Err(format!(
                "dependency {dep:?} is not of the form {{account}}/{{project}}@{{version}}"
            ));
        };
        if dep_project == project_id {
            return Err(format!(
                "dependency {dep:?} names this project; a version cannot depend on its own project"
            ));
        }
        if seen.contains(&dep_project) {
            return Err(format!(
                "dependency {dep:?} repeats project {dep_project}; depend on one version of it"
            ));
        }
        seen.push(dep_project);
        if !visible(dep, dep_project)
            || !store
                .manifests()
                .has(&Manifest::to_path(dep_project, dep_version))
        {
            return Err(format!(
                "dependency {dep:?} names version {dep_version} of {dep_project}, which does not exist"
            ));
        }
    }
    Ok(())
}

/// Self entry plus direct deps from the manifest, as `(project_id, version)`
/// pairs. Writes are checked by [`check_dependencies`], but a manifest loaded
/// from disk is not, so a malformed entry is skipped here rather than failing
/// the read.
fn direct_dependencies(
    project_id: &str,
    version: &str,
    manifest: Option<&Manifest>,
) -> Vec<(String, String)> {
    let mut deps = vec![(project_id.to_string(), version.to_string())];
    if let Some(manifest) = manifest {
        for child in manifest.dependencies() {
            if let Some((namespace, v)) = parse_dependency(child) {
                deps.push((namespace.to_string(), v.to_string()));
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    const PROJECT: &str = "ben/crm";
    const VERSION: &str = "0.0.1-dev";

    fn draft_schema() -> (tempfile::TempDir, Arc<SchemaStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SchemaStore::load(dir.path()).unwrap());
        store
            .manifests()
            .create(Manifest::new(
                PROJECT.to_string(),
                VERSION.to_string(),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        (dir, store)
    }

    fn collection(name: &str) -> Collection {
        Collection::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
        )
    }

    /// The view was built while the version existed — as `VersionScope`
    /// checks — and the version is deleted before the write. The write must
    /// find that out itself and leave nothing on disk (#95).
    #[test]
    fn write_after_version_delete_is_refused_and_writes_nothing() {
        let (dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        assert!(schema.exists());
        crate::http::project_config::ProjectConfig::new(store.clone(), "ben", "crm")
            .delete_version(VERSION)
            .unwrap();

        let err = schema.create_collection(collection("tasks")).unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::UnknownVersion(_)),
            "{err:?}"
        );
        let err = schema
            .put_bundle(&loco_schema_runtime::FileTree::default())
            .unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::UnknownVersion(_)),
            "{err:?}"
        );
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert!(store.collections().list(&prefix).is_empty());
        assert!(store.fieldsets().list(&prefix).is_empty());
        assert!(store.bundles().list(&prefix).is_empty());
        // And on disk, where a reload would find an orphan.
        let reloaded = SchemaStore::load(dir.path()).unwrap();
        assert!(reloaded.collections().list(&prefix).is_empty());
        assert!(reloaded.bundles().list(&prefix).is_empty());
    }

    fn field(collection: &str, name: &str) -> Field {
        Field {
            project: PROJECT.to_string(),
            version: VERSION.to_string(),
            collection: collection.to_string(),
            name: name.to_string(),
            r#type: "string".to_string(),
            ..Field::default()
        }
    }

    /// Races `create_field` calls on one collection and returns the auto-add
    /// fieldset's `fields` afterwards.
    fn race_field_creates(store: &Arc<SchemaStore>, collection: &str, n: usize) -> Vec<String> {
        let barrier = Arc::new(Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                let collection = collection.to_string();
                std::thread::spawn(move || {
                    let schema = VersionSchema::new(store, PROJECT, VERSION);
                    barrier.wait();
                    schema
                        .create_field(field(&collection, &format!("f{i}")))
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let key = Fieldset::to_path(PROJECT, VERSION, collection, DEFAULT_FIELDSET_NAME);
        store.fieldsets().get(&key).unwrap().fields.clone()
    }

    fn assert_all_present(fields: &[String], n: usize) {
        let mut got = fields.to_vec();
        got.sort();
        let mut want: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
        want.sort();
        assert_eq!(got, want, "auto-add fieldset lost or duplicated a name");
    }

    #[test]
    fn concurrent_create_field_keeps_every_name_in_auto_add_set() {
        const N: usize = 16;
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        for round in 0..5 {
            let collection = format!("c{round}");
            schema
                .create_collection(self::collection(&collection))
                .unwrap();
            assert_all_present(&race_field_creates(&store, &collection, N), N);
        }
    }

    /// No default fieldset yet: the racers all try to lazy-create it, and the
    /// losers must still land their name in the winner's set.
    #[test]
    fn concurrent_create_field_lazy_creates_one_default_set() {
        const N: usize = 16;
        let (_dir, store) = draft_schema();
        for round in 0..5 {
            let collection = format!("c{round}");
            assert_all_present(&race_field_creates(&store, &collection, N), N);
        }
    }

    /// Directory mode is what stops `YamlFsAdapter` unlinking a key. Root
    /// ignores it, so the test refuses to pass vacuously there.
    struct Unlock(std::path::PathBuf);

    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    fn freeze(dir: &std::path::Path) -> Unlock {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".loco-probe");
        if std::fs::write(&probe, b"x").is_ok() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_file(&probe);
            panic!("directory mode did not block writes; this test needs a non-root user");
        }
        Unlock(dir.to_path_buf())
    }

    fn secret(name: &str) -> Secret {
        Secret::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
            false,
        )
    }

    fn variable(name: &str) -> Variable {
        Variable::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
            false,
            String::new(),
        )
    }

    #[test]
    fn secret_and_variable_cannot_share_a_name() {
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store, PROJECT, VERSION);
        schema.create_secret(secret("consumer_key")).unwrap();
        let err = schema
            .create_variable(variable("consumer_key"))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "secret and variable share the name 'consumer_key'"
        );
        schema.create_variable(variable("api_base")).unwrap();
        let err = schema.create_secret(secret("api_base")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "secret and variable share the name 'api_base'"
        );
        assert!(schema.secret("consumer_key").is_some());
        assert!(schema.variable("api_base").is_some());
    }

    #[test]
    fn dollar_is_not_a_collection_name() {
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        let err = schema
            .create_collection(collection("$secrets"))
            .unwrap_err();
        assert!(matches!(err, VersionSchemaError::InvalidName(_)), "{err}");
        assert!(err.to_string().contains('$'), "{err}");
        assert_eq!(
            crate::http::response::version_schema_error_to_response(err).status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        // A file planted under that name still does not resolve. `/data` and
        // `/data/query` both go through `collection`.
        store.collections().create(collection("$secrets")).unwrap();
        assert!(schema.collection("$secrets").is_none());
        assert!(schema.collection("alice/other.$variables").is_none());
        schema.create_collection(collection("orders")).unwrap();
        assert!(schema.collection("orders").is_some());
    }

    #[test]
    fn secret_body_rejects_default_and_value() {
        let err =
            reject_secret_value(&serde_json::json!({"name": "k", "default": "x"})).unwrap_err();
        assert_eq!(
            err.to_string(),
            "a secret declares a name, not a value: remove 'default'"
        );
        assert_eq!(
            crate::http::response::version_schema_error_to_response(err).status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let err = reject_secret_value(&serde_json::json!({"value": serde_json::Value::Null}))
            .unwrap_err();
        assert!(err.to_string().contains("'value'"));
        let err =
            reject_secret_value(&serde_json::json!({"default": "x", "value": "y"})).unwrap_err();
        assert_eq!(
            err.to_string(),
            "a secret declares a name, not a value: remove 'default' and 'value'"
        );
        assert!(reject_secret_value(&serde_json::json!({"name": "k", "label": "K"})).is_ok());
    }

    #[test]
    fn left_behind_answers_500() {
        let response = crate::http::response::version_schema_error_to_response(
            VersionSchemaError::LeftBehind(vec![
                "ben/crm/versions/0.0.1-dev/fieldsets/task/default".into(),
            ]),
        );
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn delete_collection_reports_a_failed_fieldset_and_clears_fields() {
        let (dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        schema.create_collection(collection("task")).unwrap();
        schema.create_field(field("task", "title")).unwrap();

        let fieldset_key = Fieldset::to_path(PROJECT, VERSION, "task", "default");
        let field_key = Field::to_path(PROJECT, VERSION, "task", "title");
        let collection_key = Collection::to_path(PROJECT, VERSION, "task");
        let frozen = dir
            .path()
            .join(format!("{fieldset_key}.yaml"))
            .parent()
            .unwrap()
            .to_path_buf();
        let _hold = freeze(&frozen);

        let err = schema.delete_collection("task").unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("delete left behind: {fieldset_key}")
        );
        match err {
            VersionSchemaError::LeftBehind(keys) => assert_eq!(keys, vec![fieldset_key.clone()]),
            other => panic!("expected leftovers, got {other}"),
        }
        assert!(store.fieldsets().has(&fieldset_key));
        assert!(!store.fields().has(&field_key));
        assert!(store.collections().has(&collection_key));

        drop(_hold);
        schema.delete_collection("task").unwrap();
        assert!(!store.fieldsets().has(&fieldset_key));
        assert!(!store.collections().has(&collection_key));
    }
}

#[cfg(test)]
mod action_param_checks {
    use super::check_action_params;
    use crate::{ActionParam, ActionParamOption};

    fn param(name: &str, ty: &str, options: &[(&str, &str)]) -> ActionParam {
        ActionParam {
            name: name.into(),
            r#type: ty.into(),
            options: options
                .iter()
                .map(|(value, label)| ActionParamOption {
                    value: (*value).into(),
                    label: (*label).into(),
                })
                .collect(),
            ..ActionParam::default()
        }
    }

    #[test]
    fn names_types_and_options() {
        assert!(check_action_params(&[
            param("qty", "integer", &[]),
            param("size", "string", &[("s", "Small")]),
        ])
        .is_ok());

        let err = check_action_params(&[param("Qty", "string", &[])]).unwrap_err();
        assert!(err.to_string().contains("slug"), "{err}");

        let err = check_action_params(&[param("qty", "integer", &[]), param("qty", "string", &[])])
            .unwrap_err();
        assert!(err.to_string().contains("more than once"), "{err}");

        let err = check_action_params(&[param("tags", "list", &[])]).unwrap_err();
        assert!(
            err.to_string().contains("unknown field type 'list'"),
            "{err}"
        );

        let err = check_action_params(&[param("qty", "integer", &[("1", "One")])]).unwrap_err();
        assert!(
            err.to_string().contains("only meaningful for type string"),
            "{err}"
        );
    }
}
