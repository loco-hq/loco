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

/// Fully-qualified `(project_id, version)` pointer to an installed schema.
/// `project_id` is `"user/project"`. Used as the entry type in a
/// `VersionSchema`'s dependency closure.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Dependency {
    pub project_id: String,
    pub version: String,
}

impl Dependency {
    pub fn new(project_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            version: version.into(),
        }
    }
}

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
    schema
        .manifests()
        .list_all()
        .into_iter()
        .find(|(_, m)| m.project() == project && m.version() == version)
        .map(|(_, m)| m)
}

/// Given a root dependency string like `ben/cars@0.0.1-dev`, walk the full
/// transitive dependency graph via Manifest instances. Returns every
/// `(project_id, version)` reachable from the root, root first.
pub fn resolve_dependency_tree(
    schema: &SchemaStore,
    root: &str,
) -> Result<Vec<Dependency>, ManifestError> {
    let (root_user, root_project, root_version) = parse_dependency(root)?;

    let mut visited: HashSet<Dependency> = HashSet::new();
    let mut result = Vec::new();
    let mut queue: VecDeque<Dependency> = VecDeque::new();
    queue.push_back(Dependency::new(
        format!("{root_user}/{root_project}"),
        root_version,
    ));

    while let Some(dep) = queue.pop_front() {
        if !visited.insert(dep.clone()) {
            continue;
        }
        result.push(dep.clone());

        if let Some(manifest) = find_manifest(schema, &dep.project_id, &dep.version) {
            for child in manifest.dependencies() {
                let (du, dp, dv) = parse_dependency(child)?;
                queue.push_back(Dependency::new(format!("{du}/{dp}"), dv));
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
