//! Process configuration, read from the environment in one place.
//!
//! Nothing else in `loco-apps` calls [`std::env::var`]. Tests build a
//! [`Config`] directly, or call [`Config::parse`] with the variables they care
//! about.

use std::path::{Path, PathBuf};

pub use loco_lake::LakeConfig;

use crate::auth::AuthConfig;
use crate::values::KeyStatus;

/// Default SQLite file name. A relative path. See [`resolve_sqlite_path`].
const DEFAULT_SQLITE_FILE: &str = "loco.db";

/// What one process was asked to run.
///
/// `Debug` hides the secret key bytes: [`KeyStatus`] prints `Ready` without them.
#[derive(Debug)]
pub struct Config {
    /// Schema and auth root. `LOCO_ROOT` when that names a directory, made
    /// absolute. Otherwise this crate's directory (`loco-apps/`).
    pub root: PathBuf,
    /// `PORT`, or 3000.
    pub port: u16,
    /// Lake to open. A relative SQLite path stays relative to the working
    /// directory when `LOCO_ROOT` is unset, and is joined onto the root when
    /// `LOCO_ROOT` is set. `memory` carries no path: the process opens no file.
    pub lake: LakeConfig,
    pub auth: AuthConfig,
    /// `LOCO_DEFAULT_SITE` when it is non-blank. Blank and unset are `None`
    /// (the apex is API-only). The shape is checked at boot, not here.
    pub default_site: Option<String>,
    pub secret_key: KeyStatus,
}

impl Config {
    /// Read the process environment. An unknown `LOCO_ADAPTER` or
    /// `LOCO_AUTH_ADAPTER`, or a bad `PORT`, is an error. A missing or
    /// malformed `LOCO_SECRET_KEY` is not: it stays on [`KeyStatus`] and the
    /// process still boots.
    pub fn from_env() -> Result<Self, String> {
        Self::parse(|key| std::env::var(key).ok())
    }

    /// `var` returns one variable, or `None` when it is unset.
    pub fn parse(mut var: impl FnMut(&str) -> Option<String>) -> Result<Self, String> {
        let configured_root = parse_loco_root(var("LOCO_ROOT").as_deref());
        let root = match &configured_root {
            Some(path) => absolute_path(path),
            None => default_data_root().to_path_buf(),
        };
        // Join the database onto the root only when LOCO_ROOT is set. Unset,
        // the path stays relative to the working directory, which is where
        // the dev database already lives.
        let sqlite_root = configured_root.as_ref().map(|_| root.as_path());
        let sqlite_path = resolve_sqlite_path(sqlite_root, var("LOCO_DB_PATH").as_deref());

        // A bad port is reported before an unknown adapter, matching the
        // order main used to check them.
        let port = resolve_port(var("PORT").as_deref())?;

        let adapter = var("LOCO_ADAPTER").unwrap_or_else(|| "sqlite".to_string());
        let lake = match adapter.as_str() {
            "memory" => LakeConfig::Memory,
            "sqlite" => LakeConfig::Sqlite { path: sqlite_path },
            other => {
                return Err(format!(
                    "unknown LOCO_ADAPTER: {other} (expected \"sqlite\" or \"memory\")"
                ));
            }
        };

        let auth_adapter = var("LOCO_AUTH_ADAPTER").unwrap_or_else(|| "local".to_string());
        let auto_create = var("LOCO_AUTH_AUTO_CREATE")
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        let auth = match auth_adapter.as_str() {
            "local" => AuthConfig::Local {
                dir: root.join("auth"),
                auto_create,
            },
            other => {
                return Err(format!(
                    "unknown LOCO_AUTH_ADAPTER: {other} (expected \"local\")"
                ));
            }
        };

        let default_site = var("LOCO_DEFAULT_SITE").filter(|raw| !raw.trim().is_empty());
        let secret_key = KeyStatus::parse(var("LOCO_SECRET_KEY").as_deref());

        Ok(Self {
            root,
            port,
            lake,
            auth,
            default_site,
            secret_key,
        })
    }
}

