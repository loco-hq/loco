//! Per-project, scoped view over the global `SchemaStore`.
//!
//! `ProjectConfig` is to the `/config` routes what `VersionSchema` is to
//! `/schema`: a layer between handlers and the underlying typed stores
//! (`projects()`, `datasets()`, `sites()`). It pins to a `(user, project)`
//! pair and exposes typed CRUD methods so handlers never reach into
//! `SchemaStore` directly.
//!
//! Construction is unchecked — `ProjectConfig::new` does not require the
//! project to exist. That's deliberate so `create_project` can build a
//! `ProjectConfig` first and then register the project plus its default
//! dataset/site through the same handle. Routes that *require* an
//! existing project gate on `exists()` at the scope layer
//! (`ConfigProjectScope`).
//!
//! A site's `version` and `dataset` are references into this project. Site
//! create and update refuse a pin to a version or dataset that does not
//! exist, and a dataset or version delete refuses while a site still pins
//! it. See [`PINS`] for how the check and the write stay one step.
//!
//! A manifest's `dependencies` are references too, across projects. A
//! version another project's manifest depends on cannot be deleted, nor can
//! its project, and a version copy re-checks the dependencies it carries.

use std::sync::{Arc, Mutex, MutexGuard};

use loco_schema_runtime::{Error, SchemaInstance};

use crate::http::names::{check_slug, check_version};
use crate::http::version_schema::{check_dependencies, parse_dependency};
use crate::{
    Bundle, Dataset, DatasetUpdate, Manifest, Project, ProjectUpdate, SchemaStore, Site, SiteUpdate,
};

/// Serializes every change that can make or break a site's pin or a
/// manifest's dependency: site create and update, manifest update
/// (`VersionSchema::update_manifest`), version copy, dataset and version
/// delete, and project delete. Every other `/schema` write holds it too
/// (`VersionSchema::write_guard`), so it cannot land in a version a delete
/// is removing and leave an orphan tree (#95). Each checks
/// one store (does the version exist? does a site pin it?) and writes
/// another, and a store's own writer lock covers only that store — without
/// this, a site create and a version delete could each pass its check and
/// both commit, leaving the site pinned to nothing.
///
/// It is global rather than per project because these are rare config
/// operations. Lock order: `PINS` first, then store writers one after
/// another, as usual. No store takes `PINS`, so it cannot deadlock them.
static PINS: Mutex<()> = Mutex::new(());

/// Guards no data of its own, so poison is ignored (as `InstanceStore` does).
pub(crate) fn lock_pins() -> MutexGuard<'static, ()> {
    PINS.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug)]
