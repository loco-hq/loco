use std::path::{Path, PathBuf};

use crate::error::Error;

/// Serialize `value` to YAML and write it to `path`, creating parent directories as needed.
pub fn write_yaml<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(value)?;
    std::fs::write(path, yaml)?;
    Ok(())
}

/// Delete the file at `path` and recursively remove any now-empty parent
/// directories up to (but not including) `instances_dir`.
pub fn delete_yaml(path: &Path, instances_dir: &Path) -> Result<(), Error> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == instances_dir {
            break;
        }
        match std::fs::remove_dir(d) {
            Ok(()) => dir = d.parent(),
            Err(_) => break,
        }
    }
    Ok(())
}

/// Recursively collect every `.yaml` file under `dir`.
pub fn collect_yaml_files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
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