/// Crate directory (`loco-apps/`). The schema and auth root when `LOCO_ROOT`
/// is unset.
fn default_data_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `LOCO_ROOT` when it names a directory. Blank is unset: the process keeps
/// [`default_data_root`] and does not move the SQLite file.
fn parse_loco_root(value: Option<&str>) -> Option<PathBuf> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

/// SQLite path the process opens.
///
/// `root` is `Some` only when `LOCO_ROOT` is set. The default file
/// (`loco.db`) and a relative `LOCO_DB_PATH` are then joined onto that
/// root. When `root` is `None`, the path is `LOCO_DB_PATH` or `loco.db`
/// and a relative path stays relative to the working directory.
///
/// `cargo run -p loco-apps` from the repo root, with `LOCO_ROOT` unset,
/// opens `./loco.db` there. The schema root in that case is still
/// `loco-apps/`. Joining the database onto that directory would open a
/// different file.
///
/// An absolute `LOCO_DB_PATH` is used as given in both cases. An empty
/// string is a relative path, the same as an empty `LOCO_DB_PATH` was
/// before `LOCO_ROOT` existed.
fn resolve_sqlite_path(root: Option<&Path>, db_path: Option<&str>) -> PathBuf {
    let raw = db_path.unwrap_or(DEFAULT_SQLITE_FILE);
    let path = Path::new(raw);
    match root {
        Some(root) if path.is_relative() => root.join(path),
        _ => PathBuf::from(raw),
    }
}