pub enum ConfigError {
    /// A project, dataset, site, or version name outside its charset
    /// (`crate::http::names`).
    InvalidName(String),
    /// A site names a version or dataset this project does not have.
    InvalidPin(String),
    /// A dataset, version, or project cannot be deleted while a site pins it
    /// or another project's manifest depends on it.
    Pinned(String),
    /// A copied manifest's dependencies no longer hold.
    InvalidDependency(String),
    /// Purging a dataset's records from the lake failed; nothing was deleted
    /// from the schema store.
    Purge(String),
    Schema(Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(msg)
            | Self::InvalidPin(msg)
            | Self::Pinned(msg)
            | Self::InvalidDependency(msg) => {
                write!(f, "{msg}")
            }
            Self::Purge(msg) => write!(f, "failed to purge dataset records: {msg}"),
            Self::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<Error> for ConfigError {
    fn from(e: Error) -> Self {
        Self::Schema(e)
    }
}

pub struct ProjectConfig {
    store: Arc<SchemaStore>,
    user: String,
    project: String,
}

impl ProjectConfig {
    pub fn new(
        store: Arc<SchemaStore>,
        user: impl Into<String>,
        project: impl Into<String>,
    ) -> Self {
        Self {
            store,
            user: user.into(),
            project: project.into(),
        }
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn project_id(&self) -> String {
        format!("{}/{}", self.user, self.project)
    }

    pub fn exists(&self) -> bool {
        self.store
            .projects()
            .has(&Project::to_path(&self.project_id()))
    }

    // --- Project ---

    pub fn project_record(&self) -> Option<Arc<Project>> {
        self.store
            .projects()
            .get(&Project::to_path(&self.project_id()))
    }

    pub fn create_project(
        &self,
        label: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Arc<Project>, ConfigError> {
        check_slug("project", &self.project).map_err(ConfigError::InvalidName)?;
        Ok(self.store.projects().create(Project::new(
            self.project_id(),
            label.into(),
            description.into(),
        ))?)
    }

    pub fn update_project(&self, patch: ProjectUpdate) -> Result<Arc<Project>, Error> {
        self.store
            .projects()
            .update(&Project::to_path(&self.project_id()), patch)
    }

    /// Schema-level cascade: removes every version (manifest + its
    /// collections, fields, and fieldsets), every dataset, and every site under this
    /// project, then the project record itself. Returns the dataset names
    /// that were removed so callers can purge their records from the lake.
    /// Refused while another project's manifest depends on any of its
    /// versions; this project's own sites are swept, not a reason to refuse.
    pub fn delete_project(&self) -> Result<Vec<String>, ConfigError> {
        // Under `PINS` so a site create racing this delete either lands first
        // and is swept, or runs after and finds its version gone.
        let _pins = lock_pins();
        let depended_on_by = self.dependents(|_| true);
        if !depended_on_by.is_empty() {
            return Err(ConfigError::Pinned(format!(
                "project {} is a dependency of: {}",
                self.project_id(),
                depended_on_by.join(", ")
            )));
        }
        let prefix = format!("{}/", self.project_id());
        let versions_prefix = format!("{}/versions/", self.project_id());

        // Cascade versioned metadata for every version. Fieldsets first, and
        // fatal: an orphan would hand a same-named project stale ordering.
        self.store.fieldsets().delete_by_prefix(&versions_prefix)?;
        let _ = self.store.fields().delete_by_prefix(&versions_prefix);
        let _ = self.store.collections().delete_by_prefix(&versions_prefix);
        let _ = self
            .store
            .permission_sets()
            .delete_by_prefix(&versions_prefix);
        let _ = self.store.manifests().delete_by_prefix(&versions_prefix);
        // File trees cascade too, or a deleted project leaves its frontends on
        // disk for a same-named project to inherit.
        let _ = self.store.bundles().delete_by_prefix(&versions_prefix);

        let mut dataset_names = Vec::new();
        for (ds_id, ds) in self.store.datasets().list_all() {
            if ds_id.starts_with(&prefix) {
                dataset_names.push(ds.name().to_string());
                let _ = self.store.datasets().delete(&ds_id);
            }
        }

        for (site_id, _) in self.store.sites().list_all() {
            if site_id.starts_with(&prefix) {
                let _ = self.store.sites().delete(&site_id);
            }
        }

        self.store
            .projects()
            .delete(&Project::to_path(&self.project_id()))?;

        Ok(dataset_names)
    }

    // --- Dataset ---

    pub fn datasets(&self) -> Vec<(String, Arc<Dataset>)> {
        let prefix = format!("{}/", self.project_id());
        self.store.datasets().list(&prefix)
    }

    pub fn dataset(&self, name: &str) -> Option<Arc<Dataset>> {
        self.store
            .datasets()
            .get(&Dataset::to_path(&self.project_id(), name))
    }

    pub fn create_dataset(&self, mut input: Dataset) -> Result<Arc<Dataset>, ConfigError> {
        check_slug("dataset", &input.name).map_err(ConfigError::InvalidName)?;
        input.project = self.project_id();
        Ok(self.store.datasets().create(input)?)
    }

    pub fn update_dataset(&self, name: &str, patch: DatasetUpdate) -> Result<Arc<Dataset>, Error> {
        self.store
            .datasets()
            .update(&Dataset::to_path(&self.project_id(), name), patch)
    }

    /// Delete a dataset, refusing while a site pins it. `purge` removes its
    /// records from the lake; it runs after the pin check and before the
    /// dataset itself goes, so a failed purge leaves the dataset in place.
    pub fn delete_dataset(
        &self,
        name: &str,
        purge: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), ConfigError> {
        let _pins = lock_pins();
        let pinned_by = self.sites_pinning(|site| site.dataset == name);
        if !pinned_by.is_empty() {
            return Err(ConfigError::Pinned(format!(
                "dataset {}/{name} is pinned by site(s): {}",
                self.project_id(),
                pinned_by.join(", ")
            )));
        }
        purge().map_err(ConfigError::Purge)?;
        self.store
            .datasets()
            .delete(&Dataset::to_path(&self.project_id(), name))?;
        Ok(())
    }

    // --- Site ---

    pub fn sites(&self) -> Vec<(String, Arc<Site>)> {
        let prefix = format!("{}/", self.project_id());
        self.store.sites().list(&prefix)
    }

    pub fn site(&self, name: &str) -> Option<Arc<Site>> {
        self.store
            .sites()
            .get(&Site::to_path(&self.project_id(), name))
    }

    /// Create a site. Its `version` and `dataset` must exist in this project.
    pub fn create_site(&self, mut input: Site) -> Result<Arc<Site>, ConfigError> {
        check_slug("site", &input.name).map_err(ConfigError::InvalidName)?;
        input.project = self.project_id();
        let _pins = lock_pins();
        self.check_pin(Some(&input.version), Some(&input.dataset))?;
        Ok(self.store.sites().create(input)?)
    }

    /// Update a site. A `version` or `dataset` in the patch must exist in
    /// this project; one left out is not re-checked.
    pub fn update_site(&self, name: &str, patch: SiteUpdate) -> Result<Arc<Site>, ConfigError> {
        let _pins = lock_pins();
        self.check_pin(patch.version.as_deref(), patch.dataset.as_deref())?;
        Ok(self
            .store
            .sites()
            .update(&Site::to_path(&self.project_id(), name), patch)?)
    }

    /// The pinned version and dataset, where given, exist in this project.
    /// Callers hold `PINS`.
    fn check_pin(&self, version: Option<&str>, dataset: Option<&str>) -> Result<(), ConfigError> {
        if let Some(version) = version {
            if self.manifest(version).is_none() {
                return Err(ConfigError::InvalidPin(format!(
                    "site pins version {version:?}, which {} does not have",
                    self.project_id()
                )));
            }
        }
        if let Some(dataset) = dataset {
            if self.dataset(dataset).is_none() {
                return Err(ConfigError::InvalidPin(format!(
                    "site pins dataset {dataset:?}, which {} does not have",
                    self.project_id()
                )));
            }
        }
        Ok(())
    }

    /// Names of this project's sites for which `pins` holds.
    fn sites_pinning(&self, pins: impl Fn(&Site) -> bool) -> Vec<String> {
        self.sites()
            .into_iter()
            .filter(|(_, site)| pins(site))
            .map(|(_, site)| site.name.clone())
            .collect()
    }

    pub fn delete_site(&self, name: &str) -> Result<(), Error> {
        self.store
            .sites()
            .delete(&Site::to_path(&self.project_id(), name))
    }

    // --- Version (manifest) ---

    pub fn manifests(&self) -> Vec<(String, Arc<Manifest>)> {
        let prefix = format!("{}/versions/", self.project_id());
        self.store.manifests().list(&prefix)
    }

    pub fn manifest(&self, version: &str) -> Option<Arc<Manifest>> {
        self.store
            .manifests()
            .get(&Manifest::to_path(&self.project_id(), version))
    }

    pub fn create_version(&self, version: String) -> Result<Arc<Manifest>, ConfigError> {
        check_version(&version).map_err(ConfigError::InvalidName)?;
        let manifest = Manifest::new(self.project_id(), version, Vec::new(), Vec::new());
        Ok(self.store.manifests().create(manifest)?)
    }

    /// Delete a version: cascade-removes the manifest plus all collections,
    /// fields, fieldsets, permission sets, and the bundle scoped to that
    /// version. Refused while a site pins it.
    pub fn delete_version(&self, version: &str) -> Result<(), ConfigError> {
        let _pins = lock_pins();
        let manifest_path = Manifest::to_path(&self.project_id(), version);
        if !self.store.manifests().has(&manifest_path) {
            return Err(Error::NotFound(manifest_path).into());
        }
        let pinned_by = self.sites_pinning(|site| site.version == version);
        if !pinned_by.is_empty() {
            return Err(ConfigError::Pinned(format!(
                "version {}@{version} is pinned by site(s): {}",
                self.project_id(),
                pinned_by.join(", ")
            )));
        }
        let depended_on_by = self.dependents(|v| v == version);
        if !depended_on_by.is_empty() {
            return Err(ConfigError::Pinned(format!(
                "version {}@{version} is a dependency of: {}",
                self.project_id(),
                depended_on_by.join(", ")
            )));
        }
        let fieldsets_prefix = format!("{}/versions/{version}/fieldsets/", self.project_id());
        let fields_prefix = format!("{}/versions/{version}/fields/", self.project_id());
        let collections_prefix = format!("{}/versions/{version}/collections/", self.project_id());
        let permission_sets_prefix =
            format!("{}/versions/{version}/permission_sets/", self.project_id());
        // Fatal, unlike the rest: an orphaned fieldset would hand a
        // recreated version the deleted one's field ordering.
        self.store.fieldsets().delete_by_prefix(&fieldsets_prefix)?;
        let _ = self.store.fields().delete_by_prefix(&fields_prefix);
        let _ = self
            .store
            .collections()
            .delete_by_prefix(&collections_prefix);
        let _ = self
            .store
            .permission_sets()
            .delete_by_prefix(&permission_sets_prefix);
        // The version's bundle goes with it; a recreated version must not
        // inherit the frontend of the one that was deleted.
        let _ = self
            .store
            .bundles()
            .delete(&Bundle::to_path(&self.project_id(), version));
        Ok(self.store.manifests().delete(&manifest_path)?)
    }

    /// Other projects' versions, as `{project}@{version}`, whose manifest
    /// depends on a version of this project for which `versions` holds.
    /// Callers hold `PINS`.
    ///
    /// A dependency is refused at write when it names this project, so only
    /// other projects are counted: an entry from before that check must not
    /// make a project undeletable by itself.
    fn dependents(&self, versions: impl Fn(&str) -> bool) -> Vec<String> {
        let project_id = self.project_id();
        self.store
            .manifests()
            .list_all()
            .into_iter()
            .filter(|(_, m)| m.project != project_id)
            .filter(|(_, m)| {
                m.dependencies().iter().any(|dep| {
                    parse_dependency(dep).is_some_and(|(p, v)| p == project_id && versions(v))
                })
            })
            .map(|(_, m)| format!("{}@{}", m.project, m.version))
            .collect()
    }

    // --- Version copy (the publish primitive) ---

    /// Copy every piece of versioned metadata from `from` into a brand-new
    /// version `to`: collections, fields, fieldsets, permission sets, the
    /// version's file trees (its `bundle`), and the manifest (dependencies
    /// plus the public permission-set assignment).
    ///
    /// Metadata is a version directory, not just its YAML — so a published
    /// snapshot that dropped the frontend would not be a snapshot, and
    /// rolling back by re-pinning the previous version would serve the wrong
    /// HTML.
    ///
    /// Datasets, sites, and lake records have no version dimension and are
    /// deliberately untouched. Publishing a snapshot neither forks the data
    /// nor moves a URL — pointing a site at `to` is a separate, explicit act.
    ///
    /// This lives on `/config` rather than `/schema` precisely because the
    /// target is normally *published*: `0.0.1-dev` → `0.0.1` is how a
    /// published version gets its content, so the copy must not go through
    /// `VersionSchema`'s draft-only write gate. Afterwards the published
    /// target is read-only to `/schema` like any other non-draft version.
    ///
    /// The manifest is written last, so a manifest at `to` means the copy
    /// finished. A failure part-way rolls back the instances this call
    /// created instead of leaving half a version that would block the retry
    /// with `AlreadyExists`.
    ///
    /// The source's dependencies are checked again, under `PINS`. A written
    /// manifest's always hold — its targets cannot be deleted while it names
    /// them — but one loaded from disk was never checked, and copying it into
    /// a published version would make a bad dependency permanent.
    ///
    /// The copier's access to each dependency is not checked. A copy is a
    /// snapshot: it declares nothing the source does not already declare,
    /// and the copier, a developer of this project, already reads through
    /// the source's dependencies. Checking would only stop a teammate
    /// without access to a dependency from publishing the project's draft.
    /// Its existence is still checked, and a missing one is reported only
    /// for a dependency the source names, which the copier can already read
    /// in the source manifest.
    ///
    /// `to` must be a valid version name; `from` need only exist, since a
    /// version loaded from disk may predate the charset.
    pub fn copy_version(&self, from: &str, to: &str) -> Result<Arc<Manifest>, ConfigError> {
        check_version(to).map_err(ConfigError::InvalidName)?;
        let _pins = lock_pins();
        let source_manifest = self
            .manifest(from)
            .ok_or_else(|| Error::NotFound(Manifest::to_path(&self.project_id(), from)))?;

        let target_manifest = Manifest::to_path(&self.project_id(), to);
        if self.store.manifests().has(&target_manifest) {
            return Err(Error::AlreadyExists(target_manifest).into());
        }
        check_dependencies(
            &self.store,
            &self.project_id(),
            source_manifest.dependencies(),
            |_, _| true,
        )
        .map_err(ConfigError::InvalidDependency)?;
        // `FileTreePersistence::write_tree` is a whole-tree replace, so unlike
        // the YAML `create` calls below it would silently overwrite a tree
        // already at the target. Refuse up front instead.
        if let Some((existing, _)) = self
            .store
            .bundles()
            .list(&Self::version_prefix(&self.project_id(), to))
            .into_iter()
            .next()
        {
            return Err(Error::AlreadyExists(existing).into());
        }

        let mut copied = CopiedKeys::default();
        match self.copy_version_metadata(from, to, &source_manifest, &mut copied) {
            Ok(manifest) => Ok(manifest),
            Err(e) => {
                self.rollback_version_copy(&copied);
                Err(e.into())
            }
        }
    }

    /// Keys are recorded *after* a successful `create`, never before: on a
    /// key collision the colliding instance belongs to whoever wrote it
    /// first, and the rollback must not delete it.
    fn copy_version_metadata(
        &self,
        from: &str,
        to: &str,
        source_manifest: &Manifest,
        copied: &mut CopiedKeys,
    ) -> Result<Arc<Manifest>, Error> {
        let source_prefix =
            |kind: &str| format!("{}{kind}/", Self::version_prefix(&self.project_id(), from));

        // These write straight to the stores rather than going through
        // `VersionSchema::create_collection`, which would inject a fresh
        // `default` fieldset and collide with the fieldsets copied below.
        for (_, collection) in self.store.collections().list(&source_prefix("collections")) {
            let mut copy = (*collection).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.collections().create(copy)?;
            copied.collections.push(key);
        }
        for (_, field) in self.store.fields().list(&source_prefix("fields")) {
            let mut copy = (*field).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.fields().create(copy)?;
            copied.fields.push(key);
        }
        for (_, fieldset) in self.store.fieldsets().list(&source_prefix("fieldsets")) {
            let mut copy = (*fieldset).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.fieldsets().create(copy)?;
            copied.fieldsets.push(key);
        }
        for (_, set) in self
            .store
            .permission_sets()
            .list(&source_prefix("permission_sets"))
        {
            let mut copy = (*set).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.permission_sets().create(copy)?;
            copied.permission_sets.push(key);
        }

        // File trees, by prefix rather than by naming `bundle`: the store
        // rewrites each key's version segment and skips anything that stops
        // matching its template. `bundle` is the only `kind: files` type
        // today, and each later one needs its own line here — `SchemaStore`
        // does not expose file-tree stores generically.
        let project_id = self.project_id();
        copied.bundles.extend(self.store.bundles().copy_by_prefix(
            &Self::version_prefix(&project_id, from),
            &Self::version_prefix(&project_id, to),
        )?);

        let mut manifest = source_manifest.clone();
        manifest.version = to.to_string();
        self.store.manifests().create(manifest)
    }

    /// The key prefix every instance of one version shares. File-tree keys sit
    /// directly under it (`…/versions/0.0.1/bundle`), which is why the copy and
    /// its guard use the version prefix rather than a per-kind one.
    fn version_prefix(project_id: &str, version: &str) -> String {
        format!("{project_id}/versions/{version}/")
    }

    /// Undo a partial [`Self::copy_version`], deleting exactly the instances
    /// that call created — never whatever else may already sit under the
    /// target version id. A prefix wipe would be shorter and would also
    /// destroy metadata this call had no part in writing.
    ///
    /// The manifest is never in here: it is written last, so a copy that
    /// reached it did not fail.
    fn rollback_version_copy(&self, copied: &CopiedKeys) {
        for key in &copied.fields {
            let _ = self.store.fields().delete(key);
        }
        for key in &copied.collections {
            let _ = self.store.collections().delete(key);
        }
        for key in &copied.fieldsets {
            let _ = self.store.fieldsets().delete(key);
        }
        for key in &copied.permission_sets {
            let _ = self.store.permission_sets().delete(key);
        }
        for key in &copied.bundles {
            let _ = self.store.bundles().delete(key);
        }
    }
}

/// Persistence keys written so far by one [`ProjectConfig::copy_version`],
/// so a failure can undo precisely that set.
#[derive(Default)]
struct CopiedKeys {
    collections: Vec<String>,
    fields: Vec<String>,
    fieldsets: Vec<String>,
    permission_sets: Vec<String>,
    /// File-tree keys (the version's `bundle`), which are whole trees rather
    /// than documents but undo the same way.
    bundles: Vec<String>,
}
