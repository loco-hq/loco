//! Secret and variable values on a dataset.
//!
//! A list is the declarations of the versions this dataset's sites pin
//! (self and direct dependencies). A write is allowed when any version of
//! the project declares the name, itself or through a direct dependency —
//! a store can set a credential before a site pins the version that needs
//! it. The two rules are different on purpose.
//!
//! The reference a value is stored under is the same string a client puts
//! in the path: bare for this project, `{account}/{project}.{name}` for a
//! dependency ([`VersionSchema::reference`]).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::MutexGuard;

use serde::Serialize;

use super::project_config::lock_pins;
use super::project_config::ProjectConfig;
use super::version_schema::VersionSchema;
use crate::values::KeyStatus;
use crate::Secret;
use crate::Variable;

#[derive(Debug)]
pub enum ValueError {
    /// The name cannot be one path segment, or load would skip the file.
    InvalidName(String),
    /// No version of this project declares the name.
    Undeclared(String),
    UnknownDataset(String),
    /// DELETE of a value that is not set.
    NotFound(String),
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it.
    Unavailable(String),
    Schema(loco_schema_runtime::Error),
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(msg)
            | Self::Undeclared(msg)
            | Self::UnknownDataset(msg)
            | Self::NotFound(msg)
            | Self::Unavailable(msg) => write!(f, "{msg}"),
            Self::Schema(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ValueError {}

impl From<loco_schema_runtime::Error> for ValueError {
    fn from(err: loco_schema_runtime::Error) -> Self {
        match err {
            loco_schema_runtime::Error::InvalidPath(name) => {
                Self::InvalidName(format!("invalid config value name: {name}"))
            }
            other => Self::Schema(other),
        }
    }
}

/// One row of `GET /config/secret/.../list`, and the body of a secret `PUT`.
/// There is no `value` field.
#[derive(Debug, Serialize)]
pub struct SecretValueView {
    pub name: String,
    pub project: String,
    pub set: bool,
    pub updated_at: Option<String>,
}

/// One row of `GET /config/variable/.../list`, and the body of a variable `PUT`.
///
/// `source` is `value` when the dataset set it and `default` when the row is
/// the declaration's default. Absent when neither is available. `value` is
/// null in that last case.
#[derive(Debug, Serialize)]
pub struct VariableValueView {
    pub name: String,
    pub project: String,
    pub set: bool,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    pub updated_at: Option<String>,
}

#[derive(Clone)]
struct Decl {
    project: String,
    name: String,
    /// First non-empty default among the pinned versions, in version-name
    /// order. Secrets leave this empty.
    default_value: Option<String>,
}

#[derive(Clone, Copy)]
enum Kind {
    Secret,
    Variable,
}

impl Kind {
    fn noun(&self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Variable => "variable",
        }
    }
}

/// How a client names `name` owned by `owner` from `project_id`. Same rule
/// as [`VersionSchema::reference`].
fn reference(project_id: &str, owner: &str, name: &str) -> String {
    if owner == project_id {
        name.to_string()
    } else {
        format!("{owner}.{name}")
    }
}

impl ProjectConfig {
    pub fn list_secret_values(&self, dataset: &str) -> Result<Vec<SecretValueView>, ValueError> {
        self.require_dataset(dataset)?;
        let mut rows = Vec::new();
        for decl in self.pinned_declarations(dataset, Kind::Secret) {
            let stored_as = reference(&self.project_id(), &decl.project, &decl.name);
            let updated_at = self
                .values
                .secret(&self.project_id(), dataset, &stored_as)
                .map(|record| record.updated_at().to_string());
            rows.push(SecretValueView {
                name: decl.name,
                project: decl.project,
                set: updated_at.is_some(),
                updated_at,
            });
        }
        Ok(rows)
    }

    pub fn list_variable_values(
        &self,
        dataset: &str,
    ) -> Result<Vec<VariableValueView>, ValueError> {
        self.require_dataset(dataset)?;
        let mut rows = Vec::new();
        for decl in self.pinned_declarations(dataset, Kind::Variable) {
            let stored_as = reference(&self.project_id(), &decl.project, &decl.name);
            let stored = self
                .values
                .variable(&self.project_id(), dataset, &stored_as);
            let (value, source, set, updated_at) = if let Some(record) = stored {
                (
                    Some(record.value().to_string()),
                    Some("value"),
                    true,
                    Some(record.updated_at().to_string()),
                )
            } else if let Some(default) = decl.default_value.clone() {
                (Some(default), Some("default"), false, None)
            } else {
                (None, None, false, None)
            };
            rows.push(VariableValueView {
                name: decl.name,
                project: decl.project,
                set,
                value,
                source,
                updated_at,
            });
        }
        Ok(rows)
    }

    pub fn set_secret_value(
        &self,
        dataset: &str,
        name: &str,
        plaintext: &str,
        key: &KeyStatus,
    ) -> Result<SecretValueView, ValueError> {
        let key = key.require().map_err(ValueError::Unavailable)?;
        // `PINS` across the check and the write, so a dataset delete either
        // finishes first (this finds the dataset gone) or runs after and
        // sweeps the value. Lock order is `PINS`, then the value writer.
        let _pins = self.pin_dataset(dataset)?;
        let decl = self.require_declaration(name, Kind::Secret)?;
        let record = self
            .values
            .put_secret(&self.project_id(), dataset, name, plaintext, key)?;
        Ok(SecretValueView {
            name: decl.name,
            project: decl.project,
            set: true,
            updated_at: Some(record.updated_at().to_string()),
        })
    }