/// Absolute form of `path` for the startup log. The file does not have to
/// exist, and symlinks are left as written. This does not change the path
/// [`resolve_sqlite_path`] returns: with `LOCO_ROOT` unset that path stays
/// relative so the open follows the working directory.
pub fn absolute_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `PORT`, or 3000 when the variable is unset. Blank and non-numeric values
/// are errors. `0` and `65535` are in range.
fn resolve_port(value: Option<&str>) -> Result<u16, String> {
    let Some(raw) = value else {
        return Ok(3000);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("PORT is empty; set a number from 0 to 65535".to_string());
    }
    raw.parse::<u16>()
        .map_err(|_| format!("PORT must be a number from 0 to 65535, got {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(pairs: &[(&str, &str)]) -> Result<Config, String> {
        Config::parse(|key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        })
    }

    fn sqlite_path(config: &Config) -> &Path {
        match &config.lake {
            LakeConfig::Sqlite { path } => path,
            LakeConfig::Memory => panic!("expected the sqlite adapter"),
        }
    }

    fn auto_create(config: &Config) -> bool {
        match &config.auth {
            AuthConfig::Local { auto_create, .. } => *auto_create,
        }
    }

    #[test]
    fn sqlite_path_stays_cwd_relative_when_root_is_unset() {
        let config = parse(&[]).unwrap();
        let path = sqlite_path(&config);
        assert_eq!(path, Path::new("loco.db"));
        assert!(path.is_relative());
        // The schema root is the crate directory. The database is not under it.
        assert!(config.root.join("loco.db").is_absolute());
        assert_ne!(path, config.root.join("loco.db"));
        assert_eq!(config.root, default_data_root());
        assert_eq!(config.port, 3000);
        assert!(!auto_create(&config));
        assert!(config.default_site.is_none());
        assert!(matches!(config.secret_key, KeyStatus::Missing));
        match &config.auth {
            AuthConfig::Local { dir, .. } => assert_eq!(dir, &config.root.join("auth")),
        }
    }

    #[test]
    fn sqlite_path_keeps_a_relative_override_when_root_is_unset() {
        assert_eq!(
            sqlite_path(&parse(&[("LOCO_DB_PATH", "data/app.db")]).unwrap()),
            Path::new("data/app.db")
        );
        assert_eq!(
            sqlite_path(&parse(&[("LOCO_DB_PATH", "")]).unwrap()),
            Path::new("")
        );
    }

    #[test]
    fn sqlite_path_joins_relative_paths_when_root_is_set() {
        let config = parse(&[("LOCO_ROOT", "/tmp/x")]).unwrap();
        assert_eq!(config.root, Path::new("/tmp/x"));
        assert_eq!(sqlite_path(&config), Path::new("/tmp/x/loco.db"));
        assert_eq!(
            sqlite_path(
                &parse(&[("LOCO_ROOT", "/tmp/x"), ("LOCO_DB_PATH", "data/app.db")]).unwrap()
            ),
            Path::new("/tmp/x/data/app.db")
        );
        assert_eq!(
            sqlite_path(&parse(&[("LOCO_ROOT", "/tmp/x"), ("LOCO_DB_PATH", "")]).unwrap()),
            Path::new("/tmp/x")
        );
    }

    #[test]
    fn sqlite_path_keeps_an_absolute_override() {
        let absolute = "/var/loco/app.db";
        let unset = parse(&[("LOCO_DB_PATH", absolute)]).unwrap();
        assert_eq!(sqlite_path(&unset), Path::new(absolute));
        assert!(sqlite_path(&unset).is_absolute());
        let rooted = parse(&[("LOCO_ROOT", "/tmp/x"), ("LOCO_DB_PATH", absolute)]).unwrap();
        assert_eq!(sqlite_path(&rooted), Path::new(absolute));
        assert_eq!(rooted.root, Path::new("/tmp/x"));
    }

    #[test]
    fn blank_loco_root_is_unset() {
        let unset = parse(&[]).unwrap();
        assert_eq!(parse(&[("LOCO_ROOT", "")]).unwrap().root, unset.root);
        assert_eq!(parse(&[("LOCO_ROOT", "   ")]).unwrap().root, unset.root);
        assert_eq!(
            parse(&[("LOCO_ROOT", " /tmp/x ")]).unwrap().root,
            Path::new("/tmp/x")
        );
    }

    #[test]
    fn relative_root_is_made_absolute_and_the_database_joins_it() {
        let config = parse(&[("LOCO_ROOT", "data")]).unwrap();
        assert_eq!(config.root, absolute_path(Path::new("data")));
        assert!(config.root.is_absolute());
        assert_eq!(sqlite_path(&config), config.root.join("loco.db"));
    }

    #[test]
    fn absolute_path_logs_a_relative_database_under_the_working_directory() {
        let logged = absolute_path(Path::new("loco.db"));
        assert!(logged.is_absolute());
        assert_eq!(logged.file_name().unwrap(), "loco.db");
        assert_eq!(logged, std::env::current_dir().unwrap().join("loco.db"));
        assert_eq!(
            absolute_path(Path::new("/tmp/x/loco.db")),
            Path::new("/tmp/x/loco.db")
        );
    }

    #[test]
    fn port_defaults_and_parses() {
        assert_eq!(parse(&[]).unwrap().port, 3000);
        assert_eq!(parse(&[("PORT", "3100")]).unwrap().port, 3100);
        assert_eq!(parse(&[("PORT", " 3100 ")]).unwrap().port, 3100);
        assert_eq!(parse(&[("PORT", "0")]).unwrap().port, 0);
        assert_eq!(parse(&[("PORT", "65535")]).unwrap().port, 65535);
    }

    #[test]
    fn port_rejects_blank_and_non_numeric() {
        let empty = parse(&[("PORT", "")]).unwrap_err();
        assert!(empty.contains("PORT is empty"), "{empty}");
        let blank = parse(&[("PORT", "   ")]).unwrap_err();
        assert!(blank.contains("PORT is empty"), "{blank}");
        let text = parse(&[("PORT", "http")]).unwrap_err();
        assert!(
            text.contains("PORT must be a number from 0 to 65535, got http"),
            "{text}"
        );
        assert!(parse(&[("PORT", "65536")]).is_err());
        assert!(parse(&[("PORT", "-1")]).is_err());
        // A bad port is reported before an unknown adapter.
        let both = parse(&[("PORT", "nope"), ("LOCO_ADAPTER", "nope")]).unwrap_err();
        assert!(both.contains("PORT"), "{both}");
    }

    #[test]
    fn adapter_kinds_and_blank_values() {
        assert!(matches!(
            parse(&[]).unwrap().lake,
            LakeConfig::Sqlite { .. }
        ));
        assert!(matches!(
            parse(&[("LOCO_ADAPTER", "sqlite")]).unwrap().lake,
            LakeConfig::Sqlite { .. }
        ));
        let memory = parse(&[("LOCO_ADAPTER", "memory")]).unwrap();
        assert!(matches!(memory.lake, LakeConfig::Memory));

        let unknown = parse(&[("LOCO_ADAPTER", "postgres")]).unwrap_err();
        assert_eq!(
            unknown,
            "unknown LOCO_ADAPTER: postgres (expected \"sqlite\" or \"memory\")"
        );
        let blank = parse(&[("LOCO_ADAPTER", "")]).unwrap_err();
        assert!(blank.contains("unknown LOCO_ADAPTER"), "{blank}");
        // Not trimmed. A surrounding space is an unknown adapter.
        let padded = parse(&[("LOCO_ADAPTER", " sqlite")]).unwrap_err();
        assert!(padded.contains("unknown LOCO_ADAPTER"), "{padded}");

        let auth = parse(&[("LOCO_AUTH_ADAPTER", "ldap")]).unwrap_err();
        assert_eq!(auth, "unknown LOCO_AUTH_ADAPTER: ldap (expected \"local\")");
        let auth_blank = parse(&[("LOCO_AUTH_ADAPTER", "")]).unwrap_err();
        assert!(
            auth_blank.contains("unknown LOCO_AUTH_ADAPTER"),
            "{auth_blank}"
        );
    }

    #[test]
    fn auto_create_accepts_one_and_true_only() {
        assert!(!auto_create(&parse(&[]).unwrap()));
        assert!(auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "1")]).unwrap()
        ));
        assert!(auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "true")]).unwrap()
        ));
        assert!(auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "TRUE")]).unwrap()
        ));
        assert!(!auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "0")]).unwrap()
        ));
        assert!(!auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "yes")]).unwrap()
        ));
        assert!(!auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", "")]).unwrap()
        ));
        // Not trimmed.
        assert!(!auto_create(
            &parse(&[("LOCO_AUTH_AUTO_CREATE", " 1")]).unwrap()
        ));
    }

    #[test]
    fn blank_default_site_is_unset_and_a_value_is_kept_raw() {
        assert!(parse(&[]).unwrap().default_site.is_none());
        assert!(parse(&[("LOCO_DEFAULT_SITE", "")])
            .unwrap()
            .default_site
            .is_none());
        assert!(parse(&[("LOCO_DEFAULT_SITE", "   ")])
            .unwrap()
            .default_site
            .is_none());
        assert_eq!(
            parse(&[("LOCO_DEFAULT_SITE", "alice/blog/www")])
                .unwrap()
                .default_site
                .as_deref(),
            Some("alice/blog/www")
        );
        assert_eq!(
            parse(&[("LOCO_DEFAULT_SITE", " alice/blog/www ")])
                .unwrap()
                .default_site
                .as_deref(),
            Some(" alice/blog/www ")
        );
    }

    #[test]
    fn secret_key_is_parsed_without_reading_the_process_environment() {
        assert!(matches!(parse(&[]).unwrap().secret_key, KeyStatus::Missing));
        assert!(matches!(
            parse(&[("LOCO_SECRET_KEY", "")]).unwrap().secret_key,
            KeyStatus::Invalid(_)
        ));
        let raw = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        match parse(&[("LOCO_SECRET_KEY", raw)]).unwrap().secret_key {
            KeyStatus::Ready(key) => assert_eq!(key, [0u8; 32]),
            other => panic!("expected a ready key, got {other:?}"),
        }
        let padded = format!("\n{raw}\n");
        match parse(&[("LOCO_SECRET_KEY", padded.as_str())])
            .unwrap()
            .secret_key
        {
            KeyStatus::Ready(key) => assert_eq!(key, [0u8; 32]),
            other => panic!("expected a ready key, got {other:?}"),
        }
    }
}
