//! Filesystem-backed adapter that persists each instance as a YAML file.
//!
//! Keys map to relative paths under `dir`: `key + ".yaml"`. Parent directories
//! are created on write and pruned on delete (up to but not including `dir`).
//!
//! Writes are atomic: the YAML is written to a temp sibling and renamed over
//! the target, so a crash or full disk mid-write leaves the old file, never a
//! truncated one. A temp file a crash leaves behind is ignored by `load_all`.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::adapters::atomic::{is_temp_name, write_file_atomic};
use crate::adapters::SchemaPersistence;
use crate::error::Error;
use crate::file_tree::validate_relative_path;
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

    /// The file a key lives at. Keys are relative paths, same rules as
    /// file-tree keys.
    fn resolve(&self, key: &str) -> Result<PathBuf, Error> {
        validate_relative_path(key)?;
        Ok(self.dir.join(format!("{key}.yaml")))
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
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(value)?;
        write_file_atomic(&path, yaml.as_bytes())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        let path = self.resolve(key)?;
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
        // Skip temp artefacts from an interrupted write — a half-written
        // YAML file or a file tree's staging directory.
        if is_temp_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
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

    fn item(name: &str, label: &str) -> TestItem {
        TestItem {
            project: "ben/crm".to_string(),
            name: name.to_string(),
            label: label.to_string(),
        }
    }

    #[test]
    fn write_replaces_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());

        adapter
            .write("ben/crm/items/a", &item("a", "First"))
            .unwrap();
        adapter
            .write("ben/crm/items/a", &item("a", "Second"))
            .unwrap();

        let parent = dir.path().join("ben/crm/items");
        let names: Vec<String> = std::fs::read_dir(&parent)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.yaml".to_string()]);

        let yaml = std::fs::read_to_string(parent.join("a.yaml")).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(value.get("label").and_then(|v| v.as_str()), Some("Second"));
    }

    #[test]
    fn load_all_ignores_leftover_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());
        adapter.write("ben/crm/items/a", &item("a", "A")).unwrap();

        // What a crash mid-write leaves: a truncated temp file next to the
        // target, and a file tree's staging directory holding YAML.
        let parent = dir.path().join("ben/crm/items");
        std::fs::write(parent.join(".loco-write-1-2-3"), "label: [unclosed").unwrap();
        std::fs::write(parent.join(".loco-write-1-2-4.yaml"), "label: [unclosed").unwrap();
        let staging = dir.path().join("ben/.loco-staging-1-2-3/items");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("b.yaml"), "label: [unclosed").unwrap();

        let loaded = adapter.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "ben/crm/items/a");
    }

    #[test]
    fn load_all_fails_on_corrupt_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().to_path_buf());

        let path = dir.path().join("ben/crm/items/a.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "label: [unclosed").unwrap();

        assert!(matches!(adapter.load_all(), Err(Error::Yaml(_))));
    }

    #[test]
    fn write_rejects_traversal_key() {
        let dir = tempfile::tempdir().unwrap();
        let adapter: YamlFsAdapter<TestItem> = YamlFsAdapter::new(dir.path().join("store"));
        let err = adapter.write("../escape", &item("a", "A")).unwrap_err();
        assert!(matches!(err, Error::InvalidPath(_)));
        assert!(!dir.path().join("escape.yaml").exists());
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
