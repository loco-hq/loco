//! Secret and variable values on a dataset.
//!
//! A list is the declarations of the versions this dataset's sites pin
//! (self and direct dependencies). A write is allowed when any version of
//! the project declares the name, itself or through a direct dependency —
//! a store can set a credential before a site pins the version that needs
//! it. The two rules are different on purpose.
//!
//! The row id of a loose declaration is its canonical reference
//! ([`reference`]): bare when the declaration belongs to this project,
//! `{account}/{project}.{name}` for a dependency. A qualified name for this
//! project (`alice/shop.label_prefix`) is the same row as the bare name.
//! A connection value is `{qualified integration}:{name}` (`sf_east:token`,
//! `alice/pkg.store:consumer_key`). A name with `:` is that form and never
//! a loose declaration. `DELETE` skips the declaration lookup, so it only
//! strips a `{project}.` prefix — `ben/sync.sf_east:token` becomes
//! `sf_east:token` — and otherwise uses the path string as the id, `:`
//! included.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::MutexGuard;

use serde::Serialize;

use loco_lake::DataAdapter;

use super::project_config::lock_pins;
use super::project_config::ProjectConfig;
use super::version_schema::parse_address;
use super::version_schema::ConnectionDeclarations;
use super::version_schema::VersionSchema;
use crate::values::check_name;
use crate::values::delete_variable;
use crate::values::list_variables;
use crate::values::put_variable;
use crate::values::SecretError;
use crate::values::SecretStore;
use crate::Secret;
use crate::Variable;

