//! Which lake to open. Adding an adapter is a new variant and a match arm
//! in [`LakeConfig::open`]; callers do not grow a new argument.

use std::path::PathBuf;

use crate::adapter::DataAdapter;
use crate::adapters::memory::InMemoryAdapter;
use crate::adapters::sqlite::SqliteAdapter;
use crate::error::Error;

/// The lake a process opens.
#[derive(Debug)]
pub enum LakeConfig {
    Memory,
    Sqlite { path: PathBuf },
}

impl LakeConfig {
    /// Open the adapter.
    ///
    /// A SQLite path with a non-empty parent directory is created first, so
    /// `data/app.db` does not fail because `data/` is missing. A bare
    /// `loco.db` has an empty parent and creates nothing: the file stays
    /// relative to the working directory.
    pub fn open(&self) -> Result<Box<dyn DataAdapter>, Error> {
        match self {
            Self::Memory => Ok(Box::new(InMemoryAdapter::new())),
            Self::Sqlite { path } => {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    std::fs::create_dir_all(parent).map_err(|err| {
                        Error::Open(format!(
                            "failed to create the directory for the SQLite database {}: {err}",
                            parent.display()
                        ))
                    })?;
                }
                SqliteAdapter::new(path)
                    .map(|adapter| Box::new(adapter) as Box<dyn DataAdapter>)
                    .map_err(|err| Error::Open(format!("failed to open SQLite database: {err}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_open_creates_a_missing_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "loco-lake-open-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let path = dir.join("nested").join("app.db");
        assert!(!dir.exists());
        let adapter = LakeConfig::Sqlite { path: path.clone() }.open().unwrap();
        assert!(path.is_file());
        drop(adapter);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn memory_open_returns_an_adapter() {
        LakeConfig::Memory.open().unwrap();
    }

    #[test]
    fn sqlite_open_names_a_parent_that_cannot_be_created() {
        let dir = std::env::temp_dir().join(format!(
            "loco-lake-open-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("not-a-directory");
        std::fs::write(&blocker, b"x").unwrap();
        let err = match (LakeConfig::Sqlite {
            path: blocker.join("app.db"),
        })
        .open()
        {
            Err(err) => err,
            Ok(_) => panic!("expected open to fail"),
        };
        let message = err.to_string();
        assert!(
            message.starts_with("failed to create the directory for the SQLite database "),
            "{message}"
        );
        assert!(!message.contains("internal error"), "{message}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sqlite_open_names_a_file_that_cannot_be_opened() {
        let dir = std::env::temp_dir().join(format!(
            "loco-lake-open-dir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = match (LakeConfig::Sqlite { path: dir.clone() }).open() {
            Err(err) => err,
            Ok(_) => panic!("expected open to fail"),
        };
        let message = err.to_string();
        assert!(
            message.starts_with("failed to open SQLite database: "),
            "{message}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
