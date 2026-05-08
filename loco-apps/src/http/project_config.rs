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

use std::sync::Arc;

use loco_schema_runtime::Error;

use crate::{Dataset, DatasetUpdate, Project, ProjectUpdate, SchemaStore, Site, SiteUpdate};

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
    ) -> Result<Arc<Project>, Error> {
        self.store.projects().create(Project::new(
            self.project_id(),
            label.into(),
            description.into(),
        ))
    }

    pub fn update_project(&self, patch: ProjectUpdate) -> Result<Arc<Project>, Error> {
        self.store
            .projects()
            .update(&Project::to_path(&self.project_id()), patch)
    }

    /// Schema-level cascade: removes every dataset and site under this
    /// project, then the project record itself. Returns the dataset names
    /// that were removed so callers can purge their records from the lake.
    pub fn delete_project(&self) -> Result<Vec<String>, Error> {
        let prefix = format!("{}/", self.project_id());

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

    pub fn create_dataset(
        &self,
        name: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Arc<Dataset>, Error> {
        self.store.datasets().create(Dataset::new(
            self.project_id(),
            name.into(),
            label.into(),
            description.into(),
        ))
    }

    pub fn update_dataset(
        &self,
        name: &str,
        patch: DatasetUpdate,
    ) -> Result<Arc<Dataset>, Error> {
        self.store
            .datasets()
            .update(&Dataset::to_path(&self.project_id(), name), patch)
    }

    pub fn delete_dataset(&self, name: &str) -> Result<(), Error> {
        self.store
            .datasets()
            .delete(&Dataset::to_path(&self.project_id(), name))
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

    pub fn create_site(
        &self,
        name: impl Into<String>,
        label: impl Into<String>,
        version: impl Into<String>,
        dataset: impl Into<String>,
    ) -> Result<Arc<Site>, Error> {
        self.store.sites().create(Site::new(
            self.project_id(),
            name.into(),
            label.into(),
            version.into(),
            dataset.into(),
        ))
    }

    pub fn update_site(&self, name: &str, patch: SiteUpdate) -> Result<Arc<Site>, Error> {
        self.store
            .sites()
            .update(&Site::to_path(&self.project_id(), name), patch)
    }

    pub fn delete_site(&self, name: &str) -> Result<(), Error> {
        self.store
            .sites()
            .delete(&Site::to_path(&self.project_id(), name))
    }
}
