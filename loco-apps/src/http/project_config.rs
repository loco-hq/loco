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
    /// A project or version cascade attempted every store it could. These
    /// keys are still present. The project record or the version manifest
    /// was left in place, so the delete can be retried and a recreate cannot
    /// inherit what remains.
    LeftBehind(Vec<String>),
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
            Self::LeftBehind(keys) => write!(f, "delete left behind: {}", keys.join(", ")),
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

/// Record keys `delete_by_prefix` could not remove. An `Ok` means the prefix
/// is empty; on `Err` the store kept exactly the keys still on disk.
fn note_prefix(
    left: &mut Vec<String>,
    result: Result<Vec<String>, Error>,
    remaining: impl FnOnce() -> Vec<String>,
) {
    if result.is_err() {
        left.extend(remaining());
    }
}

/// Record `key` when its delete failed for a reason other than it already
/// being gone. A missing bundle, for example, is not something left behind.
fn note_key(left: &mut Vec<String>, result: Result<(), Error>, key: &str) {
    if let Err(err) = result {
        if !matches!(err, Error::NotFound(_)) {
            left.push(key.to_string());
        }
    }
}

fn keys_of<T>(entries: Vec<(String, T)>) -> Vec<String> {
    entries.into_iter().map(|(key, _)| key).collect()
}

fn cascade_result(mut left: Vec<String>) -> Result<(), ConfigError> {
    if left.is_empty() {
        Ok(())
    } else {
        left.sort();
        left.dedup();
        Err(ConfigError::LeftBehind(left))
    }
}

pub struct ProjectConfig {
    /// Schema instances. `pub(crate)` so config-value reads can build a
    /// `VersionSchema` without a second handle.
    pub(crate) store: Arc<SchemaStore>,
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