#[derive(Debug)]
pub enum ValueError {
    /// Empty, or a NUL, which the lake would cut short.
    InvalidName(String),
    /// No version of this project declares the name.
    Undeclared(String),
    UnknownDataset(String),
    /// DELETE of a value that is not set.
    NotFound(String),
    /// `LOCO_SECRET_KEY` is missing or malformed. The string names it.
    Unavailable(String),
    Lake(loco_lake::Error),
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(msg)
            | Self::Undeclared(msg)
            | Self::UnknownDataset(msg)
            | Self::NotFound(msg)
            | Self::Unavailable(msg) => write!(f, "{msg}"),
            Self::Lake(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ValueError {}

/// One row of `GET /config/secret/.../list`, and the body of a secret `PUT`.
/// There is no `value` field.
#[derive(Debug, Serialize)]
pub struct SecretValueView {
    pub name: String,
    pub project: String,
    /// Canonical qualified integration. Absent on a loose declaration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
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
    /// Canonical qualified integration. Absent on a loose declaration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
    pub set: bool,
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    pub updated_at: Option<String>,
}

#[derive(Clone)]
struct Decl {
    /// Who declared the integration, or the loose declaration's project.
    project: String,
    name: String,
    /// Canonical qualified integration. `None` for a loose declaration.
    integration: Option<String>,
    /// First non-empty default among the pinned versions, in version-name
    /// order. Secrets leave this empty.
    default_value: Option<String>,
}

/// Loose rows sort before connection rows, so an existing list of only loose
/// declarations keeps its order. Within a kind, the pair is the order.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum DeclKey {
    Loose { project: String, name: String },
    Connection { integration: String, name: String },
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

/// How a client names a loose `name` owned by `owner` from `project_id`. Same
/// rule as [`VersionSchema::reference`]. This is the row id a loose write stores.
fn reference(project_id: &str, owner: &str, name: &str) -> String {
    if owner == project_id {
        name.to_string()
    } else {
        format!("{owner}.{name}")
    }
}

/// Row id a write stores. A connection value is `{integration}:{name}`.
/// A loose value is [`reference`].
fn value_id(project_id: &str, decl: &Decl) -> String {
    match &decl.integration {
        Some(integration) => format!("{integration}:{}", decl.name),
        None => reference(project_id, &decl.project, &decl.name),
    }
}

/// Row id for a delete. There is no declaration to resolve: a `{project_id}.`
/// prefix is a qualified name for this project and means the bare name.
/// Anything else, including a dependency's `{owner}.{name}` and a connection
/// id (`sf_east:token`, `alice/pkg.store:consumer_key`), is already the stored
/// id. The prefix strip leaves the colon in place, so
/// `{project}.sf_east:token` deletes `sf_east:token`.
fn canonical_delete_name(project_id: &str, name: &str) -> String {
    let prefix = format!("{project_id}.");
    name.strip_prefix(&prefix).unwrap_or(name).to_string()
}

impl ProjectConfig {
    fn dataset_id(&self, dataset: &str) -> String {
        format!("{}/{dataset}", self.project_id())
    }

    pub fn list_secret_values(
        &self,
        secrets: &dyn SecretStore,
        dataset: &str,
    ) -> Result<Vec<SecretValueView>, ValueError> {
        self.require_dataset(dataset)?;
        let stored = secrets
            .list(&self.dataset_id(dataset))
            .map_err(secret_err)?;
        let mut rows = Vec::new();
        for decl in self.pinned_declarations(dataset, Kind::Secret) {
            let stored_as = value_id(&self.project_id(), &decl);
            let updated_at = stored
                .iter()
                .find(|row| row.name == stored_as)
                .map(|row| row.updated_at.clone());
            rows.push(SecretValueView {
                name: decl.name,
                project: decl.project,
                integration: decl.integration,
                set: updated_at.is_some(),
                updated_at,
            });
        }
        Ok(rows)
    }

    pub fn list_variable_values(
        &self,
        data: &dyn DataAdapter,
        dataset: &str,
    ) -> Result<Vec<VariableValueView>, ValueError> {
        self.require_dataset(dataset)?;
        let stored = list_variables(data, &self.dataset_id(dataset)).map_err(ValueError::Lake)?;
        let mut rows = Vec::new();
        for decl in self.pinned_declarations(dataset, Kind::Variable) {
            let stored_as = value_id(&self.project_id(), &decl);
            let (value, source, set, updated_at) =
                if let Some(record) = stored.iter().find(|row| row.name == stored_as) {
                    (
                        Some(record.value.clone()),
                        Some("value"),
                        true,
                        Some(record.updated_at.clone()),
                    )
                } else if let Some(default) = decl.default_value.clone() {
                    (Some(default), Some("default"), false, None)
                } else {
                    (None, None, false, None)
                };
            rows.push(VariableValueView {
                name: decl.name,
                project: decl.project,
                integration: decl.integration,
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
        secrets: &dyn SecretStore,
        dataset: &str,
        name: &str,
        plaintext: &str,
    ) -> Result<SecretValueView, ValueError> {
        // `PINS` across the check and the write, so a dataset delete either
        // finishes first (this finds the dataset gone) or runs after and
        // sweeps the row. Lock order is `PINS`, then the lake adapter.
        let _pins = self.pin_dataset(dataset)?;
        let decl = self.require_declaration(name, Kind::Secret)?;
        // The path may be the bare name, `{self}.{name}`, or
        // `{integration}:{name}` (including `{self}.{integration}:{name}`).
        // Those qualified-self forms are this row. The ciphertext's
        // additional data is this canonical name.
        let stored_as = value_id(&self.project_id(), &decl);
        let record = secrets
            .put(&self.dataset_id(dataset), &stored_as, plaintext)
            .map_err(secret_err)?;
        Ok(SecretValueView {
            name: decl.name,
            project: decl.project,
            integration: decl.integration,
            set: true,
            updated_at: Some(record.updated_at),
        })
    }

    pub fn delete_secret_value(
        &self,
        secrets: &dyn SecretStore,
        dataset: &str,
        name: &str,
    ) -> Result<(), ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        // A value whose declaration was removed since the write is still
        // deleted: the declaration check would make that orphan permanent
        // until the dataset itself went. A `{self}.` prefix is stripped so
        // this still finds the row a qualified self name wrote.
        let stored_as = canonical_delete_name(&self.project_id(), name);
        check_name(&stored_as).map_err(ValueError::InvalidName)?;
        match secrets.delete(&self.dataset_id(dataset), &stored_as) {
            Ok(()) => Ok(()),
            Err(SecretError::NotFound) => Err(ValueError::NotFound(format!(
                "secret value not found: {name}"
            ))),
            Err(err) => Err(secret_err(err)),
        }
    }

    pub fn set_variable_value(
        &self,
        data: &dyn DataAdapter,
        dataset: &str,
        name: &str,
        value: &str,
    ) -> Result<VariableValueView, ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        let decl = self.require_declaration(name, Kind::Variable)?;
        let stored_as = value_id(&self.project_id(), &decl);
        let record = put_variable(data, &self.dataset_id(dataset), &stored_as, value)
            .map_err(|err| lake_write_err(err, "variable", name))?;
        Ok(VariableValueView {
            name: decl.name,
            project: decl.project,
            integration: decl.integration,
            set: true,
            value: Some(record.value),
            source: Some("value"),
            updated_at: Some(record.updated_at),
        })
    }

    pub fn delete_variable_value(
        &self,
        data: &dyn DataAdapter,
        dataset: &str,
        name: &str,
    ) -> Result<(), ValueError> {
        let _pins = self.pin_dataset(dataset)?;
        let stored_as = canonical_delete_name(&self.project_id(), name);
        check_name(&stored_as).map_err(ValueError::InvalidName)?;
        delete_variable(data, &self.dataset_id(dataset), &stored_as)
            .map_err(|err| lake_write_err(err, "variable", name))
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

    /// The declaration `name` resolves to on any version of this project.
    ///
    /// A `:` is a connection value: the integration must be visible on that
    /// version (this project or a direct dependency) and its type must declare
    /// `name`. That path does not fall through to a loose declaration.
    fn find_declaration(&self, name: &str, kind: Kind) -> Option<Decl> {
        if parse_address(name).integration.is_some() {
            return self.find_connection_declaration(name, kind);
        }
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

    /// The connection declaration `name` (`{integration}:{decl}`, or qualified)
    /// resolves to. The first version in name order that sees the integration
    /// and whose type declares the name wins. A qualified name for this
    /// project's own integration stores as the bare integration id.
    fn find_connection_declaration(&self, name: &str, kind: Kind) -> Option<Decl> {
        let project_id = self.project_id();
        let parsed = parse_address(name);
        let integration_name = parsed.integration?;
        let decl_name = parsed.name.to_string();
        let integration_project = parsed.project.unwrap_or(project_id.as_str()).to_string();
        let mut versions: Vec<String> = self
            .manifests()
            .into_iter()
            .map(|(_, manifest)| manifest.version().to_string())
            .collect();
        versions.sort();
        for version in versions {
            let view = VersionSchema::new_read_only(self.store.clone(), &project_id, version);
            // A version that does not see the integration, or whose type is
            // gone, is skipped. A later version can still declare it.
            let Some(spec) = view.connection_declarations(&integration_project, integration_name)
            else {
                continue;
            };
            let found = connection_decl(&spec, &decl_name, kind);
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Declarations visible to the versions this dataset's sites pin. Loose
    /// rows come first, one per `(owner project, bare name)`, then one row
    /// per integration per declaration. A default is the first non-empty one
    /// in version-name order.
    fn pinned_declarations(&self, dataset: &str, kind: Kind) -> Vec<Decl> {
        let versions: BTreeSet<String> = self
            .sites()
            .into_iter()
            .filter(|(_, site)| site.dataset == dataset)
            .map(|(_, site)| site.version.clone())
            .collect();
        let mut rows: BTreeMap<DeclKey, Decl> = BTreeMap::new();
        for version in versions {
            let view = VersionSchema::new_read_only(self.store.clone(), self.project_id(), version);
            match kind {
                Kind::Secret => {
                    for secret in view.secrets() {
                        let key = DeclKey::Loose {
                            project: secret.project().to_string(),
                            name: secret.name().to_string(),
                        };
                        rows.entry(key).or_insert_with(|| decl_from_secret(&secret));
                    }
                }
                Kind::Variable => {
                    for variable in view.variables() {
                        let key = DeclKey::Loose {
                            project: variable.project().to_string(),
                            name: variable.name().to_string(),
                        };
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
                                integration: None,
                                default_value: default.clone(),
                            });
                    }
                }
            }
            for integration in view.integrations() {
                let Some(spec) =
                    view.connection_declarations(integration.project(), integration.name())
                else {
                    continue;
                };
                remember_connection(&mut rows, &spec, kind);
            }
        }
        rows.into_values().collect()
    }
}

fn secret_err(err: SecretError) -> ValueError {
    match err {
        SecretError::Unavailable(msg) => ValueError::Unavailable(msg),
        SecretError::InvalidName(msg) => ValueError::InvalidName(msg),
        SecretError::NotFound => ValueError::NotFound("secret value not found".to_string()),
        SecretError::Failed(msg) => ValueError::Lake(loco_lake::Error::Internal(msg)),
        SecretError::Lake(err) => ValueError::Lake(err),
    }
}

/// A lake `Internal` whose text is `invalid config value name` is the name
/// check inside the variable helpers. Everything else is a real lake error.
/// `NotFound` is the missing-value 404.
fn lake_write_err(err: loco_lake::Error, noun: &str, name: &str) -> ValueError {
    match err {
        loco_lake::Error::NotFound => {
            ValueError::NotFound(format!("{noun} value not found: {name}"))
        }
        loco_lake::Error::Internal(msg) if msg.starts_with("invalid config value name") => {
            ValueError::InvalidName(msg)
        }
        other => ValueError::Lake(other),
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
        integration: None,
        default_value: None,
    }
}

fn decl_from_variable(variable: &Variable) -> Decl {
    Decl {
        project: variable.project().to_string(),
        name: variable.name().to_string(),
        integration: None,
        default_value: non_empty(variable.default()),
    }
}

fn connection_decl(spec: &ConnectionDeclarations, name: &str, kind: Kind) -> Option<Decl> {
    match kind {
        Kind::Secret => {
            let secret = spec.secrets.iter().find(|secret| secret.name == name)?;
            Some(Decl {
                project: spec.project.clone(),
                name: secret.name.clone(),
                integration: Some(spec.integration.clone()),
                default_value: None,
            })
        }
        Kind::Variable => {
            let variable = spec
                .variables
                .iter()
                .find(|variable| variable.name == name)?;
            Some(Decl {
                project: spec.project.clone(),
                name: variable.name.clone(),
                integration: Some(spec.integration.clone()),
                default_value: non_empty(&variable.default_value),
            })
        }
    }
}

fn remember_connection(
    rows: &mut BTreeMap<DeclKey, Decl>,
    spec: &ConnectionDeclarations,
    kind: Kind,
) {
    match kind {
        Kind::Secret => {
            for secret in &spec.secrets {
                let key = DeclKey::Connection {
                    integration: spec.integration.clone(),
                    name: secret.name.clone(),
                };
                rows.entry(key).or_insert_with(|| Decl {
                    project: spec.project.clone(),
                    name: secret.name.clone(),
                    integration: Some(spec.integration.clone()),
                    default_value: None,
                });
            }
        }
        Kind::Variable => {
            for variable in &spec.variables {
                let key = DeclKey::Connection {
                    integration: spec.integration.clone(),
                    name: variable.name.clone(),
                };
                let default = non_empty(&variable.default_value);
                rows.entry(key)
                    .and_modify(|row| {
                        if row.default_value.is_none() {
                            row.default_value.clone_from(&default);
                        }
                    })
                    .or_insert_with(|| Decl {
                        project: spec.project.clone(),
                        name: variable.name.clone(),
                        integration: Some(spec.integration.clone()),
                        default_value: default.clone(),
                    });
            }
        }
    }
}
