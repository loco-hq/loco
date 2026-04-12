use std::collections::{HashMap, HashSet, VecDeque};

use loco_gen_schema::registry::SchemaRegistry;

#[derive(Debug)]
pub enum ManifestError {
    InvalidDependency(String),
    UnsatisfiedDependency { from: String, missing: String },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDependency(s) => write!(f, "invalid dependency: {s}"),
            Self::UnsatisfiedDependency { from, missing } => {
                write!(f, "{from} requires {missing}, but it was not found")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Parse a dependency string like `alice/billing@0.1.0` into (user, project, version).
pub fn parse_dependency(dep: &str) -> Result<(&str, &str, &str), ManifestError> {
    let (namespace, version) = dep
        .split_once('@')
        .ok_or_else(|| ManifestError::InvalidDependency(dep.to_string()))?;
    let (user, project) = namespace
        .split_once('/')
        .ok_or_else(|| ManifestError::InvalidDependency(dep.to_string()))?;
    Ok((user, project, version))
}

/// Look up a Manifest's fields by (project, version). Returns `None` if no
/// matching Manifest instance is registered.
fn find_manifest(
    registry: &SchemaRegistry,
    project: &str,
    version: &str,
) -> Option<HashMap<String, String>> {
    registry
        .list_all_instances("manifest")
        .into_iter()
        .find(|(_, fields)| {
            fields.get("project").map(|p| p == project).unwrap_or(false)
                && fields.get("version").map(|v| v == version).unwrap_or(false)
        })
        .map(|(_, fields)| fields)
}

/// Read the `dependencies` list from a Manifest's fields. The registry
/// stores list values as JSON strings (see `instance_values_to_strings` in
/// loco-gen-schema).
fn parse_dependencies(fields: &HashMap<String, String>) -> Vec<String> {
    fields
        .get("dependencies")
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

/// Given a root dependency string like `ben/cars@0.0.1-dev`, walk the full
/// transitive dependency graph via Manifest instances. Returns every
/// `(user, project)` reachable from the root, root first.
pub fn resolve_dependency_tree(
    registry: &SchemaRegistry,
    root: &str,
) -> Result<Vec<(String, String)>, ManifestError> {
    let (root_user, root_project, root_version) = parse_dependency(root)?;

    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut result = Vec::new();
    let mut queue: VecDeque<(String, String, String)> = VecDeque::new();
    queue.push_back((
        root_user.to_string(),
        root_project.to_string(),
        root_version.to_string(),
    ));

    while let Some((user, project, version)) = queue.pop_front() {
        if !visited.insert((user.clone(), project.clone())) {
            continue;
        }
        result.push((user.clone(), project.clone()));

        let project_str = format!("{user}/{project}");
        if let Some(fields) = find_manifest(registry, &project_str, &version) {
            for dep in parse_dependencies(&fields) {
                let (du, dp, dv) = parse_dependency(&dep)?;
                queue.push_back((du.to_string(), dp.to_string(), dv.to_string()));
            }
        }
    }

    Ok(result)
}

/// Walk every Manifest in the registry and verify that each declared
/// dependency is itself present as a Manifest. Intended for startup.
pub fn validate_manifests(registry: &SchemaRegistry) -> Result<(), ManifestError> {
    let all = registry.list_all_instances("manifest");
    let available: HashSet<String> = all
        .iter()
        .filter_map(|(_, fields)| manifest_dep_key(fields))
        .collect();

    for (_, fields) in &all {
        let Some(from) = manifest_dep_key(fields) else { continue };
        for dep in parse_dependencies(fields) {
            parse_dependency(&dep)?;
            if !available.contains(&dep) {
                return Err(ManifestError::UnsatisfiedDependency {
                    from,
                    missing: dep,
                });
            }
        }
    }
    Ok(())
}

/// Derive the dependency-string form (`{project}@{version}`) from a Manifest's fields.
fn manifest_dep_key(fields: &HashMap<String, String>) -> Option<String> {
    let project = fields.get("project")?;
    let version = fields.get("version")?;
    if project.is_empty() || version.is_empty() {
        return None;
    }
    Some(format!("{project}@{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dependency_ok() {
        let (u, p, v) = parse_dependency("alice/billing@0.1.0").unwrap();
        assert_eq!((u, p, v), ("alice", "billing", "0.1.0"));
    }

    #[test]
    fn test_parse_dependency_errors() {
        assert!(parse_dependency("no-at-sign").is_err());
        assert!(parse_dependency("noSlash@1").is_err());
    }

    #[test]
    fn test_manifest_dep_key() {
        let mut fields = HashMap::new();
        fields.insert("project".to_string(), "ben/crm".to_string());
        fields.insert("version".to_string(), "0.0.1-dev".to_string());
        assert_eq!(manifest_dep_key(&fields), Some("ben/crm@0.0.1-dev".to_string()));
    }
}
