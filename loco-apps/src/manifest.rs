use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::{Manifest, SchemaStore};

#[derive(Debug)]
pub enum ManifestError {
    InvalidDependency(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDependency(s) => write!(f, "invalid dependency: {s}"),
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

fn find_manifest(schema: &SchemaStore, project: &str, version: &str) -> Option<Arc<Manifest>> {
    schema.manifests().list_all()
        .into_iter()
        .find(|(_, m)| m.project() == project && m.version() == version)
        .map(|(_, m)| m)
}

/// Given a root dependency string like `ben/cars@0.0.1-dev`, walk the full
/// transitive dependency graph via Manifest instances. Returns every
/// `(user, project)` reachable from the root, root first.
pub fn resolve_dependency_tree(schema: &SchemaStore, root: &str) -> Result<Vec<(String, String)>, ManifestError> {
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
        if let Some(manifest) = find_manifest(schema, &project_str, &version) {
            for dep in manifest.dependencies() {
                let (du, dp, dv) = parse_dependency(dep)?;
                queue.push_back((du.to_string(), dp.to_string(), dv.to_string()));
            }
        }
    }

    Ok(result)
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
}