    /// Removes everything under this project: each version's collections,
    /// fields, fieldsets, permission sets, secrets, variables, actions,
    /// integration types, integrations, bundle, and manifest; every dataset
    /// and its records; every site; then the project record.
    /// Collections, fields, and actions declared on a type or an integration
    /// are lists on those documents, so deleting the document removes them.
    ///
    /// `purge` is called with that dataset's name before its schema row is
    /// removed. The handler's purge deletes the dataset's secrets
    /// (`SecretStore::delete_dataset`) and then its lake records. A purge
    /// failure leaves the row in place.
    ///
    /// Every store is attempted even when an earlier one fails, and the
    /// error names every key that could not be removed. A leftover fieldset
    /// is the worst of those: a later project or version at the same path
    /// would inherit its field ordering. The project record, and a version's
    /// manifest, are therefore removed only once nothing they own remains, so
    /// the failure can be retried and a recreate cannot inherit the leftover.
    /// Refused while another project's manifest depends on any version; this
    /// project's own sites are swept, not a reason to refuse.
    pub fn delete_project(
        &self,
        mut purge: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), ConfigError> {
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
        let versions_prefix = format!("{}/versions/", self.project_id());
        let mut left = Vec::new();

        note_prefix(
            &mut left,
            self.store.fieldsets().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.fieldsets().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.fields().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.fields().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.collections().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.collections().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store
                .permission_sets()
                .delete_by_prefix(&versions_prefix),
            || keys_of(self.store.permission_sets().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.secrets().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.secrets().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.variables().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.variables().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.actions().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.actions().list(&versions_prefix)),
        );
        // Integration types and integrations sit under the same version
        // prefix. Their collections and actions are on the documents.
        note_prefix(
            &mut left,
            self.store
                .integration_types()
                .delete_by_prefix(&versions_prefix),
            || keys_of(self.store.integration_types().list(&versions_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.integrations().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.integrations().list(&versions_prefix)),
        );
        // File trees too, or a deleted project leaves its frontends on disk
        // for a same-named project to inherit.
        note_prefix(
            &mut left,
            self.store.bundles().delete_by_prefix(&versions_prefix),
            || keys_of(self.store.bundles().list(&versions_prefix)),
        );

        for (ds_id, ds) in self.datasets() {
            if purge(ds.name()).is_err() {
                left.push(ds_id);
                continue;
            }
            note_key(&mut left, self.store.datasets().delete(&ds_id), &ds_id);
        }
        for (site_id, _) in self.sites() {
            note_key(&mut left, self.store.sites().delete(&site_id), &site_id);
        }

        // A version whose fieldset (or anything else under it) is still here
        // keeps its manifest. Deleting the manifest is what would let a
        // recreated version inherit that fieldset's ordering.
        let manifests = self.store.manifests().list(&versions_prefix);
        for (key, manifest) in manifests {
            if self.version_has_remaining(&manifest.version) {
                continue;
            }
            note_key(&mut left, self.store.manifests().delete(&key), &key);
        }

        if left.is_empty() {
            let project_key = Project::to_path(&self.project_id());
            note_key(
                &mut left,
                self.store.projects().delete(&project_key),
                &project_key,
            );
        }
        cascade_result(left)
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

    /// Delete a dataset, refusing while a site pins it. `purge` deletes its
    /// secrets and then its lake records, including `$variables`. It runs
    /// after the pin check and before the dataset itself goes, so a failed
    /// purge leaves the dataset in place. A value write holds `PINS` too, so
    /// it either lands first and is swept or runs after and finds the
    /// dataset gone.
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

    /// Delete a version: cascade-removes its collections, fields, fieldsets,
    /// permission sets, secrets, variables, actions, integration types,
    /// integrations, bundle, and manifest. Collections and actions on a type
    /// or an integration are lists on those documents. Secret and variable
    /// *values* are per dataset, not per version, and stay. Refused while a
    /// site pins it or another project's manifest depends on it.
    ///
    /// Every store is attempted even when an earlier one fails, and the error
    /// names every key that could not be removed. A leftover fieldset is the
    /// worst of those: a recreated version would inherit its field ordering,
    /// so the manifest stays until the rest are gone. The delete can then be
    /// retried, and a recreate cannot inherit the leftover.
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
        let secrets_prefix = format!("{}/versions/{version}/secrets/", self.project_id());
        let variables_prefix = format!("{}/versions/{version}/variables/", self.project_id());
        let actions_prefix = format!("{}/versions/{version}/actions/", self.project_id());
        let mut left = Vec::new();
        note_prefix(
            &mut left,
            self.store.fieldsets().delete_by_prefix(&fieldsets_prefix),
            || keys_of(self.store.fieldsets().list(&fieldsets_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.fields().delete_by_prefix(&fields_prefix),
            || keys_of(self.store.fields().list(&fields_prefix)),
        );
        note_prefix(
            &mut left,
            self.store
                .collections()
                .delete_by_prefix(&collections_prefix),
            || keys_of(self.store.collections().list(&collections_prefix)),
        );
        note_prefix(
            &mut left,
            self.store
                .permission_sets()
                .delete_by_prefix(&permission_sets_prefix),
            || keys_of(self.store.permission_sets().list(&permission_sets_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.secrets().delete_by_prefix(&secrets_prefix),
            || keys_of(self.store.secrets().list(&secrets_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.variables().delete_by_prefix(&variables_prefix),
            || keys_of(self.store.variables().list(&variables_prefix)),
        );
        note_prefix(
            &mut left,
            self.store.actions().delete_by_prefix(&actions_prefix),
            || keys_of(self.store.actions().list(&actions_prefix)),
        );
        let integration_types_prefix = format!(
            "{}/versions/{version}/integration_types/",
            self.project_id()
        );
        let integrations_prefix = format!("{}/versions/{version}/integrations/", self.project_id());
        note_prefix(
            &mut left,
            self.store
                .integration_types()
                .delete_by_prefix(&integration_types_prefix),
            || {
                keys_of(
                    self.store
                        .integration_types()
                        .list(&integration_types_prefix),
                )
            },
        );
        note_prefix(
            &mut left,
            self.store
                .integrations()
                .delete_by_prefix(&integrations_prefix),
            || keys_of(self.store.integrations().list(&integrations_prefix)),
        );
        // The version's bundle goes with it; a recreated version must not
        // inherit the frontend of the one that was deleted.
        let bundle_key = Bundle::to_path(&self.project_id(), version);
        note_key(
            &mut left,
            self.store.bundles().delete(&bundle_key),
            &bundle_key,
        );

        if !self.version_has_remaining(version) {
            note_key(
                &mut left,
                self.store.manifests().delete(&manifest_path),
                &manifest_path,
            );
        }
        cascade_result(left)
    }

    /// Anything under this version still in a store the cascade deletes.
    /// The manifest is not one of these: it is the anchor that stays while
    /// any of them do. Callers hold `PINS`.
    fn version_has_remaining(&self, version: &str) -> bool {
        let prefix = format!("{}/versions/{version}/", self.project_id());
        !self.store.fieldsets().list(&prefix).is_empty()
            || !self.store.fields().list(&prefix).is_empty()
            || !self.store.collections().list(&prefix).is_empty()
            || !self.store.permission_sets().list(&prefix).is_empty()
            || !self.store.secrets().list(&prefix).is_empty()
            || !self.store.variables().list(&prefix).is_empty()
            || !self.store.actions().list(&prefix).is_empty()
            || !self.store.integration_types().list(&prefix).is_empty()
            || !self.store.integrations().list(&prefix).is_empty()
            || !self.store.bundles().list(&prefix).is_empty()
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
    /// version `to`: collections, fields, fieldsets, permission sets, secrets,
    /// variables, actions, integration types, and integrations. Collections,
    /// fields, and actions on a type or an integration travel inside those
    /// documents. Also the version's file trees (its `bundle`) and the
    /// manifest (dependencies plus the public permission-set assignment).
    ///
    /// Metadata is a version directory, not just its YAML — so a published
    /// snapshot that dropped the frontend would not be a snapshot, and
    /// rolling back by re-pinning the previous version would serve the wrong
    /// HTML.
    ///
    /// Datasets, sites, lake records, and secret and variable values have no
    /// version dimension and are deliberately untouched. Publishing a
    /// snapshot neither forks the data nor moves a URL — pointing a site at
    /// `to` is a separate, explicit act.
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
        for (_, secret) in self.store.secrets().list(&source_prefix("secrets")) {
            let mut copy = (*secret).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.secrets().create(copy)?;
            copied.secrets.push(key);
        }
        for (_, variable) in self.store.variables().list(&source_prefix("variables")) {
            let mut copy = (*variable).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.variables().create(copy)?;
            copied.variables.push(key);
        }
        for (_, action) in self.store.actions().list(&source_prefix("actions")) {
            let mut copy = (*action).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.actions().create(copy)?;
            copied.actions.push(key);
        }
        // Collections, fields, and actions travel inside these documents.
        for (_, item) in self
            .store
            .integration_types()
            .list(&source_prefix("integration_types"))
        {
            let mut copy = (*item).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.integration_types().create(copy)?;
            copied.integration_types.push(key);
        }
        for (_, item) in self
            .store
            .integrations()
            .list(&source_prefix("integrations"))
        {
            let mut copy = (*item).clone();
            copy.version = to.to_string();
            let key = copy.to_path();
            self.store.integrations().create(copy)?;
            copied.integrations.push(key);
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
        for key in &copied.secrets {
            let _ = self.store.secrets().delete(key);
        }
        for key in &copied.variables {
            let _ = self.store.variables().delete(key);
        }
        for key in &copied.actions {
            let _ = self.store.actions().delete(key);
        }
        for key in &copied.integration_types {
            let _ = self.store.integration_types().delete(key);
        }
        for key in &copied.integrations {
            let _ = self.store.integrations().delete(key);
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
    secrets: Vec<String>,
    variables: Vec<String>,
    actions: Vec<String>,
    integration_types: Vec<String>,
    integrations: Vec<String>,
    /// File-tree keys (the version's `bundle`), which are whole trees rather
    /// than documents but undo the same way.
    bundles: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use loco_lake::DataAdapter;
    use loco_schema_runtime::FileTree;

    use super::*;
    use crate::{
        Action, ActionParam, Collection, Field, Fieldset, PermissionSet, Secret, Variable,
    };

    const PROJECT: &str = "ben/crm";
    const VERSION: &str = "0.0.1-dev";

    /// `chmod` a directory so the real `YamlFsAdapter` cannot unlink the files
    /// in it. Same failure `FailingDeletes` stands in for on `InstanceStore`:
    /// the adapter returns `Error::Io`, and the store keeps those keys.
    struct Unlock(PathBuf);

    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    fn freeze(dir: &Path) -> Unlock {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".loco-probe");
        if std::fs::write(&probe, b"x").is_ok() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_file(&probe);
            panic!("directory mode did not block writes; this test needs a non-root user");
        }
        Unlock(dir.to_path_buf())
    }

    fn yaml_dir(root: &Path, key: &str) -> PathBuf {
        root.join(format!("{key}.yaml"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    struct World {
        dir: tempfile::TempDir,
        store: Arc<SchemaStore>,
        data: Arc<dyn DataAdapter>,
        secrets: crate::values::LakeSecretStore,
        config: ProjectConfig,
    }

    fn world(with_site: bool) -> World {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SchemaStore::load(dir.path()).unwrap());
        let data: Arc<dyn DataAdapter> = Arc::new(loco_lake::InMemoryAdapter::new());
        let secrets = crate::values::LakeSecretStore::new(data.clone(), ready_key());
        let config = ProjectConfig::new(store.clone(), "ben", "crm");
        config.create_project("CRM", "").unwrap();
        config.create_version(VERSION.to_string()).unwrap();
        config
            .create_dataset(Dataset::new(
                String::new(),
                "dev".into(),
                "Dev".into(),
                String::new(),
            ))
            .unwrap();
        if with_site {
            config
                .create_site(Site::new(
                    String::new(),
                    "dev".into(),
                    "Dev".into(),
                    VERSION.into(),
                    "dev".into(),
                ))
                .unwrap();
        }
        for name in ["task", "note"] {
            store
                .collections()
                .create(Collection::new(
                    PROJECT.into(),
                    VERSION.into(),
                    name.into(),
                    String::new(),
                    String::new(),
                ))
                .unwrap();
        }
        for (collection, name) in [("task", "title"), ("note", "body")] {
            store
                .fields()
                .create(Field {
                    project: PROJECT.into(),
                    version: VERSION.into(),
                    collection: collection.into(),
                    name: name.into(),
                    r#type: "string".into(),
                    ..Field::default()
                })
                .unwrap();
        }
        store
            .fieldsets()
            .create(Fieldset {
                project: PROJECT.into(),
                version: VERSION.into(),
                collection: "task".into(),
                name: "default".into(),
                label: String::new(),
                fields: vec!["title".into()],
                auto_add: true,
            })
            .unwrap();
        store
            .permission_sets()
            .create(PermissionSet::new(
                PROJECT.into(),
                VERSION.into(),
                "public_read".into(),
                "Public".into(),
                String::new(),
                Vec::new(),
            ))
            .unwrap();
        store
            .secrets()
            .create(Secret::new(
                PROJECT.into(),
                VERSION.into(),
                "consumer_key".into(),
                "Consumer key".into(),
                String::new(),
                true,
            ))
            .unwrap();
        store
            .variables()
            .create(Variable::new(
                PROJECT.into(),
                VERSION.into(),
                "api_base".into(),
                "API base".into(),
                String::new(),
                false,
                "https://example.test".into(),
            ))
            .unwrap();
        store
            .actions()
            .create(Action::new(
                PROJECT.into(),
                VERSION.into(),
                "echo".into(),
                "Echo".into(),
                String::new(),
                vec![ActionParam {
                    name: "qty".into(),
                    r#type: "integer".into(),
                    required: true,
                    ..ActionParam::default()
                }],
            ))
            .unwrap();
        let mut tree = FileTree::new();
        tree.insert("index.html", b"<p>hi</p>".to_vec()).unwrap();
        store
            .bundles()
            .put(&Bundle::to_path(PROJECT, VERSION), &tree)
            .unwrap();
        World {
            dir,
            store,
            data,
            secrets,
            config,
        }
    }

    fn fieldset_key() -> String {
        Fieldset::to_path(PROJECT, VERSION, "task", "default")
    }

    fn task_field_key() -> String {
        Field::to_path(PROJECT, VERSION, "task", "title")
    }

    fn note_field_key() -> String {
        Field::to_path(PROJECT, VERSION, "note", "body")
    }

    fn assert_version_children_cleared(store: &SchemaStore, except_fields: &[&str]) {
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert!(store.collections().list(&prefix).is_empty());
        assert!(store.permission_sets().list(&prefix).is_empty());
        assert!(store.secrets().list(&prefix).is_empty());
        assert!(store.variables().list(&prefix).is_empty());
        assert!(store.actions().list(&prefix).is_empty());
        assert!(store.bundles().list(&prefix).is_empty());
        let fields = super::keys_of(store.fields().list(&prefix));
        let expected: Vec<String> = except_fields.iter().map(|key| (*key).to_string()).collect();
        assert_eq!(fields, expected);
    }

    #[test]
    fn left_behind_answers_500() {
        let response =
            crate::http::response::config_error_to_response(ConfigError::LeftBehind(vec![
                fieldset_key(),
            ]));
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn project_delete_reports_every_failed_store_and_empties_the_rest() {
        let world = world(true);
        let fieldset = fieldset_key();
        let task_field = task_field_key();
        let _fieldset_dir = freeze(&yaml_dir(world.dir.path(), &fieldset));
        let _field_dir = freeze(&yaml_dir(world.dir.path(), &task_field));

        let mut purged = Vec::new();
        let err = world
            .config
            .delete_project(|name| {
                purged.push(name.to_string());
                Ok(())
            })
            .unwrap_err();
        // Fieldsets fail first. Later stores still run: the other field, the
        // collections, the bundle, the dataset's records, and the site.
        assert_eq!(purged, vec!["dev".to_string()]);
        assert_eq!(
            err.to_string(),
            format!("delete left behind: {task_field}, {fieldset}")
        );
        let ConfigError::LeftBehind(keys) = err else {
            panic!("expected leftovers");
        };
        assert_eq!(keys, vec![task_field.clone(), fieldset.clone()]);

        assert!(world.store.fieldsets().has(&fieldset));
        assert!(world.store.fields().has(&task_field));
        assert!(!world.store.fields().has(&note_field_key()));
        assert_version_children_cleared(&world.store, &[task_field.as_str()]);
        assert!(world
            .store
            .datasets()
            .list(&format!("{PROJECT}/"))
            .is_empty());
        assert!(world.store.sites().list(&format!("{PROJECT}/")).is_empty());
        // Manifest and project stay, so a retry can finish and a recreate
        // cannot inherit the fieldset.
        assert!(world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
        assert!(world.store.projects().has(&Project::to_path(PROJECT)));

        drop(_fieldset_dir);
        drop(_field_dir);
        world.config.delete_project(|_| Ok(())).unwrap();
        assert!(!world.store.projects().has(&Project::to_path(PROJECT)));
        assert!(world
            .store
            .fieldsets()
            .list(&format!("{PROJECT}/"))
            .is_empty());
        assert!(world.store.fields().list(&format!("{PROJECT}/")).is_empty());
        assert!(world
            .store
            .manifests()
            .list(&format!("{PROJECT}/"))
            .is_empty());
    }

    #[test]
    fn version_delete_removes_secrets_and_variables() {
        let world = world(false);
        world.config.delete_version(VERSION).unwrap();
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert!(world.store.secrets().list(&prefix).is_empty());
        assert!(world.store.variables().list(&prefix).is_empty());
        assert!(world.store.actions().list(&prefix).is_empty());
        assert!(!world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
    }

    /// A secret the cascade cannot unlink stays, and so does the manifest:
    /// a recreated version must not inherit it. The variable, in another
    /// directory, is still removed.
    #[test]
    fn version_delete_keeps_the_manifest_when_a_secret_cannot_be_removed() {
        let world = world(false);
        let key = Secret::to_path(PROJECT, VERSION, "consumer_key");
        let _hold = freeze(&yaml_dir(world.dir.path(), &key));

        let err = world.config.delete_version(VERSION).unwrap_err();
        let ConfigError::LeftBehind(keys) = err else {
            panic!("expected leftovers");
        };
        assert_eq!(keys, vec![key.clone()]);
        assert!(world.store.secrets().has(&key));
        assert!(world
            .store
            .variables()
            .list(&format!("{PROJECT}/versions/{VERSION}/"))
            .is_empty());
        assert!(world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));

        drop(_hold);
        world.config.delete_version(VERSION).unwrap();
        assert!(!world.store.secrets().has(&key));
        assert!(!world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
    }

    #[test]
    fn project_delete_keeps_a_dataset_whose_records_fail_to_purge() {
        let world = world(true);
        world
            .config
            .create_dataset(Dataset::new(
                String::new(),
                "prod".into(),
                "Prod".into(),
                String::new(),
            ))
            .unwrap();
        let mut purged = Vec::new();
        let err = world
            .config
            .delete_project(|name| {
                purged.push(name.to_string());
                Err("disk says no".into())
            })
            .unwrap_err();
        assert_eq!(purged, vec!["dev".to_string(), "prod".to_string()]);
        let dev = Dataset::to_path(PROJECT, "dev");
        let prod = Dataset::to_path(PROJECT, "prod");
        assert_eq!(
            err.to_string(),
            format!("delete left behind: {dev}, {prod}")
        );
        assert!(world.store.datasets().has(&dev));
        assert!(world.store.datasets().has(&prod));
        assert_version_children_cleared(&world.store, &[]);
        assert!(!world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
        assert!(world.store.sites().list(&format!("{PROJECT}/")).is_empty());
        assert!(world.store.projects().has(&Project::to_path(PROJECT)));
    }

    #[test]
    fn version_delete_reports_a_failed_fieldset_and_clears_the_other_stores() {
        let world = world(false);
        let fieldset = fieldset_key();
        let _hold = freeze(&yaml_dir(world.dir.path(), &fieldset));

        let err = world.config.delete_version(VERSION).unwrap_err();
        assert_eq!(err.to_string(), format!("delete left behind: {fieldset}"));
        assert!(world.store.fieldsets().has(&fieldset));
        assert_version_children_cleared(&world.store, &[]);
        assert!(world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
        assert!(world
            .store
            .datasets()
            .has(&Dataset::to_path(PROJECT, "dev")));
        assert!(world.store.projects().has(&Project::to_path(PROJECT)));

        drop(_hold);
        world.config.delete_version(VERSION).unwrap();
        assert!(!world
            .store
            .manifests()
            .has(&Manifest::to_path(PROJECT, VERSION)));
        assert!(world
            .store
            .fieldsets()
            .list(&format!("{PROJECT}/"))
            .is_empty());
        assert!(world.store.projects().has(&Project::to_path(PROJECT)));
    }

    const VALUE_KEY: [u8; 32] = [4u8; 32];

    fn ready_key() -> crate::values::KeyStatus {
        crate::values::KeyStatus::Ready(VALUE_KEY)
    }

    fn dev_dataset() -> String {
        format!("{PROJECT}/dev")
    }

    fn purge_all(data: &dyn DataAdapter, name: &str) -> Result<(), String> {
        data.delete_dataset(&format!("{PROJECT}/{name}"))
            .map_err(|err| err.to_string())
    }

    #[test]
    fn project_delete_removes_secret_and_variable_values() {
        use crate::values::{put_variable, SecretStore, SECRETS, VARIABLES};

        let world = world(false);
        world
            .secrets
            .put(&dev_dataset(), "consumer_key", "s3cret")
            .unwrap();
        put_variable(
            world.data.as_ref(),
            &dev_dataset(),
            "api_base",
            "https://set.example",
        )
        .unwrap();
        // A user row in the same dataset goes with the same purge.
        world
            .data
            .insert(
                &dev_dataset(),
                "ben/crm.task",
                loco_lake::InsertRequest {
                    user: "ben".into(),
                    fields: std::collections::HashMap::new(),
                },
            )
            .unwrap();

        let data = world.data.clone();
        world
            .config
            .delete_project(|name| purge_all(data.as_ref(), name))
            .unwrap();

        assert!(world.data.list(&dev_dataset(), SECRETS).unwrap().is_empty());
        assert!(world
            .data
            .list(&dev_dataset(), VARIABLES)
            .unwrap()
            .is_empty());
        assert!(world
            .data
            .list(&dev_dataset(), "ben/crm.task")
            .unwrap()
            .is_empty());
        assert!(!world.store.projects().has(&Project::to_path(PROJECT)));
    }

    /// A purge that fails leaves the dataset row, so the secret and the
    /// variable stay with it. One lake delete covers every collection; there
    /// is no separate value path that could remove one and keep the other.
    #[test]
    fn project_delete_keeps_values_when_the_purge_fails() {
        use crate::values::{get_variable, SecretStore};

        let world = world(false);
        world
            .secrets
            .put(&dev_dataset(), "consumer_key", "s3cret")
            .unwrap();
        crate::values::put_variable(
            world.data.as_ref(),
            &dev_dataset(),
            "api_base",
            "https://set.example",
        )
        .unwrap();

        let err = world
            .config
            .delete_project(|_| Err("disk says no".into()))
            .unwrap_err();
        let dev = Dataset::to_path(PROJECT, "dev");
        assert_eq!(err.to_string(), format!("delete left behind: {dev}"));
        assert!(world.store.projects().has(&Project::to_path(PROJECT)));
        assert_eq!(
            world
                .secrets
                .get(&dev_dataset(), "consumer_key")
                .unwrap()
                .as_deref(),
            Some("s3cret")
        );
        assert_eq!(
            get_variable(world.data.as_ref(), &dev_dataset(), "api_base")
                .unwrap()
                .as_deref(),
            Some("https://set.example")
        );

        let data = world.data.clone();
        world
            .config
            .delete_project(|name| purge_all(data.as_ref(), name))
            .unwrap();
        assert!(world
            .secrets
            .get(&dev_dataset(), "consumer_key")
            .unwrap()
            .is_none());
        assert!(!world.store.projects().has(&Project::to_path(PROJECT)));
    }

    #[test]
    fn dataset_delete_removes_only_that_datasets_values() {
        use crate::values::{get_variable, put_variable, SecretStore};

        let world = world(false);
        world
            .config
            .create_dataset(Dataset::new(
                String::new(),
                "other".into(),
                "Other".into(),
                String::new(),
            ))
            .unwrap();
        world
            .secrets
            .put(&dev_dataset(), "consumer_key", "s3cret")
            .unwrap();
        let other = format!("{PROJECT}/other");
        put_variable(world.data.as_ref(), &other, "api_base", "keep").unwrap();

        let data = world.data.clone();
        world
            .config
            .delete_dataset("dev", || purge_all(data.as_ref(), "dev"))
            .unwrap();

        assert!(world
            .secrets
            .get(&dev_dataset(), "consumer_key")
            .unwrap()
            .is_none());
        assert!(world.config.dataset("dev").is_none());
        assert_eq!(
            get_variable(world.data.as_ref(), &other, "api_base")
                .unwrap()
                .as_deref(),
            Some("keep")
        );
        assert!(world.config.dataset("other").is_some());
    }

    #[test]
    fn version_delete_leaves_dataset_values() {
        use crate::values::{get_variable, put_variable, SecretStore};

        let world = world(false);
        world
            .secrets
            .put(&dev_dataset(), "consumer_key", "s3cret")
            .unwrap();
        put_variable(
            world.data.as_ref(),
            &dev_dataset(),
            "api_base",
            "https://set.example",
        )
        .unwrap();

        world.config.delete_version(VERSION).unwrap();

        assert!(world
            .store
            .secrets()
            .list(&format!("{PROJECT}/versions/{VERSION}/"))
            .is_empty());
        assert_eq!(
            world
                .secrets
                .get(&dev_dataset(), "consumer_key")
                .unwrap()
                .as_deref(),
            Some("s3cret")
        );
        assert_eq!(
            get_variable(world.data.as_ref(), &dev_dataset(), "api_base")
                .unwrap()
                .as_deref(),
            Some("https://set.example")
        );
    }

    #[test]
    fn list_follows_pins_and_put_follows_any_version() {
        use crate::http::config_values::ValueError;
        use crate::values::SecretStore;

        let world = world(false);
        let missing = crate::values::LakeSecretStore::new(
            world.data.clone(),
            crate::values::KeyStatus::Missing,
        );
        // No site pins `dev`, so the list is empty even though the version
        // declares both names.
        assert!(world
            .config
            .list_secret_values(&world.secrets, "dev")
            .unwrap()
            .is_empty());
        assert!(world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap()
            .is_empty());

        let err = world
            .config
            .set_secret_value(&missing, "dev", "consumer_key", "s3cret")
            .unwrap_err();
        assert!(matches!(err, ValueError::Unavailable(_)));
        assert!(err.to_string().contains("LOCO_SECRET_KEY"), "{err}");
        assert!(world
            .secrets
            .get(&dev_dataset(), "consumer_key")
            .unwrap()
            .is_none());

        let err = world
            .config
            .set_variable_value(world.data.as_ref(), "dev", "consumer_key", "nope")
            .unwrap_err();
        assert!(matches!(err, ValueError::Undeclared(_)));
        let err = world
            .config
            .set_secret_value(&world.secrets, "dev", "api_base", "nope")
            .unwrap_err();
        assert!(matches!(err, ValueError::Undeclared(_)));
        let err = world
            .config
            .list_secret_values(&world.secrets, "missing")
            .unwrap_err();
        assert!(matches!(err, ValueError::UnknownDataset(_)));

        // A write is allowed before any site pins the declaring version.
        let set = world
            .config
            .set_variable_value(world.data.as_ref(), "dev", "api_base", "")
            .unwrap();
        assert!(set.set);
        assert_eq!(set.value.as_deref(), Some(""));
        assert_eq!(set.source, Some("value"));
        assert!(world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap()
            .is_empty());

        world
            .config
            .create_site(Site::new(
                String::new(),
                "dev".into(),
                "Dev".into(),
                VERSION.into(),
                "dev".into(),
            ))
            .unwrap();
        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "api_base");
        assert_eq!(rows[0].project, PROJECT);
        assert!(rows[0].set);
        assert_eq!(rows[0].value.as_deref(), Some(""));
        assert_eq!(rows[0].source, Some("value"));

        world
            .config
            .delete_variable_value(world.data.as_ref(), "dev", "api_base")
            .unwrap();
        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert!(!rows[0].set);
        assert_eq!(rows[0].source, Some("default"));
        assert_eq!(rows[0].value.as_deref(), Some("https://example.test"));
        assert!(rows[0].updated_at.is_none());

        let secret = world
            .config
            .set_secret_value(&world.secrets, "dev", "consumer_key", "s3cret")
            .unwrap();
        assert!(secret.set);
        assert!(secret.updated_at.is_some());
        let listed = world
            .config
            .list_secret_values(&world.secrets, "dev")
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].set);
        assert_eq!(listed[0].name, "consumer_key");
    }

    /// `alice/shop.label_prefix` and `label_prefix` are one row. The secret
    /// read is the bare name, which is also the additional data the
    /// ciphertext was sealed under.
    #[test]
    fn qualified_self_name_stores_the_bare_row() {
        use crate::values::SecretStore;

        let world = world(true);
        let qualified = format!("{PROJECT}.consumer_key");
        let set = world
            .config
            .set_secret_value(&world.secrets, "dev", &qualified, "s3cret")
            .unwrap();
        assert_eq!(set.name, "consumer_key");
        assert_eq!(
            world
                .secrets
                .get(&dev_dataset(), "consumer_key")
                .unwrap()
                .as_deref(),
            Some("s3cret")
        );
        assert!(world
            .secrets
            .get(&dev_dataset(), &qualified)
            .unwrap()
            .is_none());
        assert!(
            world
                .config
                .list_secret_values(&world.secrets, "dev")
                .unwrap()[0]
                .set
        );
        world
            .config
            .delete_secret_value(&world.secrets, "dev", "consumer_key")
            .unwrap();
        assert!(world
            .secrets
            .get(&dev_dataset(), "consumer_key")
            .unwrap()
            .is_none());

        let qualified_var = format!("{PROJECT}.api_base");
        world
            .config
            .set_variable_value(
                world.data.as_ref(),
                "dev",
                &qualified_var,
                "https://set.example",
            )
            .unwrap();
        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert!(rows[0].set);
        assert_eq!(rows[0].value.as_deref(), Some("https://set.example"));
        world
            .config
            .delete_variable_value(world.data.as_ref(), "dev", &qualified_var)
            .unwrap();
        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert!(!rows[0].set);
        assert_eq!(rows[0].source, Some("default"));
    }

    /// Two sites pin two versions that declare the same variable with
    /// different defaults. The row's default is the first non-empty one in
    /// version-name order, and a stored value replaces it.
    #[test]
    fn variable_default_comes_from_the_earliest_pinned_version() {
        let world = world(true);
        world.config.create_version("0.0.0-dev".into()).unwrap();
        world.config.create_version("9.9.9-dev".into()).unwrap();
        world
            .store
            .variables()
            .create(Variable::new(
                PROJECT.into(),
                "0.0.0-dev".into(),
                "api_base".into(),
                "API base".into(),
                String::new(),
                false,
                String::new(),
            ))
            .unwrap();
        world
            .store
            .variables()
            .create(Variable::new(
                PROJECT.into(),
                "9.9.9-dev".into(),
                "api_base".into(),
                "API base".into(),
                String::new(),
                false,
                "https://later.example".into(),
            ))
            .unwrap();
        for (site, version) in [("early", "0.0.0-dev"), ("later", "9.9.9-dev")] {
            world
                .config
                .create_site(Site::new(
                    String::new(),
                    site.into(),
                    site.into(),
                    version.into(),
                    "dev".into(),
                ))
                .unwrap();
        }

        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].set);
        assert_eq!(rows[0].source, Some("default"));
        // `0.0.0-dev` sorts first and its default is empty, so the row takes
        // `0.0.1-dev`'s default rather than `9.9.9-dev`'s.
        assert_eq!(rows[0].value.as_deref(), Some("https://example.test"));

        world
            .config
            .set_variable_value(
                world.data.as_ref(),
                "dev",
                "api_base",
                "https://set.example",
            )
            .unwrap();
        let rows = world
            .config
            .list_variable_values(world.data.as_ref(), "dev")
            .unwrap();
        assert!(rows[0].set);
        assert_eq!(rows[0].source, Some("value"));
        assert_eq!(rows[0].value.as_deref(), Some("https://set.example"));
    }

    /// Version copy carries integration types and integrations, including
    /// the collections, fields, and actions inline on those documents.
    /// Deleting the source version removes the documents and leaves the copy.
    #[test]
    fn copy_and_delete_version_carry_integration_declarations() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SchemaStore::load(dir.path()).unwrap());
        let config = ProjectConfig::new(store.clone(), "ben", "crm");
        config.create_version(VERSION.into()).unwrap();
        let schema =
            crate::http::version_schema::VersionSchema::new(store.clone(), PROJECT, VERSION);

        schema
            .create_integration_type(crate::IntegrationType {
                name: "bricklink".into(),
                label: "BrickLink".into(),
                secrets: vec![crate::IntegrationSecret {
                    name: "consumer_key".into(),
                    required: true,
                    ..crate::IntegrationSecret::default()
                }],
                collections: vec![crate::IntegrationCollection {
                    name: "orders".into(),
                    label: "Orders".into(),
                    fields: vec![crate::IntegrationField {
                        name: "status".into(),
                        r#type: "string".into(),
                        ..crate::IntegrationField::default()
                    }],
                    ..crate::IntegrationCollection::default()
                }],
                actions: vec![crate::IntegrationAction {
                    name: "set_status".into(),
                    params: vec![crate::IntegrationActionParam {
                        name: "order_id".into(),
                        r#type: "string".into(),
                        ..crate::IntegrationActionParam::default()
                    }],
                    ..crate::IntegrationAction::default()
                }],
                ..crate::IntegrationType::default()
            })
            .unwrap();
        schema
            .create_integration(crate::Integration {
                name: "store".into(),
                r#type: "bricklink".into(),
                collections: vec![crate::IntegrationCollection {
                    name: "invoice".into(),
                    label: "Invoice".into(),
                    fields: vec![crate::IntegrationField {
                        name: "amount".into(),
                        r#type: "string".into(),
                        ..crate::IntegrationField::default()
                    }],
                    ..crate::IntegrationCollection::default()
                }],
                ..crate::Integration::default()
            })
            .unwrap();

        config.copy_version(VERSION, "1.0.0").unwrap();
        let published = crate::http::version_schema::VersionSchema::new_read_only(
            store.clone(),
            PROJECT,
            "1.0.0",
        );
        let copied_type = published.integration_type("bricklink").unwrap();
        assert_eq!(copied_type.version(), "1.0.0");
        assert_eq!(copied_type.secrets()[0].name(), "consumer_key");
        assert_eq!(copied_type.collections()[0].label(), "Orders");
        assert_eq!(copied_type.collections()[0].fields()[0].name(), "status");
        assert_eq!(copied_type.actions()[0].params()[0].name(), "order_id");
        let copied = published.integration("store").unwrap();
        assert_eq!(copied.r#type(), "bricklink");
        assert_eq!(copied.collections()[0].label(), "Invoice");
        assert_eq!(copied.collections()[0].fields()[0].name(), "amount");
        assert_eq!(
            schema.integration_type("bricklink").unwrap().version(),
            VERSION
        );

        config.delete_version(VERSION).unwrap();
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert!(store.integration_types().list(&prefix).is_empty());
        assert!(store.integrations().list(&prefix).is_empty());
        assert!(store
            .manifests()
            .get(&Manifest::to_path(PROJECT, VERSION))
            .is_none());
        assert!(store
            .integration_types()
            .get(&crate::IntegrationType::to_path(
                PROJECT,
                "1.0.0",
                "bricklink"
            ))
            .is_some());
        let kept = store
            .integrations()
            .get(&crate::Integration::to_path(PROJECT, "1.0.0", "store"))
            .unwrap();
        assert_eq!(kept.collections()[0].name(), "invoice");
        assert_eq!(kept.collections()[0].fields()[0].name(), "amount");
    }
}
