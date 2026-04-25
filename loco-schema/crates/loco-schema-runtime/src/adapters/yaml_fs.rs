//! Filesystem-backed adapter that persists each instance as a YAML file.
//!
//! Keys map to relative paths under `dir`: `key + ".yaml"`. Parent directories
//! are created on write and pruned on delete (up to but not including `dir`).

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::adapters::SchemaPersistence;
use crate::error::Error;
use crate::store::SchemaInstance;

pub struct YamlFsAdapter<T: SchemaInstance> {
    dir: PathBuf,
    _marker: PhantomData<fn() -> T>,
}

impl<T: SchemaInstance> YamlFsAdapter<T> {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            _marker: PhantomData,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn resolve(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.yaml"))
    }
}

impl<T: SchemaInstance> SchemaPersistence<T> for YamlFsAdapter<T> {
    fn load_all(&self) -> Result<Vec<(String, T)>, Error> {
        let mut out = Vec::new();
        for file_path in collect_yaml_files(&self.dir)? {
            let rel = file_path
                .strip_prefix(&self.dir)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .into_owned();
            let key = rel.strip_suffix(".yaml").unwrap_or(&rel).to_string();
            let Some(vars) = T::from_path(&key) else {
                continue;
            };
            let yaml = std::fs::read_to_string(&file_path)?;
            let inst = T::from_yaml(&yaml, &vars)?;
            out.push((key, inst));
        }
        Ok(out)
    }

    fn write(&self, key: &str, value: &T) -> Result<(), Error> {
        let path = self.resolve(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(value)?;
        std::fs::write(&path, yaml)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        let path = self.resolve(key);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == self.dir {
                break;
            }
            match std::fs::remove_dir(d) {
                Ok(()) => dir = d.parent(),
                Err(_) => break,
            }
        }
        Ok(())
    }
}

fn collect_yaml_files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_yaml_files(&path)?);
        } else if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            files.push(path);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestItem {
        project: String,
        name: String,
        label: String,
    }

    #[derive(Debug, Default)]
    struct TestItemUpdate {
        label: Option<String>,
    }

    impl SchemaInstance for TestItem {
        type Update = TestItemUpdate;
        fn to_path(&self) -> String {
            format!("{}/items/{}", self.project, self.name)
        }
        fn apply_update(&mut self, patch: &Self::Update) {
            if let Some(v) = &patch.label {
                self.label = v.clone();
            }
        }
        fn from_path(path: &str) -> Option<HashMap<String, String>> {
            let segs: Vec<&str> = path.split('/').collect();
            if segs.len() != 4 || segs[2] != "items" {
                return None;
            }
            let mut vars = HashMap::new();
            vars.insert("project".to_string(), format!("{}/{}", segs[0], segs[1]));
            vars.insert("name".to_string(), segs[3].to_string());
            Some(vars)
        }
        fn from_yaml(yaml: &str, vars: &HashMap<String, String>) -> Result<Self, Error> {
            let value: serde_yaml::Value = serde_yaml::from_str(yaml)?;
            Ok(TestItem {
                project: vars.get("project").cloned().unwrap_or_default(),
                name: vars.get("name").cloned().unwrap_or_default(),
                label: value
                    .get("label")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default(),
            })
        }
    }

    #[test]
    fn write_then_load_all_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());

        let item = TestItem {
            project: "ben/crm".to_string(),
            name: "account".to_string(),
            label: "Account".to_string(),
        };
        adapter.write("ben/crm/items/account", &item).unwrap();

        let yaml_path = dir.path().join("ben/crm/items/account.yaml");
        assert!(yaml_path.exists());

        let loaded = adapter.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "ben/crm/items/account");
        assert_eq!(loaded[0].1, item);
    }

    #[test]
    fn delete_prunes_empty_parents_up_to_dir() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());

        let item = TestItem {
            project: "ben/crm".to_string(),
            name: "a".to_string(),
            label: "A".to_string(),
        };
        adapter.write("ben/crm/items/a", &item).unwrap();
        assert!(dir.path().join("ben/crm/items").exists());

        adapter.delete("ben/crm/items/a").unwrap();
        assert!(!dir.path().join("ben/crm/items/a.yaml").exists());
        assert!(!dir.path().join("ben/crm/items").exists());
        assert!(!dir.path().join("ben/crm").exists());
        // Root is preserved.
        assert!(dir.path().exists());
    }

    #[test]
    fn delete_missing_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());
        adapter.delete("ben/crm/items/nope").unwrap();
    }

    #[test]
    fn load_all_skips_files_that_do_not_match_path_template() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());

        // Wrong shape — `from_path` returns None.
        let stray = dir.path().join("ben/crm/sites/acme.yaml");
        std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
        std::fs::write(&stray, "label: Acme\n").unwrap();

        let loaded = adapter.load_all().unwrap();
        assert!(loaded.is_empty());
    }
}
