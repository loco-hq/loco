use std::collections::{HashSet, VecDeque};

use crate::Manifest;

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

fn find_manifest(project: &str, version: &str) -> Option<Manifest> {
    crate::list_all_manifests()
        .into_iter()
        .find(|(_, m)| m.project() == project && m.version() == version)
        .map(|(_, m)| m)
}

fn manifest_dep_key(manifest: &Manifest) -> Option<String> {
    let project = manifest.project();
    let version = manifest.version();
    if project.is_empty() || version.is_empty() {
        return None;
    }
    Some(format!("{project}@{version}"))
}

/// Given a root dependency string like `ben/cars@0.0.1-dev`, walk the full
/// transitive dependency graph via Manifest instances. Returns every
/// `(user, project)` reachable from the root, root first.
pub fn resolve_dependency_tree(root: &str) -> Result<Vec<(String, String)>, ManifestError> {
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
        if let Some(manifest) = find_manifest(&project_str, &version) {
            for dep in manifest.dependencies() {
                let (du, dp, dv) = parse_dependency(dep)?;
                queue.push_back((du.to_string(), dp.to_string(), dv.to_string()));
            }
        }
    }

    Ok(result)
}

/// Walk every Manifest in the registry and verify that each declared
/// dependency is itself present as a Manifest. Intended for startup.
pub fn validate_manifests() -> Result<(), ManifestError> {
    let all = crate::list_all_manifests();
    let available: HashSet<String> = all
        .iter()
        .filter_map(|(_, m)| manifest_dep_key(m))
        .collect();

    for (_, manifest) in &all {
        let Some(from) = manifest_dep_key(manifest) else {
            continue;
        };
        for dep in manifest.dependencies() {
            parse_dependency(dep)?;
            if !available.contains(dep.as_str()) {
                return Err(ManifestError::UnsatisfiedDependency {
                    from,
                    missing: dep.to_string(),
                });
            }
        }
    }
    Ok(())
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
        let manifest = Manifest::new(
            "ben/crm".to_string(),
            "0.0.1-dev".to_string(),
            vec![],
        );
        assert_eq!(manifest_dep_key(&manifest), Some("ben/crm@0.0.1-dev".to_string()));
    }
}
