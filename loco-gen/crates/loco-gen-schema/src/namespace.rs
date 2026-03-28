use serde::Deserialize;

use crate::error::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct NamespaceConfig {
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Parse a `loco.yaml` namespace config file.
pub fn parse_namespace_config(yaml: &str) -> Result<NamespaceConfig, Error> {
    let config: NamespaceConfig = serde_yaml::from_str(yaml)?;
    Ok(config)
}

/// Parse a dependency string like `alice/billing@0.1.0` into (user, project, version).
pub fn parse_dependency(dep: &str) -> Result<(&str, &str, &str), Error> {
    let (namespace, version) = dep
        .split_once('@')
        .ok_or_else(|| Error::InvalidValue(format!("dependency must be in format user/project@version, got: {dep}")))?;
    let (user, project) = namespace
        .split_once('/')
        .ok_or_else(|| Error::InvalidValue(format!("dependency namespace must be in format user/project, got: {namespace}")))?;
    Ok((user, project, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let yaml = "name: crm\n";
        let config = parse_namespace_config(yaml).unwrap();
        assert_eq!(config.name, "crm");
        assert!(config.dependencies.is_empty());
    }

    #[test]
    fn test_parse_config_with_dependencies() {
        let yaml = r#"
name: crm
dependencies:
  - loco/core@0.0.1
  - alice/billing@0.1.0
"#;
        let config = parse_namespace_config(yaml).unwrap();
        assert_eq!(config.name, "crm");
        assert_eq!(config.dependencies.len(), 2);
        assert_eq!(config.dependencies[0], "loco/core@0.0.1");
        assert_eq!(config.dependencies[1], "alice/billing@0.1.0");
    }

    #[test]
    fn test_parse_dependency() {
        let (user, project, version) = parse_dependency("alice/billing@0.1.0").unwrap();
        assert_eq!(user, "alice");
        assert_eq!(project, "billing");
        assert_eq!(version, "0.1.0");
    }

    #[test]
    fn test_parse_dependency_dev_version() {
        let (user, project, version) = parse_dependency("loco/core@0.0.1-dev").unwrap();
        assert_eq!(user, "loco");
        assert_eq!(project, "core");
        assert_eq!(version, "0.0.1-dev");
    }

    #[test]
    fn test_parse_dependency_missing_version() {
        let result = parse_dependency("alice/billing");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_dependency_missing_slash() {
        let result = parse_dependency("billing@0.1.0");
        assert!(result.is_err());
    }
}