    pub fn delete_secret_value(&self, dataset: &str, name: &str) -> Result<(), ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        // A value whose declaration was removed since the write is still
        // deleted: the declaration check would make that orphan permanent
        // until the dataset itself went.
        self.delete_stored(dataset, name, true)
    }

    pub fn set_variable_value(
        &self,
        dataset: &str,
        name: &str,
        value: &str,
    ) -> Result<VariableValueView, ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        let decl = self.require_declaration(name, Kind::Variable)?;
        let record = self
            .values
            .put_variable(&self.project_id(), dataset, name, value)?;
        Ok(VariableValueView {
            name: decl.name,
            project: decl.project,
            set: true,
            value: Some(record.value().to_string()),
            source: Some("value"),
            updated_at: Some(record.updated_at().to_string()),
        })
    }

    pub fn delete_variable_value(&self, dataset: &str, name: &str) -> Result<(), ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        self.delete_stored(dataset, name, false)
    }

    fn pin_dataset(&self, dataset: &str) -> Result<MutexGuard<'static, ()>, ValueError> {
        let pins = lock_pins();
        self.require_dataset(dataset)?;
        Ok(pins)
    }

    fn require_dataset(&self, dataset: &str) -> Result<(), ValueError> {
        if self.dataset(dataset).is_none() {
            return Err(ValueError::UnknownDataset(format!(
                "dataset not found: {}/{dataset}",
                self.project_id()
            )));
        }
        Ok(())
    }

    fn require_declaration(&self, name: &str, kind: Kind) -> Result<Decl, ValueError> {
        self.find_declaration(name, kind).ok_or_else(|| {
            ValueError::Undeclared(format!(
                "{noun} '{name}' is not declared by {} or a direct dependency of its versions",
                self.project_id(),
                noun = kind.noun(),
            ))
        })
    }

    fn delete_stored(&self, dataset: &str, name: &str, secret: bool) -> Result<(), ValueError> {
        let result = if secret {
            self.values.delete_secret(&self.project_id(), dataset, name)
        } else {
            self.values
                .delete_variable(&self.project_id(), dataset, name)
        };
        match result {
            Ok(()) => Ok(()),
            Err(loco_schema_runtime::Error::NotFound(_)) => {
                let noun = if secret { "secret" } else { "variable" };
                Err(ValueError::NotFound(format!(
                    "{noun} value not found: {name}"
                )))
            }
            Err(err) => Err(err.into()),
        }
    }

    /// The declaration `name` resolves to on any version of this project.
    fn find_declaration(&self, name: &str, kind: Kind) -> Option<Decl> {
        let mut versions: Vec<String> = self
            .manifests()
            .into_iter()
            .map(|(_, manifest)| manifest.version().to_string())
            .collect();
        versions.sort();
        for version in versions {
            let view = VersionSchema::new_read_only(self.store.clone(), self.project_id(), version);
            let found = match kind {
                Kind::Secret => view.secret(name).map(|secret| decl_from_secret(&secret)),
                Kind::Variable => view
                    .variable(name)
                    .map(|variable| decl_from_variable(&variable)),
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Declarations visible to the versions this dataset's sites pin, one
    /// row per `(owner project, bare name)`, ordered by that pair. A default
    /// is the first non-empty one in version-name order.
    fn pinned_declarations(&self, dataset: &str, kind: Kind) -> Vec<Decl> {
        let versions: BTreeSet<String> = self
            .sites()
            .into_iter()
            .filter(|(_, site)| site.dataset == dataset)
            .map(|(_, site)| site.version.clone())
            .collect();
        let mut rows: BTreeMap<(String, String), Decl> = BTreeMap::new();
        for version in versions {
            let view = VersionSchema::new_read_only(self.store.clone(), self.project_id(), version);
            match kind {
                Kind::Secret => {
                    for secret in view.secrets() {
                        let key = (secret.project().to_string(), secret.name().to_string());
                        rows.entry(key).or_insert_with(|| decl_from_secret(&secret));
                    }
                }
                Kind::Variable => {
                    for variable in view.variables() {
                        let key = (variable.project().to_string(), variable.name().to_string());
                        let default = non_empty(variable.default());
                        rows.entry(key)
                            .and_modify(|row| {
                                if row.default_value.is_none() {
                                    row.default_value.clone_from(&default);
                                }
                            })
                            .or_insert_with(|| Decl {
                                project: variable.project().to_string(),
                                name: variable.name().to_string(),
                                default_value: default.clone(),
                            });
                    }
                }
            }
        }
        rows.into_values().collect()
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn decl_from_secret(secret: &Secret) -> Decl {
    Decl {
        project: secret.project().to_string(),
        name: secret.name().to_string(),
        default_value: None,
    }
}

fn decl_from_variable(variable: &Variable) -> Decl {
    Decl {
        project: variable.project().to_string(),
        name: variable.name().to_string(),
        default_value: non_empty(variable.default()),
    }
}
