use crate::{SchemaStore, Site};

pub fn lookup_site_in_project(
    schema: &SchemaStore,
    project: &str,
    site_name: &str,
) -> Option<Site> {
    schema.sites().get(&Site::to_path(project, site_name))
}

pub fn resolve_dataset_id(
    schema: &SchemaStore,
    project: &str,
    site_name: &str,
) -> Result<String, String> {
    let site = lookup_site_in_project(schema, project, site_name)
        .ok_or_else(|| format!("unknown site: {site_name} in project {project}"))?;
    let dataset_field = site.dataset();
    let dataset = if dataset_field.is_empty() {
        site_name
    } else {
        dataset_field
    };
    Ok(format!("{project}/{dataset}"))
}

pub fn collection_key(user: &str, project: &str, name: &str) -> String {
    format!("{user}/{project}.{name}")
}
