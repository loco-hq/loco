//! Per-version, scoped view over the global `SchemaStore`.
//!
//! A `VersionSchema` is built around a `(project_id, version)` pair plus the
//! direct dependencies declared in that version's manifest. It can:
//!
//! - **read** any metadata in its own version OR a directly-declared
//!   dependency. Transitive deps are not visible — to use a piece of
//!   metadata, the project must depend on its owner directly. A bare name
//!   means this version's own project; a dependency's must be written
//!   `{account}/{project}.{name}` ("Name resolution" in CLAUDE.md).
//! - **write** to its own `(project_id, version)` *only* when constructed
//!   writable AND the version is a draft (`-dev` suffix) AND it exists — has
//!   a manifest, which only `/config` creates. Each write holds `PINS` from
//!   that check to its last store write, so a version delete cannot slip in
//!   between and leave the write's files behind as an orphan tree.
//!
//! The dep set — and the public permission-set assignment that sits beside it
//! on the manifest — is snapshotted at construction so a request gets a
//! coherent view even if a concurrent write mutates the manifest mid-flight.
//!
//! Two construction modes:
//! - [`VersionSchema::new`] — writable. Used by `VersionScope` for `/schema`
//!   writes (developer + draft).
//! - [`VersionSchema::new_read_only`] — read-only. Used by `SiteScope` (data
//!   routes) and `VersionReadScope` (GET `/schema`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, MutexGuard};

use serde::Serialize;

use crate::http::authz::is_draft_version;
use crate::http::project_config::lock_pins;
use crate::validation::FIELD_TYPES;
use crate::{
    Action, ActionParam, ActionUpdate, Bundle, Collection, CollectionUpdate, Field, FieldUpdate,
    Fieldset, FieldsetUpdate, Integration, IntegrationAction, IntegrationCollection,
    IntegrationSecret, IntegrationType, IntegrationTypeUpdate, IntegrationUpdate,
    IntegrationVariable, Manifest, ManifestUpdate, PermissionSet, PermissionSetUpdate, SchemaStore,
    Secret, SecretUpdate, Variable, VariableUpdate,
};

/// Name of the fieldset auto-created when a collection is created. The boolean
/// `auto_add` flag — not this name — is what marks a fieldset as "new fields
/// land here"; the name is just a sensible default for the first one.
const DEFAULT_FIELDSET_NAME: &str = "default";

#[derive(Debug)]
pub enum VersionSchemaError {
    /// Writes refused because this schema was constructed read-only OR the
    /// version is published. The message distinguishes the two cases.
    NotWritable(String),
    /// A manifest `dependencies` entry is malformed, names this project,
    /// repeats a project, or names a version that does not exist.
    InvalidDependency(String),
    /// A field `type` outside [`crate::validation::FIELD_TYPES`].
    InvalidFieldType(String),
    /// A secret or variable declaration the version cannot store: a secret
    /// body carrying a value, or a secret and a variable sharing a name.
    InvalidDeclaration(String),
    /// A collection name outside the slug charset. `$` is reserved for the
    /// lake collections that hold secret and variable values.
    InvalidName(String),
    /// The version has no manifest: it was never created through `/config`,
    /// or it has since been deleted.
    UnknownVersion(String),
    /// `delete_collection` attempted its child prefixes.
    /// These keys are still present. The parent record was left in place, so
    /// a recreate cannot inherit a leftover child and the delete can be
    /// retried.
    LeftBehind(Vec<String>),
    Schema(loco_schema_runtime::Error),
}

impl std::fmt::Display for VersionSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotWritable(msg)
            | Self::InvalidDependency(msg)
            | Self::InvalidFieldType(msg)
            | Self::InvalidDeclaration(msg)
            | Self::InvalidName(msg)
            | Self::UnknownVersion(msg) => write!(f, "{msg}"),
            Self::LeftBehind(keys) => write!(f, "delete left behind: {}", keys.join(", ")),
            Self::Schema(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VersionSchemaError {}

impl From<loco_schema_runtime::Error> for VersionSchemaError {
    fn from(e: loco_schema_runtime::Error) -> Self {
        Self::Schema(e)
    }
}

#[derive(Clone)]
pub struct VersionSchema {
    store: Arc<SchemaStore>,
    project_id: String,
    version: String,
    /// Self entry plus direct deps as `(project_id, version)` pairs. Self is
    /// always index 0. Lookups find a project here by name; none walks the
    /// list for a match, so its order carries no meaning.
    dependencies: Vec<(String, String)>,
    /// Names of the permission sets this version's manifest assigns to
    /// `public`, snapshotted with `dependencies` for the same reason.
    ///
    /// Assignment lives on the manifest, not on the site: a site is a URL
    /// pointing at `(version, dataset)`, so two sites pinning one version
    /// share its public policy. Names resolve through
    /// [`Self::permission_set`]: a bare name is this project's own set, and a
    /// consuming version opts into a set a dependency ships by naming it
    /// qualified (`acme/crm.public_contacts`).
    public_permission_sets: Vec<String>,
    read_only: bool,
}

/// One collection a caller can address. Ordinary lake collections stay one
/// row per document. A standard collection is one row per integration that
/// exposes it. `project` is the address root (who declared the integration,
/// or who owns the ordinary collection). `owner` is who owns the fields.
#[derive(Debug, Clone)]
pub struct CollectionAddress {
    pub project: String,
    pub version: String,
    /// `sf_east:account`, or the bare collection name when there is no integration.
    pub local: String,
    pub integration: Option<String>,
    pub name: String,
    pub owner: String,
    pub label: String,
    pub label_plural: String,
    pub kind: CollectionAddressKind,
}

#[derive(Debug, Clone)]
pub enum CollectionAddressKind {
    Ordinary,
    Standard {
        type_project: String,
        type_version: String,
        type_name: String,
    },
    Custom,
    /// Both the type and the integration declare this name. `project` and
    /// `local` are the address; neither document is chosen. `lake_key` is
    /// `None`, and the handler answers 409 after the access check.
    Ambiguous,
}

/// Wire row for `GET /schema/.../collection/list`. Ordinary rows keep the
/// collection document's fields and add `owner`. `integration` is omitted
/// when the address is an ordinary collection.
#[derive(Debug, Clone, Serialize)]
pub struct CollectionListingRow {
    pub project: String,
    pub version: String,
    pub name: String,
    pub label: String,
    pub label_plural: String,
    pub owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
}

/// One action a caller can address. Same columns as a collection address.
/// `owner` is the type's project for a type action, and the action's project
/// for an ordinary action. An action is resolved or missing, never ambiguous.
#[derive(Debug, Clone)]
pub struct ActionAddress {
    pub project: String,
    pub version: String,
    pub local: String,
    pub integration: Option<String>,
    pub name: String,
    pub owner: String,
    pub label: String,
    pub description: String,
    pub params: serde_json::Value,
    pub kind: ActionAddressKind,
}

#[derive(Debug, Clone)]
pub enum ActionAddressKind {
    Ordinary,
    Type {
        type_project: String,
        type_name: String,
        action: Arc<IntegrationAction>,
    },
}

/// Secrets and variables one integration's type declares, as the view that
/// asked for them names that integration.
///
/// A connection reads only this list. A loose secret or variable of the same
/// bare name is a different row and is not included.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionDeclarations {
    /// Canonical qualified integration from the asking view's project.
    /// Bare for this project's integration (`sf_east`),
    /// `{account}/{project}.{name}` for a dependency's
    /// (`loco/bricklink.store`).
    pub integration: String,
    /// Project that declared the integration. The address root.
    pub project: String,
    /// Canonical type reference from the asking view. Bare when this version
    /// declares the type (`warehouse`), `{account}/{project}.{name}` when a
    /// dependency does (`alice/pkg.warehouse`).
    pub type_ref: String,
    pub secrets: Vec<ConnectionSecretDecl>,
    pub variables: Vec<ConnectionVariableDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConnectionSecretDecl {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ConnectionVariableDecl {
    pub name: String,
    pub required: bool,
    /// The type's default. Empty means none.
    pub default_value: String,
}

/// Wire row for `GET /actions` and `GET /actions/{name}`. Params stay in
/// declared order. `integration` is omitted on an ordinary action.
#[derive(Debug, Clone, Serialize)]
pub struct ActionListingRow {
    pub project: String,
    pub version: String,
    pub name: String,
    pub label: String,
    pub description: String,
    pub owner: String,
    pub params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
}

impl CollectionAddress {
    fn ordinary(collection: Arc<Collection>) -> Self {
        Self {
            project: collection.project().to_string(),
            version: collection.version().to_string(),
            local: collection.name().to_string(),
            integration: None,
            name: collection.name().to_string(),
            owner: collection.project().to_string(),
            label: collection.label().to_string(),
            label_plural: collection.label_plural().to_string(),
            kind: CollectionAddressKind::Ordinary,
        }
    }

    /// A collision. The address root and the local are known. Labels stay
    /// empty because neither document is the one this address names.
    fn ambiguous(project: &str, version: &str, integration: &str, name: &str) -> Self {
        Self {
            project: project.to_string(),
            version: version.to_string(),
            local: format!("{integration}:{name}"),
            integration: Some(integration.to_string()),
            name: name.to_string(),
            owner: project.to_string(),
            label: String::new(),
            label_plural: String::new(),
            kind: CollectionAddressKind::Ambiguous,
        }
    }

    pub fn listing_row(&self) -> CollectionListingRow {
        CollectionListingRow {
            project: self.project.clone(),
            version: self.version.clone(),
            name: self.name.clone(),
            label: self.label.clone(),
            label_plural: self.label_plural.clone(),
            owner: self.owner.clone(),
            integration: self.integration.clone(),
        }
    }
}

/// A name resolved to one address, or it did not. A collection both the type
/// and the integration declare is [`CollectionAddressKind::Ambiguous`]: the
/// address resolved, and the data handler reports that after the access
/// check. An action is resolved or missing, and never ambiguous.
#[derive(Debug)]
pub enum AddressResolution<T> {
    Resolved(T),
    Missing,
}

/// Read-path refusal. `name` is the address the caller wrote, qualified or bare.
pub(crate) fn ambiguous_address_message(name: &str) -> String {
    format!("ambiguous address {name}: the type offers it and the integration declares it")
}

/// Write-path refusal. Names the type document and the integration document.
fn address_collision(integration: &str, name: &str, type_ref: &str) -> String {
    format!(
        "ambiguous address {integration}:{name}: type '{type_ref}' offers collection '{name}' and integration '{integration}' declares it"
    )
}

impl ActionAddress {
    fn ordinary(action: Arc<Action>) -> Self {
        Self {
            project: action.project().to_string(),
            version: action.version().to_string(),
            local: action.name().to_string(),
            integration: None,
            name: action.name().to_string(),
            owner: action.project().to_string(),
            label: action.label().to_string(),
            description: action.description().to_string(),
            params: json_value(action.params()),
            kind: ActionAddressKind::Ordinary,
        }
    }

    pub fn listing_row(&self) -> ActionListingRow {
        ActionListingRow {
            project: self.project.clone(),
            version: self.version.clone(),
            name: self.name.clone(),
            label: self.label.clone(),
            description: self.description.clone(),
            owner: self.owner.clone(),
            params: self.params.clone(),
            integration: self.integration.clone(),
        }
    }
}

impl VersionSchema {
    /// Build a writable scoped view. Writes are still gated on the version
    /// being a draft (`-dev` suffix); see [`Self::require_writable`].
    pub fn new(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self::build(store, project_id, version, false)
    }

    /// Build a read-only scoped view. All write methods will return
    /// `VersionSchemaError::NotWritable` regardless of draft state.
    pub fn new_read_only(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self::build(store, project_id, version, true)
    }

    fn build(
        store: Arc<SchemaStore>,
        project_id: impl Into<String>,
        version: impl Into<String>,
        read_only: bool,
    ) -> Self {
        let project_id = project_id.into();
        let version = version.into();
        let manifest = store
            .manifests()
            .get(&Manifest::to_path(&project_id, &version));
        let dependencies = direct_dependencies(&project_id, &version, manifest.as_deref());
        let public_permission_sets = manifest
            .map(|m| m.public_permission_sets().to_vec())
            .unwrap_or_default();
        Self {
            store,
            project_id,
            version,
            dependencies,
            public_permission_sets,
            read_only,
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn dependencies(&self) -> &[(String, String)] {
        &self.dependencies
    }

    /// Permission-set names this version's manifest assigns to `public`.
    /// Snapshotted at construction; unknown names stay inert because
    /// [`Self::permission_set`] simply won't resolve them.
    pub fn public_permission_sets(&self) -> &[String] {
        &self.public_permission_sets
    }

    // --- Reads: self + direct dependencies, resolved strictly ---
    //
    // Every lookup below takes a name as a client writes it and resolves it
    // by the rule in CLAUDE.md ("Name resolution"): a bare name means this
    // version's own project, and a dependency's must be written
    // `{account}/{project}.{name}`. Nothing walks the dependency list looking
    // for a match, so installing a dependency never changes what a bare name
    // means, and two dependencies that share a name are both addressable.

    /// Manifest for `(self.project_id, self.version)`. Manifests are
    /// inherently per-version; there's no cross-namespace flavor.
    pub fn manifest(&self) -> Option<Arc<Manifest>> {
        self.store
            .manifests()
            .get(&Manifest::to_path(&self.project_id, &self.version))
    }

    /// The version of `project` this view sees: the running version when
    /// `project` is self, the pinned version when it is a **direct**
    /// dependency, otherwise `None`.
    pub fn visible_version(&self, project: &str) -> Option<&str> {
        self.dependencies
            .iter()
            .find(|(p, _)| p == project)
            .map(|(_, v)| v.as_str())
    }

    /// `name` as `(owning project, bare name)`: this project for a bare name,
    /// the named project for a qualified one. The project need not be visible
    /// — see [`Self::resolve`] for that.
    pub fn split<'a>(&'a self, name: &'a str) -> (&'a str, &'a str) {
        split_qualified(name).unwrap_or((&self.project_id, name))
    }

    /// `name` as `(project, version, bare name)` when its project is self or
    /// a direct dependency; `None` otherwise (a transitive or unknown
    /// project is not found, never an error of another kind).
    fn resolve<'a>(&'a self, name: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
        let (project, bare) = self.split(name);
        let version = self.visible_version(project)?;
        Some((project, version, bare))
    }

    /// How a client names `name` owned by `project` from this version: bare
    /// for this project's own, qualified for a dependency's.
    pub fn reference(&self, project: &str, name: &str) -> String {
        if project == self.project_id {
            name.to_string()
        } else {
            format!("{project}.{name}")
        }
    }

    /// Every collection visible to this version, across self + direct deps.
    /// Each carries its `project`, from which a client builds the qualified
    /// name of a dependency's.
    pub fn collections(&self) -> Vec<Arc<Collection>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/collections/");
                self.store
                    .collections()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, c)| c)
            })
            .collect()
    }

    /// The collection `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn collection(&self, name: &str) -> Option<Arc<Collection>> {
        let (project, bare) = self.split(name);
        self.collection_in(project, bare)
    }

    /// The collection `name` owned by `project`, when `project` is self or a
    /// direct dependency.
    pub fn collection_in(&self, project: &str, name: &str) -> Option<Arc<Collection>> {
        // `$secrets` and `$variables` are lake collections, not schema. A
        // bare name containing `$` does not resolve, even if a file was
        // written under it. `/data` and `/data/query` both come through here.
        if name.contains('$') {
            return None;
        }
        let version = self.visible_version(project)?;
        self.store
            .collections()
            .get(&Collection::to_path(project, version, name))
    }

    /// The collection address `name` names.
    ///
    /// Parse is a property of the string ([`parse_address`]): the first `.`
    /// separates a project id from `local`, and the first `:` inside `local`
    /// addresses an integration. A name with no `:` is an ordinary collection
    /// only — it does not reach a standard or custom collection. A name with
    /// `:` never falls through to an ordinary collection.
    ///
    /// Resolve is a property of this version. The integration's type is
    /// resolved from the version that declares the integration, so a caller
    /// that does not depend on the type's project can still address
    /// `ben/sync.sf_east:account` when `ben/sync` depends on that type. One
    /// address names one document. Both a standard collection and a custom
    /// collection is a resolved [`CollectionAddressKind::Ambiguous`]: `project`
    /// and `local` are set, and neither document is chosen.
    pub fn collection_address(&self, name: &str) -> AddressResolution<CollectionAddress> {
        let parsed = parse_address(name);
        let Some((project, version)) = self.address_root(parsed.project) else {
            return AddressResolution::Missing;
        };
        if let Some(integration) = parsed.integration {
            if !collection_name_ok(integration) || !collection_name_ok(parsed.name) {
                return AddressResolution::Missing;
            }
            self.integration_collection_address(project, version, integration, parsed.name)
        } else {
            match self.collection_in(project, parsed.name) {
                Some(collection) => {
                    AddressResolution::Resolved(CollectionAddress::ordinary(collection))
                }
                None => AddressResolution::Missing,
            }
        }
    }

    /// One row per address this version shows. Ordinary collections come
    /// first, in [`Self::collections`] order, then each visible project's
    /// integrations by name. A standard collection is one row per integration.
    /// Custom collections of that integration follow. A name both sides
    /// declare is omitted: the listing must not pick one.
    pub fn collection_addresses(&self) -> Vec<CollectionAddress> {
        let mut out: Vec<CollectionAddress> = self
            .collections()
            .into_iter()
            .map(CollectionAddress::ordinary)
            .collect();
        for (project, version) in &self.dependencies {
            let view = self.view_of(project, version);
            for integration in view.integrations_on(project, version) {
                out.extend(view.collection_addresses_of(&integration));
            }
        }
        out
    }

    /// Fields of a resolved address, in the shape of that collection's field
    /// list. A standard collection's fields are the type document's, read
    /// from the declaring version's view, which may see a type this caller
    /// does not depend on. A custom collection's fields are the integration
    /// document's. An ambiguous address has none: the data handler answers
    /// 409 before asking.
    pub fn address_fields(&self, address: &CollectionAddress) -> serde_json::Value {
        let value = match &address.kind {
            CollectionAddressKind::Ordinary => {
                json_value(self.fields_of(&address.owner, &address.name))
            }
            CollectionAddressKind::Standard {
                type_project,
                type_name,
                ..
            } => {
                let view = self.view_of(&address.project, &address.version);
                match view
                    .integration_type_at(type_project, type_name)
                    .and_then(|ty| {
                        ty.collections()
                            .iter()
                            .find(|collection| collection.name() == address.name)
                            .map(|collection| json_value(collection.fields()))
                    }) {
                    Some(fields) => fields,
                    None => serde_json::Value::Array(Vec::new()),
                }
            }
            CollectionAddressKind::Custom => {
                let view = self.view_of(&address.project, &address.version);
                let integration = address.integration.as_deref().unwrap_or("");
                match view
                    .integration_at(&address.project, integration)
                    .and_then(|doc| {
                        doc.collections()
                            .iter()
                            .find(|collection| collection.name() == address.name)
                            .map(|collection| json_value(collection.fields()))
                    }) {
                    Some(fields) => fields,
                    None => serde_json::Value::Array(Vec::new()),
                }
            }
            CollectionAddressKind::Ambiguous => serde_json::Value::Array(Vec::new()),
        };
        if value.is_array() {
            value
        } else {
            serde_json::Value::Array(Vec::new())
        }
    }

    /// The field `name` that `project` declares on the collection
    /// `collection` owned by `owner`. A collection's fields are its owner's:
    /// `None` unless `project` is `owner` and is self or a direct dependency.
    ///
    /// Nobody else's declarations under the same collection name apply —
    /// not another dependency's, which would let installing it change what
    /// `owner`'s collection accepts, and not the running project's, which
    /// would make its own `contacts` fields leak onto every dependency's
    /// `contacts`. Whether a project may extend a dependency's collection is
    /// open (docs/query.md, open question 1).
    pub fn field_in(
        &self,
        owner: &str,
        collection: &str,
        project: &str,
        name: &str,
    ) -> Option<Arc<Field>> {
        if project != owner {
            return None;
        }
        let version = self.visible_version(owner)?;
        self.store
            .fields()
            .get(&Field::to_path(owner, version, collection, name))
    }

    /// The fields of the collection `collection` names (bare or qualified),
    /// which are the ones its owner declares ([`Self::field_in`]). Empty when
    /// the name's project is not visible.
    pub fn fields(&self, collection: &str) -> Vec<Arc<Field>> {
        let (owner, bare) = self.split(collection);
        self.fields_of(owner, bare)
    }

    /// [`Self::fields`] for the collection `collection` owned by `owner`.
    ///
    /// Order is driven by the owner's `auto_add` fieldsets on this collection
    /// (concatenated in fieldset-name order, dedup'd). Fields not named in
    /// any auto_add set are appended in alphabetical-by-key fallback order.
    /// Unknown names in a fieldset are silently skipped — that's how
    /// cascade-delete drift, and in-flight half-applied writes, stay safe to
    /// read.
    pub fn fields_of(&self, owner: &str, collection: &str) -> Vec<Arc<Field>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/fields/{collection}/");
        let raw: Vec<Arc<Field>> = self
            .store
            .fields()
            .list(&prefix)
            .into_iter()
            .map(|(_, f)| f)
            .collect();

        let mut by_name: std::collections::HashMap<String, Arc<Field>> =
            raw.iter().map(|f| (f.name.clone(), f.clone())).collect();

        let mut ordered: Vec<Arc<Field>> = Vec::with_capacity(raw.len());
        let mut seen = std::collections::HashSet::new();

        for fs in auto_add_fieldsets(&self.store, owner, version, collection) {
            for name in &fs.fields {
                if seen.insert(name.clone()) {
                    if let Some(f) = by_name.remove(name) {
                        ordered.push(f);
                    }
                }
            }
        }

        // Fields not mentioned in any auto_add set — alphabetical fallback by
        // the key the store already gave us (project, version, name).
        for f in raw {
            if !seen.contains(&f.name) {
                ordered.push(f);
            }
        }

        ordered
    }

    /// The fieldsets on the collection `collection` names (bare or
    /// qualified) — its owner's, like its fields.
    pub fn fieldsets(&self, collection: &str) -> Vec<Arc<Fieldset>> {
        let Some((owner, version, bare)) = self.resolve(collection) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/fieldsets/{bare}/");
        self.store
            .fieldsets()
            .list(&prefix)
            .into_iter()
            .map(|(_, fs)| fs)
            .collect()
    }

    /// The fieldset `name` on the collection `collection` names. Both follow
    /// the rule, so a dependency's set on its own collection is
    /// `acme/crm.contacts` + `acme/crm.summary`; a bare set name means this
    /// project's, which only its own collections have.
    pub fn fieldset(&self, collection: &str, name: &str) -> Option<Arc<Fieldset>> {
        let (owner, version, bare_collection) = self.resolve(collection)?;
        let (project, bare) = self.split(name);
        if project != owner {
            return None;
        }
        self.store
            .fieldsets()
            .get(&Fieldset::to_path(owner, version, bare_collection, bare))
    }

    /// Every permission set visible to this version, across self + direct
    /// deps. Each carries its `project`, like [`Self::collections`].
    pub fn permission_sets(&self) -> Vec<Arc<PermissionSet>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/permission_sets/");
                self.store
                    .permission_sets()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, ps)| ps)
            })
            .collect()
    }

    /// The permission set `name` names: this project's for a bare name, a
    /// direct dependency's for `{account}/{project}.{name}`. A consumer's own
    /// set of the same name as a dependency's is simply a different set.
    pub fn permission_set(&self, name: &str) -> Option<Arc<PermissionSet>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .permission_sets()
            .get(&PermissionSet::to_path(project, version, bare))
    }

    // --- Writes: scoped to (self.project_id, self.version), draft-only ---

    /// `Ok` only when this view was built writable AND the version is a
    /// draft. Public so a handler can refuse a published version before it
    /// does expensive work on the request body.
    pub fn require_writable(&self) -> Result<(), VersionSchemaError> {
        if self.read_only {
            return Err(VersionSchemaError::NotWritable(format!(
                "schema for {} is read-only in this scope",
                self.project_id
            )));
        }
        if !is_draft_version(&self.version) {
            return Err(VersionSchemaError::NotWritable(format!(
                "version {} is published and read-only",
                self.version
            )));
        }
        Ok(())
    }

    /// Whether this version exists — has a manifest. A version is created
    /// only through `/config`, which writes its manifest, so a version name
    /// that has one was checked there (#95).
    pub fn exists(&self) -> bool {
        self.store
            .manifests()
            .has(&Manifest::to_path(&self.project_id, &self.version))
    }

    /// The gate every write goes through: [`Self::require_writable`], then
    /// `PINS`, then [`Self::exists`]. The caller holds the returned guard
    /// until its last store write.
    ///
    /// `VersionScope` already refused a missing version, but that was before
    /// the request body was read. A version delete cascades under `PINS`, so
    /// a write racing it either holds the lock first, finishes, and is swept
    /// by the cascade, or waits and then finds no manifest here. It can never
    /// land after the cascade and leave an orphan tree.
    ///
    /// The cost is that `/schema` writes, bundle uploads included, serialize
    /// with each other and with `/config` pin changes. All are rare
    /// developer operations; a per-version lock would be finer but would
    /// need the delete to take it too, for no case we have.
    fn write_guard(&self) -> Result<MutexGuard<'static, ()>, VersionSchemaError> {
        self.require_writable()?;
        let pins = lock_pins();
        if !self.exists() {
            return Err(VersionSchemaError::UnknownVersion(unknown_version(
                &self.project_id,
                &self.version,
            )));
        }
        Ok(pins)
    }

    // --- Bundle: the version's own static file tree ---
    //
    // Unlike collections and fields, the bundle is never read through a
    // dependency. A version ships its own frontend; installing a package does
    // not import the package's HTML.

    fn bundle_key(&self) -> String {
        Bundle::to_path(&self.project_id, &self.version)
    }

    /// The bundle tree for this version. `None` when the version has none —
    /// which is every version until something is uploaded.
    pub fn bundle(&self) -> Result<Option<loco_schema_runtime::FileTree>, VersionSchemaError> {
        Ok(self.store.bundles().read_tree(&self.bundle_key())?)
    }

    /// When the current bundle tree was written. `None` when there is none.
    pub fn bundle_uploaded_at(&self) -> Result<Option<std::time::SystemTime>, VersionSchemaError> {
        Ok(self.store.bundles().modified_at(&self.bundle_key())?)
    }

    /// Replace the whole bundle tree. Draft-only, like every other write here.
    pub fn put_bundle(
        &self,
        tree: &loco_schema_runtime::FileTree,
    ) -> Result<Arc<Bundle>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        Ok(self.store.bundles().put(&self.bundle_key(), tree)?)
    }

    /// Drop the bundle tree. `Error::NotFound` when the version has none.
    pub fn delete_bundle(&self) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        Ok(self.store.bundles().delete(&self.bundle_key())?)
    }

    /// Update this version's manifest. A dependency the stored manifest does
    /// not already name must be one `may_read` allows: the caller may only
    /// declare a project it can read (#86). One it already names was allowed
    /// when it was written, so re-sending it — as a client that PUTs the
    /// whole list does — declares nothing new, and a teammate without access
    /// to it can still edit the rest of the manifest.
    pub fn update_manifest(
        &self,
        patch: ManifestUpdate,
        may_read: impl Fn(&str) -> bool,
    ) -> Result<Arc<Manifest>, VersionSchemaError> {
        // Under `PINS`, like a site pin: the check reads other versions'
        // manifests and a version delete checks this one, so without it a
        // delete could slip between the check and the write.
        let _pins = self.write_guard()?;
        let key = Manifest::to_path(&self.project_id, &self.version);
        if let Some(deps) = &patch.dependencies {
            let current = self.store.manifests().get(&key);
            let declared = |dep: &str| {
                current
                    .as_deref()
                    .is_some_and(|m| m.dependencies().iter().any(|d| d == dep))
            };
            check_dependencies(&self.store, &self.project_id, deps, |dep, project| {
                declared(dep) || may_read(project)
            })
            .map_err(VersionSchemaError::InvalidDependency)?;
            // The new list, against the store. This view still has the old
            // dependencies. A bare type is this version's own and is checked
            // when its collections list is written. A dependency this list
            // drops has no standard side.
            check_address_collisions(&self.store, &self.project_id, &self.version, deps)
                .map_err(VersionSchemaError::InvalidDeclaration)?;
        }
        Ok(self.store.manifests().update(&key, patch)?)
    }

    pub fn create_collection(
        &self,
        mut input: Collection,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        if !collection_name_ok(&input.name) {
            return Err(VersionSchemaError::InvalidName(format!(
                "collection name {:?} must be 1 or more of a-z, 0-9, '_', '.', and '-'; \
                 '$' is reserved",
                input.name
            )));
        }
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        let collection_name = input.name.clone();
        let collection = self.store.collections().create(input)?;
        // Eagerly materialize the default fieldset so the file is visible on
        // disk from the start, rather than appearing the first time a field
        // is added. Failures here are intentionally non-fatal — a missing
        // default just falls back to alphabetical ordering until repaired.
        let _ = self.store.fieldsets().create(Fieldset {
            project: self.project_id.clone(),
            version: self.version.clone(),
            collection: collection_name,
            name: DEFAULT_FIELDSET_NAME.to_string(),
            label: String::new(),
            fields: Vec::new(),
            auto_add: true,
        });
        Ok(collection)
    }

    pub fn update_collection(
        &self,
        name: &str,
        patch: CollectionUpdate,
    ) -> Result<Arc<Collection>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Collection::to_path(&self.project_id, &self.version, name);
        Ok(self.store.collections().update(&key, patch)?)
    }

    /// Deletes the collection and every field and fieldset belonging to it
    /// in this version.
    ///
    /// Both prefixes are attempted even when the first fails, and the error
    /// names every key that could not be removed. A leftover fieldset is the
    /// worst of those: it is the ordering a collection recreated at this name
    /// would inherit, because creating the collection again ignores a default
    /// fieldset that is already there. The collection record stays until both
    /// prefixes are gone, so that cannot happen and the delete can be retried.
    pub fn delete_collection(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let field_prefix = format!(
            "{}/versions/{}/fields/{}/",
            self.project_id, self.version, name
        );
        let fieldset_prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, name
        );
        let mut left = Vec::new();
        if self.store.fields().delete_by_prefix(&field_prefix).is_err() {
            left.extend(
                self.store
                    .fields()
                    .list(&field_prefix)
                    .into_iter()
                    .map(|(key, _)| key),
            );
        }
        if self
            .store
            .fieldsets()
            .delete_by_prefix(&fieldset_prefix)
            .is_err()
        {
            left.extend(
                self.store
                    .fieldsets()
                    .list(&fieldset_prefix)
                    .into_iter()
                    .map(|(key, _)| key),
            );
        }
        let key = Collection::to_path(&self.project_id, &self.version, name);
        if left.is_empty() {
            match self.store.collections().delete(&key) {
                Ok(()) | Err(loco_schema_runtime::Error::NotFound(_)) => {}
                Err(_) => left.push(key),
            }
        }
        if left.is_empty() {
            Ok(())
        } else {
            left.sort();
            left.dedup();
            Err(VersionSchemaError::LeftBehind(left))
        }
    }

    pub fn create_field(&self, mut input: Field) -> Result<Arc<Field>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        check_field_type(&input.r#type)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        let collection = input.collection.clone();
        let name = input.name.clone();
        let field = self.store.fields().create(input)?;
        self.append_to_auto_add_sets(&collection, &name);
        Ok(field)
    }

    pub fn update_field(
        &self,
        collection: &str,
        name: &str,
        patch: FieldUpdate,
    ) -> Result<Arc<Field>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        if let Some(ty) = &patch.r#type {
            check_field_type(ty)?;
        }
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fields().update(&key, patch)?)
    }

    pub fn delete_field(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Field::to_path(&self.project_id, &self.version, collection, name);
        self.store.fields().delete(&key)?;
        // Best-effort cascade. The read path already tolerates dangling names
        // by skipping unknowns, so a crash between these two writes leaves
        // the system in a safe (if mildly stale) state.
        self.remove_from_all_fieldsets(collection, name);
        Ok(())
    }

    pub fn create_fieldset(
        &self,
        mut input: Fieldset,
    ) -> Result<Arc<Fieldset>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.fieldsets().create(input)?)
    }

    pub fn update_fieldset(
        &self,
        collection: &str,
        name: &str,
        patch: FieldsetUpdate,
    ) -> Result<Arc<Fieldset>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().update(&key, patch)?)
    }

    pub fn delete_fieldset(&self, collection: &str, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Fieldset::to_path(&self.project_id, &self.version, collection, name);
        Ok(self.store.fieldsets().delete(&key)?)
    }

    pub fn create_permission_set(
        &self,
        mut input: PermissionSet,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.permission_sets().create(input)?)
    }

    pub fn update_permission_set(
        &self,
        name: &str,
        patch: PermissionSetUpdate,
    ) -> Result<Arc<PermissionSet>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().update(&key, patch)?)
    }

    pub fn delete_permission_set(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = PermissionSet::to_path(&self.project_id, &self.version, name);
        Ok(self.store.permission_sets().delete(&key)?)
    }

    /// Every secret declaration visible to this version, across self + direct
    /// deps. Each carries its `project`, like [`Self::collections`].
    pub fn secrets(&self) -> Vec<Arc<Secret>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/secrets/");
                self.store
                    .secrets()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, secret)| secret)
            })
            .collect()
    }

    /// The secret `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn secret(&self, name: &str) -> Option<Arc<Secret>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .secrets()
            .get(&Secret::to_path(project, version, bare))
    }

    /// Secrets `owner` declares on the version this view sees.
    ///
    /// `owner` is a project id (`alice/pkg`). An owner that is not self or a
    /// direct dependency yields an empty list. This does not go through
    /// [`Self::split`]: the running project's secret of the same bare name is
    /// absent unless `owner` is that project.
    pub fn secrets_of(&self, owner: &str) -> Vec<Arc<Secret>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/secrets/");
        self.store
            .secrets()
            .list(&prefix)
            .into_iter()
            .map(|(_, secret)| secret)
            .collect()
    }

    /// The secret `name` that `owner` declares on the version this view sees.
    ///
    /// `name` is bare. A qualified string does not match, and [`Self::split`]
    /// is not used, so the running project's own `name` is a different secret.
    pub fn secret_of(&self, owner: &str, name: &str) -> Option<Arc<Secret>> {
        let version = self.visible_version(owner)?;
        self.store
            .secrets()
            .get(&Secret::to_path(owner, version, name))
    }

    pub fn create_secret(&self, mut input: Secret) -> Result<Arc<Secret>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        self.reject_shared_declaration_name(&input.name, true)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.secrets().create(input)?)
    }

    pub fn update_secret(
        &self,
        name: &str,
        patch: SecretUpdate,
    ) -> Result<Arc<Secret>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Secret::to_path(&self.project_id, &self.version, name);
        Ok(self.store.secrets().update(&key, patch)?)
    }

    pub fn delete_secret(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Secret::to_path(&self.project_id, &self.version, name);
        Ok(self.store.secrets().delete(&key)?)
    }

    /// Every variable declaration visible to this version, across self +
    /// direct deps. Each carries its `project`, like [`Self::collections`].
    pub fn variables(&self) -> Vec<Arc<Variable>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/variables/");
                self.store
                    .variables()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, variable)| variable)
            })
            .collect()
    }

    /// The variable `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn variable(&self, name: &str) -> Option<Arc<Variable>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .variables()
            .get(&Variable::to_path(project, version, bare))
    }

    /// Variables `owner` declares on the version this view sees.
    ///
    /// Same owner rule as [`Self::secrets_of`].
    pub fn variables_of(&self, owner: &str) -> Vec<Arc<Variable>> {
        let Some(version) = self.visible_version(owner) else {
            return Vec::new();
        };
        let prefix = format!("{owner}/versions/{version}/variables/");
        self.store
            .variables()
            .list(&prefix)
            .into_iter()
            .map(|(_, variable)| variable)
            .collect()
    }

    /// The variable `name` that `owner` declares on the version this view sees.
    ///
    /// Same bare-name rule as [`Self::secret_of`].
    pub fn variable_of(&self, owner: &str, name: &str) -> Option<Arc<Variable>> {
        let version = self.visible_version(owner)?;
        self.store
            .variables()
            .get(&Variable::to_path(owner, version, name))
    }

    pub fn create_variable(
        &self,
        mut input: Variable,
    ) -> Result<Arc<Variable>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        self.reject_shared_declaration_name(&input.name, false)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.variables().create(input)?)
    }

    pub fn update_variable(
        &self,
        name: &str,
        patch: VariableUpdate,
    ) -> Result<Arc<Variable>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Variable::to_path(&self.project_id, &self.version, name);
        Ok(self.store.variables().update(&key, patch)?)
    }

    pub fn delete_variable(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Variable::to_path(&self.project_id, &self.version, name);
        Ok(self.store.variables().delete(&key)?)
    }

    /// Every action visible to this version, across self + direct deps.
    /// Each carries its `project`, like [`Self::collections`].
    pub fn actions(&self) -> Vec<Arc<Action>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/actions/");
                self.store
                    .actions()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, action)| action)
            })
            .collect()
    }

    /// The action `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn action(&self, name: &str) -> Option<Arc<Action>> {
        let (project, version, bare) = self.resolve(name)?;
        self.store
            .actions()
            .get(&Action::to_path(project, version, bare))
    }

    /// The action address `name` names. The same parse as
    /// [`Self::collection_address`]. A name with no `:` is an ordinary action.
    /// A name with `:` is a type action on that integration's type document.
    /// There are no custom actions, so an action address is resolved or
    /// missing. A collection collision on the same string does not change that.
    pub fn action_address(&self, name: &str) -> AddressResolution<ActionAddress> {
        let parsed = parse_address(name);
        let Some((project, version)) = self.address_root(parsed.project) else {
            return AddressResolution::Missing;
        };
        if let Some(integration) = parsed.integration {
            if !collection_name_ok(integration) || !collection_name_ok(parsed.name) {
                return AddressResolution::Missing;
            }
            match self.integration_action_address(project, version, integration, parsed.name) {
                Some(action) => AddressResolution::Resolved(action),
                None => AddressResolution::Missing,
            }
        } else {
            match self.action_in(project, version, parsed.name) {
                Some(action) => AddressResolution::Resolved(ActionAddress::ordinary(action)),
                None => AddressResolution::Missing,
            }
        }
    }

    /// One row per action address. Ordinary actions come first, in
    /// [`Self::actions`] order, then type actions, one row per integration
    /// that exposes them.
    pub fn action_addresses(&self) -> Vec<ActionAddress> {
        let mut out: Vec<ActionAddress> = self
            .actions()
            .into_iter()
            .map(ActionAddress::ordinary)
            .collect();
        for (project, version) in &self.dependencies {
            let view = self.view_of(project, version);
            for integration in view.integrations_on(project, version) {
                out.extend(view.action_addresses_of(&integration));
            }
        }
        out
    }

    pub fn create_action(&self, mut input: Action) -> Result<Arc<Action>, VersionSchemaError> {
        check_action_params(input.params())?;
        let _pins = self.write_guard()?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.actions().create(input)?)
    }

    /// `patch.params` replaces the whole list. Omitting it leaves the stored
    /// params in place.
    pub fn update_action(
        &self,
        name: &str,
        patch: ActionUpdate,
    ) -> Result<Arc<Action>, VersionSchemaError> {
        if let Some(params) = &patch.params {
            check_action_params(params)?;
        }
        let _pins = self.write_guard()?;
        let key = Action::to_path(&self.project_id, &self.version, name);
        Ok(self.store.actions().update(&key, patch)?)
    }

    pub fn delete_action(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Action::to_path(&self.project_id, &self.version, name);
        Ok(self.store.actions().delete(&key)?)
    }

    // --- Integration types and integrations ---
    //
    // Standard collections, their fields, and the type's actions are lists on
    // the type document. Custom collections and their fields are a list on
    // the integration document. A consumer cannot add to a dependency's
    // document: the path name is this version's document, and a qualified
    // name that is not one of its documents is not found. Name and shape
    // checks run after `write_guard`, so a published version reports that it
    // is read-only first.

    /// Every integration type visible to this version, across self + direct
    /// deps. Each carries its `project`, like [`Self::collections`].
    pub fn integration_types(&self) -> Vec<Arc<IntegrationType>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/integration_types/");
                self.store
                    .integration_types()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, item)| item)
            })
            .collect()
    }

    /// The integration type `name` names: this project's for a bare name, a
    /// direct dependency's for `{account}/{project}.{name}`.
    pub fn integration_type(&self, name: &str) -> Option<Arc<IntegrationType>> {
        let (project, bare) = self.split(name);
        self.integration_type_at(project, bare)
    }

    fn integration_type_at(&self, project: &str, name: &str) -> Option<Arc<IntegrationType>> {
        let version = self.visible_version(project)?;
        self.store
            .integration_types()
            .get(&IntegrationType::to_path(project, version, name))
    }

    pub fn create_integration_type(
        &self,
        mut input: IntegrationType,
    ) -> Result<Arc<IntegrationType>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        require_slug_name("integration type", &input.name)?;
        check_integration_type_document(&input)?;
        self.reject_standard_collection_collisions(&input.name, input.collections())?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.integration_types().create(input)?)
    }

    /// A list the patch names replaces that list. The stored document, with
    /// the patch applied, is checked as a whole. Omitting a list leaves it.
    pub fn update_integration_type(
        &self,
        name: &str,
        patch: IntegrationTypeUpdate,
    ) -> Result<Arc<IntegrationType>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = IntegrationType::to_path(&self.project_id, &self.version, name);
        if let Some(current) = self.store.integration_types().get(&key) {
            let mut next = (*current).clone();
            patch.apply(&mut next);
            check_integration_type_document(&next)?;
            // Omitting `collections` leaves the stored list and does not
            // re-check it. Naming the list, including a re-send of a list
            // that already collides, refuses the first custom name an
            // integration of this type in this version already declares.
            if patch.collections.is_some() {
                self.reject_standard_collection_collisions(name, next.collections())?;
            }
        }
        Ok(self.store.integration_types().update(&key, patch)?)
    }

    /// Deletes the type. Its collections, fields, and actions are on the
    /// document, so they go with it. A missing type is not found.
    pub fn delete_integration_type(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = IntegrationType::to_path(&self.project_id, &self.version, name);
        Ok(self.store.integration_types().delete(&key)?)
    }

    /// Every integration visible to this version, across self + direct deps.
    pub fn integrations(&self) -> Vec<Arc<Integration>> {
        self.dependencies
            .iter()
            .flat_map(|(project_id, version)| {
                let prefix = format!("{project_id}/versions/{version}/integrations/");
                self.store
                    .integrations()
                    .list(&prefix)
                    .into_iter()
                    .map(|(_, item)| item)
            })
            .collect()
    }

    /// The integration `name` names: this project's for a bare name, a direct
    /// dependency's for `{account}/{project}.{name}`.
    pub fn integration(&self, name: &str) -> Option<Arc<Integration>> {
        let (project, bare) = self.split(name);
        self.integration_at(project, bare)
    }

    fn integration_at(&self, project: &str, name: &str) -> Option<Arc<Integration>> {
        let version = self.visible_version(project)?;
        self.store
            .integrations()
            .get(&Integration::to_path(project, version, name))
    }

    /// Secrets and variables of the integration `integration_project` declares
    /// as `integration_name`, resolved from the version that declares it.
    ///
    /// `integration` is the canonical qualified name from **this** view's
    /// project: bare when the integration is this project's (`sf_east`),
    /// `{account}/{project}.{name}` when it is a dependency's
    /// (`loco/bricklink.store`). `type_ref` is the type's reference from this
    /// same view. The type may live on a project this view does not itself
    /// depend on; the declaring version is what sees it. `None` when the
    /// integration is not visible here, or its type is gone.
    pub(crate) fn connection_declarations(
        &self,
        integration_project: &str,
        integration_name: &str,
    ) -> Option<ConnectionDeclarations> {
        if !collection_name_ok(integration_name) {
            return None;
        }
        let version = self.visible_version(integration_project)?;
        let view = self.view_of(integration_project, version);
        let doc = view.integration_at(integration_project, integration_name)?;
        let ty = view.integration_type(doc.r#type())?;
        Some(ConnectionDeclarations {
            integration: self.reference(integration_project, integration_name),
            project: integration_project.to_string(),
            type_ref: self.reference(ty.project(), ty.name()),
            secrets: ty
                .secrets()
                .iter()
                .map(|secret| ConnectionSecretDecl {
                    name: secret.name().to_string(),
                    required: secret.required(),
                })
                .collect(),
            variables: ty
                .variables()
                .iter()
                .map(|variable| ConnectionVariableDecl {
                    name: variable.name().to_string(),
                    required: variable.required(),
                    default_value: variable.default().to_string(),
                })
                .collect(),
        })
    }

    /// Stores `type` in canonical form: bare when this version declares it,
    /// `{account}/{project}.{name}` when a direct dependency does.
    pub fn create_integration(
        &self,
        mut input: Integration,
    ) -> Result<Arc<Integration>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        require_slug_name("integration", &input.name)?;
        input.r#type = self.canonical_type_ref(&input.r#type)?;
        self.check_integration_document(&input)?;
        input.project = self.project_id.clone();
        input.version = self.version.clone();
        Ok(self.store.integrations().create(input)?)
    }

    /// A patch that names `type` is stored in the same canonical form as
    /// [`Self::create_integration`]. A list the patch names replaces that
    /// list. The path name is this version's document, not a dependency's.
    pub fn update_integration(
        &self,
        name: &str,
        mut patch: IntegrationUpdate,
    ) -> Result<Arc<Integration>, VersionSchemaError> {
        let _pins = self.write_guard()?;
        if let Some(ty) = patch.r#type.clone() {
            patch.r#type = Some(self.canonical_type_ref(&ty)?);
        }
        let key = Integration::to_path(&self.project_id, &self.version, name);
        if let Some(current) = self.store.integrations().get(&key) {
            // A label-only write, and a re-send of the stored type, leave a
            // collision that was planted past this check for the read path.
            // Naming `collections`, or changing the canonical type, re-checks.
            let type_changed = patch
                .r#type
                .as_ref()
                .is_some_and(|ty| ty != current.r#type());
            if patch.collections.is_some() || type_changed {
                let mut next = (*current).clone();
                patch.apply(&mut next);
                self.check_integration_document(&next)?;
            }
        }
        Ok(self.store.integrations().update(&key, patch)?)
    }

    /// Deletes the integration. Its custom collections and fields are on the
    /// document, so they go with it. Secret and variable values are not this
    /// document and stay.
    pub fn delete_integration(&self, name: &str) -> Result<(), VersionSchemaError> {
        let _pins = self.write_guard()?;
        let key = Integration::to_path(&self.project_id, &self.version, name);
        Ok(self.store.integrations().delete(&key)?)
    }

    /// `type_ref` as this version should store it. Empty is refused. A type
    /// this version does not see — missing, or not a direct dependency — is
    /// the same refusal.
    fn canonical_type_ref(&self, type_ref: &str) -> Result<String, VersionSchemaError> {
        if type_ref.is_empty() {
            return Err(VersionSchemaError::InvalidDeclaration(
                "integration type is required".into(),
            ));
        }
        let (project, bare) = self.split(type_ref);
        require_slug_name("integration type", bare)?;
        if self.integration_type_at(project, bare).is_none() {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "integration type '{type_ref}' is not declared"
            )));
        }
        Ok(self.reference(project, bare))
    }

    /// Custom collection names must not be names the integration's type
    /// already offers. The type reference is the canonical one.
    fn check_integration_document(&self, input: &Integration) -> Result<(), VersionSchemaError> {
        check_inline_collections(input.collections())?;
        let (project, bare) = self.split(input.r#type());
        let Some(ty) = self.integration_type_at(project, bare) else {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "integration type '{}' is not declared",
                input.r#type()
            )));
        };
        let mut names: Vec<&str> = input.collections().iter().map(|c| c.name()).collect();
        names.sort();
        for name in names {
            if ty
                .collections()
                .iter()
                .any(|offered| offered.name() == name)
            {
                return Err(VersionSchemaError::InvalidDeclaration(address_collision(
                    input.name(),
                    name,
                    input.r#type(),
                )));
            }
        }
        Ok(())
    }

    /// A secret and a variable of one bare name in this version would make a
    /// later lookup ambiguous. The other store is only read, and that read
    /// lock drops before this store's `create` takes its writer. `PINS`,
    /// already held by `write_guard`, is what makes the check and the create
    /// one step — the two stores do not lock each other.
    fn reject_shared_declaration_name(
        &self,
        name: &str,
        creating_secret: bool,
    ) -> Result<(), VersionSchemaError> {
        let taken = if creating_secret {
            self.store
                .variables()
                .has(&Variable::to_path(&self.project_id, &self.version, name))
        } else {
            self.store
                .secrets()
                .has(&Secret::to_path(&self.project_id, &self.version, name))
        };
        if taken {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "secret and variable share the name '{name}'"
            )));
        }
        Ok(())
    }

    /// Append `field_name` to every `auto_add` fieldset in this project+version
    /// for the given collection. If none exists, lazy-create the default,
    /// seeded with every existing field in this project+version so collections
    /// that predate the feature don't end up with a partial default. The new
    /// `field_name` is appended last regardless.
    ///
    /// Each append is a [`InstanceStore::update_with`] on the fieldset store,
    /// so the list it extends is the current one, not a snapshot a concurrent
    /// `create_field` has since changed. Only the fieldset store is locked
    /// there; the field store is read beforehand, never inside it.
    ///
    /// [`InstanceStore::update_with`]: loco_schema_runtime::InstanceStore::update_with
    fn append_to_auto_add_sets(&self, collection: &str, field_name: &str) {
        let auto_sets =
            auto_add_fieldsets(&self.store, &self.project_id, &self.version, collection);
        if auto_sets.is_empty() {
            let prefix = format!(
                "{}/versions/{}/fields/{}/",
                self.project_id, self.version, collection
            );
            let mut seed: Vec<String> = self
                .store
                .fields()
                .list(&prefix)
                .into_iter()
                .map(|(_, f)| f.name.clone())
                .filter(|n| n != field_name)
                .collect();
            seed.push(field_name.to_string());
            match self.store.fieldsets().create(Fieldset {
                project: self.project_id.clone(),
                version: self.version.clone(),
                collection: collection.to_string(),
                name: DEFAULT_FIELDSET_NAME.to_string(),
                label: String::new(),
                fields: seed,
                auto_add: true,
            }) {
                // A concurrent create_field made the default first, maybe
                // from a field list that predates ours: append to it instead.
                Err(loco_schema_runtime::Error::AlreadyExists(_)) => {}
                _ => return,
            }
        }
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        for (key, _) in self.store.fieldsets().list(&prefix) {
            let _ = self.store.fieldsets().update_with(&key, |fs| {
                if !fs.auto_add || fs.fields.iter().any(|n| n == field_name) {
                    return None;
                }
                let mut next = fs.fields.clone();
                next.push(field_name.to_string());
                Some(FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                })
            });
        }
    }

    /// Strip `field_name` from every fieldset in this project+version's view of
    /// the collection. Touches only sets that actually reference the name,
    /// each under the fieldset store's write lock like the append above.
    fn remove_from_all_fieldsets(&self, collection: &str, field_name: &str) {
        let prefix = format!(
            "{}/versions/{}/fieldsets/{}/",
            self.project_id, self.version, collection
        );
        for (key, _) in self.store.fieldsets().list(&prefix) {
            let _ = self.store.fieldsets().update_with(&key, |fs| {
                if !fs.fields.iter().any(|n| n == field_name) {
                    return None;
                }
                let next: Vec<String> = fs
                    .fields
                    .iter()
                    .filter(|n| *n != field_name)
                    .cloned()
                    .collect();
                Some(FieldsetUpdate {
                    label: None,
                    fields: Some(next),
                    auto_add: None,
                })
            });
        }
    }

    /// `(project, version)` the address is rooted in. `None` when a named
    /// project is not self or a direct dependency. A bare address uses this
    /// version.
    fn address_root<'a>(&'a self, project: Option<&str>) -> Option<(&'a str, &'a str)> {
        match project {
            Some(project) => self
                .dependencies
                .iter()
                .find(|(id, _)| id == project)
                .map(|(id, version)| (id.as_str(), version.as_str())),
            None => Some((self.project_id.as_str(), self.version.as_str())),
        }
    }

    /// This view when `(project, version)` is the one it was built for, and
    /// a read-only view of that version otherwise. The declaring version is
    /// what sees the integration's type.
    fn view_of(&self, project: &str, version: &str) -> VersionSchema {
        if project == self.project_id && version == self.version {
            self.clone()
        } else {
            VersionSchema::new_read_only(Arc::clone(&self.store), project, version)
        }
    }

    fn action_in(&self, project: &str, version: &str, name: &str) -> Option<Arc<Action>> {
        self.store
            .actions()
            .get(&Action::to_path(project, version, name))
    }

    /// Integrations declared on this one version, by name. Not
    /// [`Self::integrations`], which spans the view's dependencies.
    fn integrations_on(&self, project: &str, version: &str) -> Vec<Arc<Integration>> {
        let prefix = format!("{project}/versions/{version}/integrations/");
        let mut items = self.store.integrations().list(&prefix);
        items.sort_by(|(_, a), (_, b)| a.name().cmp(b.name()));
        items.into_iter().map(|(_, item)| item).collect()
    }

    fn integration_collection_address(
        &self,
        project: &str,
        version: &str,
        integration: &str,
        name: &str,
    ) -> AddressResolution<CollectionAddress> {
        let view = self.view_of(project, version);
        let Some(integration_doc) = view.integration_at(project, integration) else {
            return AddressResolution::Missing;
        };
        let standard =
            view.standard_collection_address(&integration_doc, project, version, integration, name);
        let custom = integration_doc
            .collections()
            .iter()
            .find(|collection| collection.name() == name)
            .map(|custom| CollectionAddress {
                project: project.to_string(),
                version: version.to_string(),
                local: format!("{integration}:{name}"),
                integration: Some(integration.to_string()),
                name: name.to_string(),
                owner: project.to_string(),
                label: custom.label().to_string(),
                label_plural: custom.label_plural().to_string(),
                kind: CollectionAddressKind::Custom,
            });
        match (standard, custom) {
            (Some(_), Some(_)) => AddressResolution::Resolved(CollectionAddress::ambiguous(
                project,
                version,
                integration,
                name,
            )),
            (Some(address), None) => AddressResolution::Resolved(address),
            (None, Some(address)) => AddressResolution::Resolved(address),
            (None, None) => AddressResolution::Missing,
        }
    }

    /// The type's collection, when this integration's type offers `name`.
    /// The type is resolved on `self`, which is the declaring version's view.
    /// The collection is an entry on the type document, not its own store.
    fn standard_collection_address(
        &self,
        integration_doc: &Integration,
        project: &str,
        version: &str,
        integration: &str,
        name: &str,
    ) -> Option<CollectionAddress> {
        let ty = self.integration_type(integration_doc.r#type())?;
        let collection = ty.collections().iter().find(|item| item.name() == name)?;
        Some(CollectionAddress {
            project: project.to_string(),
            version: version.to_string(),
            local: format!("{integration}:{name}"),
            integration: Some(integration.to_string()),
            name: name.to_string(),
            owner: ty.project().to_string(),
            label: collection.label().to_string(),
            label_plural: collection.label_plural().to_string(),
            kind: CollectionAddressKind::Standard {
                type_project: ty.project().to_string(),
                type_version: ty.version().to_string(),
                type_name: ty.name().to_string(),
            },
        })
    }

    fn collection_addresses_of(&self, integration: &Integration) -> Vec<CollectionAddress> {
        let project = integration.project();
        let version = integration.version();
        let integration_name = integration.name();
        let mut standards = Vec::new();
        if let Some(ty) = self.integration_type(integration.r#type()) {
            for collection in ty.collections() {
                if let Some(address) = self.standard_collection_address(
                    integration,
                    project,
                    version,
                    integration_name,
                    collection.name(),
                ) {
                    standards.push(address);
                }
            }
        }
        let customs: Vec<CollectionAddress> = integration
            .collections()
            .iter()
            .map(|collection| CollectionAddress {
                project: project.to_string(),
                version: version.to_string(),
                local: format!("{integration_name}:{}", collection.name()),
                integration: Some(integration_name.to_string()),
                name: collection.name().to_string(),
                owner: project.to_string(),
                label: collection.label().to_string(),
                label_plural: collection.label_plural().to_string(),
                kind: CollectionAddressKind::Custom,
            })
            .collect();
        let standard_names: HashSet<String> = standards
            .iter()
            .map(|address| address.name.clone())
            .collect();
        let custom_names: HashSet<String> =
            customs.iter().map(|address| address.name.clone()).collect();
        let mut out = Vec::new();
        out.extend(
            standards
                .into_iter()
                .filter(|address| !custom_names.contains(&address.name)),
        );
        out.extend(
            customs
                .into_iter()
                .filter(|address| !standard_names.contains(&address.name)),
        );
        out
    }

    fn integration_action_address(
        &self,
        project: &str,
        version: &str,
        integration: &str,
        name: &str,
    ) -> Option<ActionAddress> {
        let view = self.view_of(project, version);
        let integration_doc = view.integration_at(project, integration)?;
        view.type_action_address(&integration_doc, project, version, integration, name)
    }

    fn type_action_address(
        &self,
        integration_doc: &Integration,
        project: &str,
        version: &str,
        integration: &str,
        name: &str,
    ) -> Option<ActionAddress> {
        let ty = self.integration_type(integration_doc.r#type())?;
        let action = ty.actions().iter().find(|item| item.name() == name)?;
        Some(ActionAddress {
            project: project.to_string(),
            version: version.to_string(),
            local: format!("{integration}:{name}"),
            integration: Some(integration.to_string()),
            name: name.to_string(),
            owner: ty.project().to_string(),
            label: action.label().to_string(),
            description: action.description().to_string(),
            params: json_value(action.params()),
            kind: ActionAddressKind::Type {
                type_project: ty.project().to_string(),
                type_name: ty.name().to_string(),
                action: Arc::new(action.clone()),
            },
        })
    }

    fn action_addresses_of(&self, integration: &Integration) -> Vec<ActionAddress> {
        let Some(ty) = self.integration_type(integration.r#type()) else {
            return Vec::new();
        };
        let project = integration.project();
        let version = integration.version();
        let integration_name = integration.name();
        ty.actions()
            .iter()
            .filter_map(|action| {
                self.type_action_address(
                    integration,
                    project,
                    version,
                    integration_name,
                    action.name(),
                )
            })
            .collect()
    }

    /// A standard collection this version is about to store, when an
    /// integration of this type in this version already declares that name
    /// as custom. `type_name` is this version's own type. A different
    /// project's integration is the read-fail-closed case, not this check.
    fn reject_standard_collection_collisions(
        &self,
        type_name: &str,
        collections: &[IntegrationCollection],
    ) -> Result<(), VersionSchemaError> {
        let offered: HashSet<&str> = collections.iter().map(|c| c.name()).collect();
        if offered.is_empty() {
            return Ok(());
        }
        let type_ref = self.reference(&self.project_id, type_name);
        for integration in self.integrations_on(&self.project_id, &self.version) {
            let (owner, bare) = self.split(integration.r#type());
            if owner != self.project_id || bare != type_name {
                continue;
            }
            let mut names: Vec<&str> = integration.collections().iter().map(|c| c.name()).collect();
            names.sort();
            for name in names {
                if offered.contains(name) {
                    return Err(VersionSchemaError::InvalidDeclaration(address_collision(
                        integration.name(),
                        name,
                        &type_ref,
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Collection names are the slug charset (`[a-z0-9_.-]+`). `$` is outside
/// it, which is what keeps `$secrets` and `$variables` from being declared.
fn collection_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// The message for a `/schema` request to a version that does not exist.
pub fn unknown_version(project_id: &str, version: &str) -> String {
    format!("unknown version: {project_id}@{version}")
}

/// `{account}/{project}.{local}` → `(project, local)`. `None` for a bare name
/// — one with no `/`, since every project id has one and no name does — and
/// for a string that has a `/` but no `.`.
///
/// The split is the first `.`. A project id contains no `.`, so `local` may
/// (`acme/crm.foo.bar` is project `acme/crm`, name `foo.bar`). The last `.`
/// would read that as project `acme/crm.foo`.
pub fn split_qualified(name: &str) -> Option<(&str, &str)> {
    if !name.contains('/') {
        return None;
    }
    name.split_once('.')
}

/// An address string, before it is resolved against a version.
///
/// `project` is `None` when the string has no `{account}/{project}.` prefix
/// (the caller treats that as self). `integration` is `None` when `local`
/// has no `:`. A second `:` stays in `name`; [`collection_name_ok`] then
/// rejects it.
pub struct ParsedAddress<'a> {
    pub project: Option<&'a str>,
    pub integration: Option<&'a str>,
    pub name: &'a str,
}

pub fn parse_address(name: &str) -> ParsedAddress<'_> {
    let (project, local) = match split_qualified(name) {
        Some((project, local)) => (Some(project), local),
        None => (None, name),
    };
    match local.split_once(':') {
        Some((integration, name)) => ParsedAddress {
            project,
            integration: Some(integration),
            name,
        },
        None => ParsedAddress {
            project,
            integration: None,
            name: local,
        },
    }
}

fn json_value(value: impl Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()))
}

/// The `auto_add` fieldsets `project` declares on `collection` in `version`.
fn auto_add_fieldsets(
    store: &SchemaStore,
    project: &str,
    version: &str,
    collection: &str,
) -> Vec<Arc<Fieldset>> {
    let prefix = format!("{project}/versions/{version}/fieldsets/{collection}/");
    store
        .fieldsets()
        .list(&prefix)
        .into_iter()
        .filter_map(|(_, fs)| if fs.auto_add { Some(fs) } else { None })
        .collect()
}

/// Split a manifest dependency, `{account}/{project}@{version}`, into
/// `(project_id, version)`. `None` unless it has exactly that shape with no
/// empty part.
pub(crate) fn parse_dependency(dep: &str) -> Option<(&str, &str)> {
    let (project_id, version) = dep.split_once('@')?;
    let (account, project) = project_id.split_once('/')?;
    let segment = |s: &str| !s.is_empty() && !s.contains(['/', '@']);
    (segment(account) && segment(project) && segment(version)).then_some((project_id, version))
}

/// A secret body is a declaration. `default` and `value` are a store's
/// setting of it, and a package must not ship either. Presence of the key is
/// enough — `null` counts. A body that is not an object is left for the
/// deserializer.
pub fn reject_secret_value(body: &serde_json::Value) -> Result<(), VersionSchemaError> {
    let Some(obj) = body.as_object() else {
        return Ok(());
    };
    let present: Vec<&str> = ["default", "value"]
        .into_iter()
        .filter(|key| obj.contains_key(*key))
        .collect();
    if present.is_empty() {
        return Ok(());
    }
    let listed = present
        .iter()
        .map(|key| format!("'{key}'"))
        .collect::<Vec<_>>()
        .join(" and ");
    Err(VersionSchemaError::InvalidDeclaration(format!(
        "a secret declares a name, not a value: remove {listed}"
    )))
}

/// A field may declare only a type the validator enforces and the lake can
/// store. Checked on write only: a field loaded from disk with another type
/// still boots.
fn check_field_type(ty: &str) -> Result<(), VersionSchemaError> {
    if FIELD_TYPES.contains(&ty) {
        return Ok(());
    }
    Err(VersionSchemaError::InvalidFieldType(format!(
        "unknown field type '{ty}': expected one of {}",
        FIELD_TYPES.join(", ")
    )))
}

/// Params are part of the action document. Each `name` is one slug segment
/// (`[a-z0-9_.-]+`) and unique in the list, each `type` is a [`FIELD_TYPES`]
/// entry, and `options` are only stored on a string param. Checked on write
/// only: an action loaded from disk is left as it is, and a version copy
/// does not run this again.
fn check_action_params(params: &[ActionParam]) -> Result<(), VersionSchemaError> {
    check_param_decls(
        params
            .iter()
            .map(|param| (param.name(), param.r#type(), !param.options().is_empty())),
    )
}

fn check_param_decls<'a>(
    params: impl IntoIterator<Item = (&'a str, &'a str, bool)>,
) -> Result<(), VersionSchemaError> {
    let mut seen = HashSet::new();
    for (name, ty, has_options) in params {
        if !param_name_ok(name) {
            return Err(VersionSchemaError::InvalidName(format!(
                "param name {name:?} must be a slug: one or more of a-z, 0-9, '_', '.', and '-'"
            )));
        }
        if !seen.insert(name) {
            return Err(VersionSchemaError::InvalidName(format!(
                "param name '{name}' is declared more than once"
            )));
        }
        check_field_type(ty)?;
        if ty != "string" && has_options {
            return Err(VersionSchemaError::InvalidDeclaration(format!(
                "param '{name}' declares options, which are only meaningful for type string"
            )));
        }
    }
    Ok(())
}

fn require_slug_name(kind: &str, name: &str) -> Result<(), VersionSchemaError> {
    if collection_name_ok(name) {
        Ok(())
    } else {
        Err(VersionSchemaError::InvalidName(format!(
            "{kind} name {name:?} must be 1 or more of a-z, 0-9, '_', '.', and '-'; '$' is reserved"
        )))
    }
}

/// A secret inside an integration type's `secrets` list is a declaration.
/// `default` and `value` on one of those objects are refused. A missing
/// `secrets`, or a `secrets` that is not an array (`null` included), is left
/// for the deserializer: `null` means the stored list stays.
pub fn reject_integration_secret_values(
    body: &serde_json::Value,
) -> Result<(), VersionSchemaError> {
    let Some(secrets) = body.get("secrets").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    for secret in secrets {
        let Some(obj) = secret.as_object() else {
            continue;
        };
        let present: Vec<&str> = ["default", "value"]
            .into_iter()
            .filter(|key| obj.contains_key(*key))
            .collect();
        if present.is_empty() {
            continue;
        }
        let name = obj
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let listed = present
            .iter()
            .map(|key| format!("'{key}'"))
            .collect::<Vec<_>>()
            .join(" and ");
        return Err(VersionSchemaError::InvalidDeclaration(format!(
            "a secret declares a name, not a value: remove {listed} on secret '{name}'"
        )));
    }
    Ok(())
}

/// Names are unique within each list and across both. A shared name uses the
/// same message as a version-level secret and variable. These lists are not
/// checked against the version's loose secrets.
fn check_connection_declarations<'a>(
    secrets: &'a [IntegrationSecret],
    variables: &'a [IntegrationVariable],
) -> Result<(), VersionSchemaError> {
    let mut seen: HashMap<&'a str, &'a str> = HashMap::new();
    for secret in secrets {
        note_connection_name(&mut seen, "secret", secret.name())?;
    }
    for variable in variables {
        note_connection_name(&mut seen, "variable", variable.name())?;
    }
    Ok(())
}

fn note_connection_name<'a>(
    seen: &mut HashMap<&'a str, &'a str>,
    kind: &'a str,
    name: &'a str,
) -> Result<(), VersionSchemaError> {
    require_slug_name(kind, name)?;
    match seen.insert(name, kind) {
        Some(previous) if previous == kind => Err(VersionSchemaError::InvalidName(format!(
            "{kind} name '{name}' is declared more than once"
        ))),
        Some(_) => Err(VersionSchemaError::InvalidDeclaration(format!(
            "secret and variable share the name '{name}'"
        ))),
        None => Ok(()),
    }
}

fn check_integration_type_document(input: &IntegrationType) -> Result<(), VersionSchemaError> {
    check_connection_declarations(input.secrets(), input.variables())?;
    check_inline_collections(input.collections())?;
    check_inline_actions(input.actions())
}

fn check_inline_collections(
    collections: &[IntegrationCollection],
) -> Result<(), VersionSchemaError> {
    let mut names = HashSet::new();
    for collection in collections {
        reject_duplicate_slug("collection", collection.name(), &mut names)?;
        let mut fields = HashSet::new();
        for field in collection.fields() {
            reject_duplicate_slug("field", field.name(), &mut fields)?;
            check_field_type(field.r#type())?;
            if field.r#type() != "string" && !field.options().is_empty() {
                return Err(VersionSchemaError::InvalidDeclaration(format!(
                    "field '{}' declares options, which are only meaningful for type string",
                    field.name()
                )));
            }
        }
    }
    Ok(())
}

fn check_inline_actions(actions: &[IntegrationAction]) -> Result<(), VersionSchemaError> {
    let mut names = HashSet::new();
    for action in actions {
        reject_duplicate_slug("action", action.name(), &mut names)?;
        check_param_decls(
            action
                .params()
                .iter()
                .map(|param| (param.name(), param.r#type(), !param.options().is_empty())),
        )?;
    }
    Ok(())
}

fn reject_duplicate_slug<'a>(
    kind: &str,
    name: &'a str,
    seen: &mut HashSet<&'a str>,
) -> Result<(), VersionSchemaError> {
    require_slug_name(kind, name)?;
    if !seen.insert(name) {
        return Err(VersionSchemaError::InvalidName(format!(
            "{kind} name '{name}' is declared more than once"
        )));
    }
    Ok(())
}

/// One segment of the schema slug charset. A param name is not a path, so a
/// `/` is not a separator here — it is simply not a slug character.
fn param_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// Why `deps` may not be the dependency list of a version of `project_id`,
/// or `Ok` when it may. Every entry must parse, name another project, name a
/// project no other entry names, and name a version whose manifest exists
/// and that `visible(entry, project)` allows.
///
/// An entry `visible` refuses gets the same error as one that does not
/// exist, word for word, so a manifest write cannot be used to learn whether
/// a project the caller cannot read has a given version (#86).
///
/// One version per project, because a qualified name
/// (`{account}/{project}.{name}`) carries no version: two versions of one
/// dependency could not both be addressed. Not this project, for the same
/// reason — its own names already resolve to self.
///
/// Callers hold `PINS`, so a version this finds cannot be deleted before
/// they write.
pub(crate) fn check_dependencies(
    store: &SchemaStore,
    project_id: &str,
    deps: &[String],
    visible: impl Fn(&str, &str) -> bool,
) -> Result<(), String> {
    let mut seen = Vec::new();
    for dep in deps {
        let Some((dep_project, dep_version)) = parse_dependency(dep) else {
            return Err(format!(
                "dependency {dep:?} is not of the form {{account}}/{{project}}@{{version}}"
            ));
        };
        if dep_project == project_id {
            return Err(format!(
                "dependency {dep:?} names this project; a version cannot depend on its own project"
            ));
        }
        if seen.contains(&dep_project) {
            return Err(format!(
                "dependency {dep:?} repeats project {dep_project}; depend on one version of it"
            ));
        }
        seen.push(dep_project);
        if !visible(dep, dep_project)
            || !store
                .manifests()
                .has(&Manifest::to_path(dep_project, dep_version))
        {
            return Err(format!(
                "dependency {dep:?} names version {dep_version} of {dep_project}, which does not exist"
            ));
        }
    }
    Ok(())
}

/// Why `deps` would make an address on `project_id`'s `version` name two
/// documents, or `Ok` when it would not.
///
/// For each integration this version declares whose type is a dependency
/// (`{account}/{project}.{name}`), look at that dependency version's type
/// document. A name this integration also declares as custom is a collision.
/// A bare type is this version's own and is checked when the type's
/// collections list is written. A dependency the new list drops has no
/// standard side here, so it is not a collision.
///
/// Callers hold `PINS` and have already accepted `deps` with
/// [`check_dependencies`]. This only reads.
pub(crate) fn check_address_collisions(
    store: &SchemaStore,
    project_id: &str,
    version: &str,
    deps: &[String],
) -> Result<(), String> {
    let mut dep_version = HashMap::new();
    for dep in deps {
        if let Some((project, dep_version_name)) = parse_dependency(dep) {
            dep_version.insert(project, dep_version_name);
        }
    }
    let prefix = format!("{project_id}/versions/{version}/integrations/");
    let mut integrations = store.integrations().list(&prefix);
    integrations.sort_by(|(_, a), (_, b)| a.name().cmp(b.name()));
    for (_, integration) in integrations {
        let Some((type_project, type_name)) = split_qualified(integration.r#type()) else {
            continue;
        };
        let Some(type_version) = dep_version.get(type_project) else {
            continue;
        };
        let Some(ty) = store.integration_types().get(&IntegrationType::to_path(
            type_project,
            type_version,
            type_name,
        )) else {
            continue;
        };
        let offered: HashSet<&str> = ty.collections().iter().map(|c| c.name()).collect();
        if offered.is_empty() {
            continue;
        }
        let mut names: Vec<&str> = integration.collections().iter().map(|c| c.name()).collect();
        names.sort();
        for name in names {
            if offered.contains(name) {
                return Err(address_collision(
                    integration.name(),
                    name,
                    integration.r#type(),
                ));
            }
        }
    }
    Ok(())
}

/// Self entry plus direct deps from the manifest, as `(project_id, version)`
/// pairs. Writes are checked by [`check_dependencies`], but a manifest loaded
/// from disk is not, so a malformed entry is skipped here rather than failing
/// the read.
fn direct_dependencies(
    project_id: &str,
    version: &str,
    manifest: Option<&Manifest>,
) -> Vec<(String, String)> {
    let mut deps = vec![(project_id.to_string(), version.to_string())];
    if let Some(manifest) = manifest {
        for child in manifest.dependencies() {
            if let Some((namespace, v)) = parse_dependency(child) {
                deps.push((namespace.to_string(), v.to_string()));
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntegrationActionParam, IntegrationField};
    use std::sync::Barrier;

    const PROJECT: &str = "ben/crm";
    const VERSION: &str = "0.0.1-dev";

    fn draft_schema() -> (tempfile::TempDir, Arc<SchemaStore>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SchemaStore::load(dir.path()).unwrap());
        store
            .manifests()
            .create(Manifest::new(
                PROJECT.to_string(),
                VERSION.to_string(),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        (dir, store)
    }

    fn collection(name: &str) -> Collection {
        Collection::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
        )
    }

    /// The view was built while the version existed — as `VersionScope`
    /// checks — and the version is deleted before the write. The write must
    /// find that out itself and leave nothing on disk (#95).
    #[test]
    fn write_after_version_delete_is_refused_and_writes_nothing() {
        let (dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        assert!(schema.exists());
        crate::http::project_config::ProjectConfig::new(store.clone(), "ben", "crm")
            .delete_version(VERSION)
            .unwrap();

        let err = schema.create_collection(collection("tasks")).unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::UnknownVersion(_)),
            "{err:?}"
        );
        let err = schema
            .put_bundle(&loco_schema_runtime::FileTree::default())
            .unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::UnknownVersion(_)),
            "{err:?}"
        );
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert!(store.collections().list(&prefix).is_empty());
        assert!(store.fieldsets().list(&prefix).is_empty());
        assert!(store.bundles().list(&prefix).is_empty());
        // And on disk, where a reload would find an orphan.
        let reloaded = SchemaStore::load(dir.path()).unwrap();
        assert!(reloaded.collections().list(&prefix).is_empty());
        assert!(reloaded.bundles().list(&prefix).is_empty());
    }

    fn field(collection: &str, name: &str) -> Field {
        Field {
            project: PROJECT.to_string(),
            version: VERSION.to_string(),
            collection: collection.to_string(),
            name: name.to_string(),
            r#type: "string".to_string(),
            ..Field::default()
        }
    }

    /// Races `create_field` calls on one collection and returns the auto-add
    /// fieldset's `fields` afterwards.
    fn race_field_creates(store: &Arc<SchemaStore>, collection: &str, n: usize) -> Vec<String> {
        let barrier = Arc::new(Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                let collection = collection.to_string();
                std::thread::spawn(move || {
                    let schema = VersionSchema::new(store, PROJECT, VERSION);
                    barrier.wait();
                    schema
                        .create_field(field(&collection, &format!("f{i}")))
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let key = Fieldset::to_path(PROJECT, VERSION, collection, DEFAULT_FIELDSET_NAME);
        store.fieldsets().get(&key).unwrap().fields.clone()
    }

    fn assert_all_present(fields: &[String], n: usize) {
        let mut got = fields.to_vec();
        got.sort();
        let mut want: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
        want.sort();
        assert_eq!(got, want, "auto-add fieldset lost or duplicated a name");
    }

    #[test]
    fn concurrent_create_field_keeps_every_name_in_auto_add_set() {
        const N: usize = 16;
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        for round in 0..5 {
            let collection = format!("c{round}");
            schema
                .create_collection(self::collection(&collection))
                .unwrap();
            assert_all_present(&race_field_creates(&store, &collection, N), N);
        }
    }

    /// No default fieldset yet: the racers all try to lazy-create it, and the
    /// losers must still land their name in the winner's set.
    #[test]
    fn concurrent_create_field_lazy_creates_one_default_set() {
        const N: usize = 16;
        let (_dir, store) = draft_schema();
        for round in 0..5 {
            let collection = format!("c{round}");
            assert_all_present(&race_field_creates(&store, &collection, N), N);
        }
    }

    /// Directory mode is what stops `YamlFsAdapter` unlinking a key. Root
    /// ignores it, so the test refuses to pass vacuously there.
    struct Unlock(std::path::PathBuf);

    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    fn freeze(dir: &std::path::Path) -> Unlock {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probe = dir.join(".loco-probe");
        if std::fs::write(&probe, b"x").is_ok() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_file(&probe);
            panic!("directory mode did not block writes; this test needs a non-root user");
        }
        Unlock(dir.to_path_buf())
    }

    fn secret(name: &str) -> Secret {
        Secret::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
            false,
        )
    }

    fn variable(name: &str) -> Variable {
        Variable::new(
            PROJECT.to_string(),
            VERSION.to_string(),
            name.to_string(),
            String::new(),
            String::new(),
            false,
            String::new(),
        )
    }

    #[test]
    fn secret_and_variable_cannot_share_a_name() {
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store, PROJECT, VERSION);
        schema.create_secret(secret("consumer_key")).unwrap();
        let err = schema
            .create_variable(variable("consumer_key"))
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "secret and variable share the name 'consumer_key'"
        );
        schema.create_variable(variable("api_base")).unwrap();
        let err = schema.create_secret(secret("api_base")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "secret and variable share the name 'api_base'"
        );
        assert!(schema.secret("consumer_key").is_some());
        assert!(schema.variable("api_base").is_some());
    }

    #[test]
    fn dollar_is_not_a_collection_name() {
        let (_dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        let err = schema
            .create_collection(collection("$secrets"))
            .unwrap_err();
        assert!(matches!(err, VersionSchemaError::InvalidName(_)), "{err}");
        assert!(err.to_string().contains('$'), "{err}");
        assert_eq!(
            crate::http::response::version_schema_error_to_response(err).status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        // A file planted under that name still does not resolve. `/data` and
        // `/data/query` both go through `collection`.
        store.collections().create(collection("$secrets")).unwrap();
        assert!(schema.collection("$secrets").is_none());
        assert!(schema.collection("alice/other.$variables").is_none());
        schema.create_collection(collection("orders")).unwrap();
        assert!(schema.collection("orders").is_some());
    }

    #[test]
    fn secret_body_rejects_default_and_value() {
        let err =
            reject_secret_value(&serde_json::json!({"name": "k", "default": "x"})).unwrap_err();
        assert_eq!(
            err.to_string(),
            "a secret declares a name, not a value: remove 'default'"
        );
        assert_eq!(
            crate::http::response::version_schema_error_to_response(err).status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let err = reject_secret_value(&serde_json::json!({"value": serde_json::Value::Null}))
            .unwrap_err();
        assert!(err.to_string().contains("'value'"));
        let err =
            reject_secret_value(&serde_json::json!({"default": "x", "value": "y"})).unwrap_err();
        assert_eq!(
            err.to_string(),
            "a secret declares a name, not a value: remove 'default' and 'value'"
        );
        assert!(reject_secret_value(&serde_json::json!({"name": "k", "label": "K"})).is_ok());
    }

    #[test]
    fn left_behind_answers_500() {
        let response = crate::http::response::version_schema_error_to_response(
            VersionSchemaError::LeftBehind(vec![
                "ben/crm/versions/0.0.1-dev/fieldsets/task/default".into(),
            ]),
        );
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn delete_collection_reports_a_failed_fieldset_and_clears_fields() {
        let (dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        schema.create_collection(collection("task")).unwrap();
        schema.create_field(field("task", "title")).unwrap();

        let fieldset_key = Fieldset::to_path(PROJECT, VERSION, "task", "default");
        let field_key = Field::to_path(PROJECT, VERSION, "task", "title");
        let collection_key = Collection::to_path(PROJECT, VERSION, "task");
        let frozen = dir
            .path()
            .join(format!("{fieldset_key}.yaml"))
            .parent()
            .unwrap()
            .to_path_buf();
        let _hold = freeze(&frozen);

        let err = schema.delete_collection("task").unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("delete left behind: {fieldset_key}")
        );
        match err {
            VersionSchemaError::LeftBehind(keys) => assert_eq!(keys, vec![fieldset_key.clone()]),
            other => panic!("expected leftovers, got {other}"),
        }
        assert!(store.fieldsets().has(&fieldset_key));
        assert!(!store.fields().has(&field_key));
        assert!(store.collections().has(&collection_key));

        drop(_hold);
        schema.delete_collection("task").unwrap();
        assert!(!store.fieldsets().has(&fieldset_key));
        assert!(!store.collections().has(&collection_key));
    }

    fn integration_type(name: &str) -> IntegrationType {
        IntegrationType {
            name: name.to_string(),
            label: name.to_string(),
            secrets: vec![IntegrationSecret {
                name: "consumer_key".into(),
                label: "Consumer key".into(),
                required: true,
                ..IntegrationSecret::default()
            }],
            variables: vec![IntegrationVariable {
                name: "base_url".into(),
                default: "https://api.bricklink.com/api/store/v1".into(),
                ..Default::default()
            }],
            ..IntegrationType::default()
        }
    }

    #[test]
    fn integration_secret_body_rejects_default_and_value() {
        let err = reject_integration_secret_values(&serde_json::json!({
            "secrets": [{"name": "consumer_key", "value": "x"}]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("'value'"), "{err}");
        assert!(err.to_string().contains("consumer_key"), "{err}");
        let err = reject_integration_secret_values(&serde_json::json!({
            "secrets": [{"name": "token", "default": "x", "value": null}]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("'default' and 'value'"), "{err}");
        assert!(reject_integration_secret_values(&serde_json::json!({"secrets": null})).is_ok());
        assert!(reject_integration_secret_values(&serde_json::json!({"label": "x"})).is_ok());
    }

    /// Two types may each offer `orders` and `set_status` on their own
    /// documents. Deleting one type leaves the other. Deleting an integration
    /// removes the custom collections that were on it. A qualified self type
    /// reference is stored bare. A reload keeps these documents out of the
    /// ordinary collection and action stores.
    #[test]
    fn integration_declarations_round_trip_and_delete_cascades() {
        let (dir, store) = draft_schema();
        let schema = VersionSchema::new(store.clone(), PROJECT, VERSION);
        schema
            .create_integration_type(integration_type("bricklink"))
            .unwrap();
        schema
            .create_integration_type(integration_type("warehouse"))
            .unwrap();
        schema
            .update_integration_type(
                "bricklink",
                IntegrationTypeUpdate {
                    collections: Some(vec![orders_collection("BrickLink orders")]),
                    actions: Some(vec![status_action("Set status")]),
                    ..Default::default()
                },
            )
            .unwrap();
        schema
            .update_integration_type(
                "warehouse",
                IntegrationTypeUpdate {
                    collections: Some(vec![orders_collection("Warehouse orders")]),
                    actions: Some(vec![status_action("Warehouse status")]),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            schema.integration_type("bricklink").unwrap().secrets()[0].name(),
            "consumer_key"
        );

        let err = schema
            .update_integration_type(
                "bricklink",
                IntegrationTypeUpdate {
                    collections: Some(vec![orders_collection("One"), orders_collection("Two")]),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::InvalidName(ref msg) if msg.contains("orders")),
            "{err}"
        );
        assert_eq!(
            schema.integration_type("bricklink").unwrap().collections()[0].label(),
            "BrickLink orders"
        );

        let stored = schema
            .create_integration(Integration {
                name: "store".into(),
                r#type: format!("{PROJECT}.bricklink"),
                label: "Store".into(),
                collections: vec![IntegrationCollection {
                    name: "invoice".into(),
                    label: "Invoice".into(),
                    fields: vec![IntegrationField {
                        name: "amount".into(),
                        r#type: "string".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(stored.r#type(), "bricklink");
        assert_eq!(stored.collections()[0].fields()[0].name(), "amount");

        let err = schema
            .update_integration(
                "store",
                IntegrationUpdate {
                    collections: Some(vec![orders_collection("Stolen")]),
                    ..Default::default()
                },
            )
            .unwrap_err();
        let VersionSchemaError::InvalidDeclaration(msg) = &err else {
            panic!("expected a collision, got {err}");
        };
        assert!(msg.contains("orders"), "{msg}");
        assert!(msg.contains("bricklink"), "{msg}");
        assert_eq!(
            schema.integration("store").unwrap().collections()[0].name(),
            "invoice"
        );

        schema.create_collection(collection("orders")).unwrap();
        assert_eq!(schema.collections().len(), 1);
        assert_eq!(
            schema.integration_type("warehouse").unwrap().collections()[0].label(),
            "Warehouse orders"
        );
        assert_eq!(
            schema.integration_type("bricklink").unwrap().actions()[0].label(),
            "Set status"
        );

        let reloaded = SchemaStore::load(dir.path()).unwrap();
        let prefix = format!("{PROJECT}/versions/{VERSION}/");
        assert_eq!(reloaded.collections().list(&prefix).len(), 1);
        assert_eq!(reloaded.integration_types().list(&prefix).len(), 2);
        assert_eq!(reloaded.integrations().list(&prefix).len(), 1);
        assert!(reloaded.fields().list(&prefix).is_empty());
        assert!(reloaded.actions().list(&prefix).is_empty());

        schema.delete_integration("store").unwrap();
        assert!(schema.integration("store").is_none());
        assert_eq!(store.integrations().list(&prefix).len(), 0);

        schema.delete_integration_type("bricklink").unwrap();
        assert!(schema.integration_type("bricklink").is_none());
        assert_eq!(
            schema.integration_type("warehouse").unwrap().collections()[0].label(),
            "Warehouse orders"
        );
        assert_eq!(
            schema.integration_type("warehouse").unwrap().actions()[0].label(),
            "Warehouse status"
        );
        assert!(schema.collection("orders").is_some());
    }

    /// An installer may declare its own integration of a dependency's type,
    /// including a custom collection. A collection the type already offers is
    /// refused, naming both. Writing the dependency's integration or type by
    /// its qualified name does not modify that document.
    #[test]
    fn installer_cannot_declare_on_a_dependency_integration() {
        let (_dir, store) = draft_schema();
        store
            .manifests()
            .create(Manifest::new(
                "alice/pkg".into(),
                VERSION.into(),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        store
            .manifests()
            .create(Manifest::new(
                "alice/shop".into(),
                VERSION.into(),
                vec!["alice/pkg@0.0.1-dev".into()],
                Vec::new(),
            ))
            .unwrap();

        let pkg = VersionSchema::new(store.clone(), "alice/pkg", VERSION);
        pkg.create_integration_type(IntegrationType {
            collections: vec![IntegrationCollection {
                name: "account".into(),
                label: "Account".into(),
                ..Default::default()
            }],
            ..integration_type("salesforce")
        })
        .unwrap();
        pkg.create_integration(Integration {
            name: "store".into(),
            r#type: "salesforce".into(),
            ..Default::default()
        })
        .unwrap();

        let shop = VersionSchema::new(store.clone(), "alice/shop", VERSION);
        let err = shop
            .update_integration_type(
                "alice/pkg.salesforce",
                IntegrationTypeUpdate {
                    collections: Some(vec![orders_collection("Stolen")]),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(
                err,
                VersionSchemaError::Schema(loco_schema_runtime::Error::NotFound(_))
            ),
            "{err}"
        );
        assert_eq!(
            pkg.integration_type("salesforce").unwrap().collections()[0].name(),
            "account"
        );

        let err = shop
            .update_integration(
                "alice/pkg.store",
                IntegrationUpdate {
                    collections: Some(vec![IntegrationCollection {
                        name: "invoice".into(),
                        label: "Stolen".into(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, VersionSchemaError::Schema(loco_schema_runtime::Error::NotFound(ref key)) if key.contains("alice/pkg.store")),
            "{err}"
        );
        assert!(pkg.integration("store").unwrap().collections().is_empty());

        let err = shop
            .create_integration(Integration {
                name: "sf_east".into(),
                r#type: "alice/pkg.salesforce".into(),
                collections: vec![IntegrationCollection {
                    name: "account".into(),
                    label: "Account".into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap_err();
        let VersionSchemaError::InvalidDeclaration(msg) = &err else {
            panic!("expected a collision, got {err}");
        };
        assert!(msg.contains("account"), "{msg}");
        assert!(msg.contains("alice/pkg.salesforce"), "{msg}");

        let created = shop
            .create_integration(Integration {
                name: "sf_east".into(),
                r#type: "alice/pkg.salesforce".into(),
                collections: vec![IntegrationCollection {
                    name: "invoice".into(),
                    label: "Invoice".into(),
                    fields: vec![IntegrationField {
                        name: "amount".into(),
                        r#type: "integer".into(),
                        options: vec![crate::IntegrationFieldOption {
                            value: "1".into(),
                            label: "One".into(),
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            matches!(created, VersionSchemaError::InvalidDeclaration(ref msg) if msg.contains("amount")),
            "{created}"
        );

        let created = shop
            .create_integration(Integration {
                name: "sf_east".into(),
                r#type: "alice/pkg.salesforce".into(),
                collections: vec![IntegrationCollection {
                    name: "invoice".into(),
                    label: "Invoice".into(),
                    fields: vec![IntegrationField {
                        name: "amount".into(),
                        r#type: "string".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(created.r#type(), "alice/pkg.salesforce");
        assert_eq!(created.collections()[0].name(), "invoice");
        assert!(pkg.integration("store").unwrap().collections().is_empty());
    }

    fn orders_collection(label: &str) -> IntegrationCollection {
        IntegrationCollection {
            name: "orders".into(),
            label: label.into(),
            fields: vec![IntegrationField {
                name: "status".into(),
                r#type: "string".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn status_action(label: &str) -> IntegrationAction {
        IntegrationAction {
            name: "set_status".into(),
            label: label.into(),
            params: vec![IntegrationActionParam {
                name: "order_id".into(),
                r#type: "string".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn inline_collection(name: &str, label: &str, label_plural: &str) -> IntegrationCollection {
        IntegrationCollection {
            name: name.into(),
            label: label.into(),
            label_plural: label_plural.into(),
            ..Default::default()
        }
    }

    fn account_with_status() -> IntegrationCollection {
        IntegrationCollection {
            name: "account".into(),
            label: "Account".into(),
            label_plural: "Accounts".into(),
            fields: vec![IntegrationField {
                name: "status".into(),
                r#type: "string".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn set_owner_action() -> IntegrationAction {
        IntegrationAction {
            name: "set_owner".into(),
            label: "Set owner".into(),
            params: vec![IntegrationActionParam {
                name: "owner_id".into(),
                r#type: "string".into(),
                required: true,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn split_qualified_uses_the_first_dot() {
        assert_eq!(
            split_qualified("acme/crm.foo.bar"),
            Some(("acme/crm", "foo.bar"))
        );
        assert_eq!(
            split_qualified("acme/crm.contacts"),
            Some(("acme/crm", "contacts"))
        );
        assert_eq!(split_qualified("sf_east:account"), None);
        assert_eq!(
            split_qualified("ben/sync.sf_east:account"),
            Some(("ben/sync", "sf_east:account"))
        );
        assert_eq!(split_qualified("acme/crm"), None);

        let parsed = parse_address("ben/sync.sf_east:account");
        assert_eq!(parsed.project, Some("ben/sync"));
        assert_eq!(parsed.integration, Some("sf_east"));
        assert_eq!(parsed.name, "account");
        let parsed = parse_address("sf_east:account:extra");
        assert_eq!(parsed.integration, Some("sf_east"));
        assert_eq!(parsed.name, "account:extra");
        let parsed = parse_address("orders");
        assert_eq!(parsed.project, None);
        assert_eq!(parsed.integration, None);
        assert_eq!(parsed.name, "orders");
    }

    fn one_collection(schema: &VersionSchema, name: &str) -> Option<CollectionAddress> {
        match schema.collection_address(name) {
            AddressResolution::Resolved(address) => Some(address),
            AddressResolution::Missing => None,
        }
    }

    fn one_action(schema: &VersionSchema, name: &str) -> Option<ActionAddress> {
        match schema.action_address(name) {
            AddressResolution::Resolved(address) => Some(address),
            AddressResolution::Missing => None,
        }
    }

    fn manifests(store: &SchemaStore, projects: &[(&str, Vec<String>)]) {
        for (project, deps) in projects {
            store
                .manifests()
                .create(Manifest::new(
                    (*project).into(),
                    VERSION.into(),
                    deps.clone(),
                    Vec::new(),
                ))
                .unwrap();
        }
    }

    /// Standard collections resolve through the integration's type document,
    /// including when the caller does not depend on the type's project. A
    /// custom collection is an entry on the integration document. A dotted
    /// ordinary name splits on the first dot.
    #[test]
    fn integration_addresses_resolve_standard_custom_and_dotted_names() {
        let (_dir, store) = draft_schema();
        manifests(
            &store,
            &[
                ("acme/salesforce", Vec::new()),
                ("acme/crm", Vec::new()),
                (
                    "ben/sync",
                    vec![
                        "acme/salesforce@0.0.1-dev".to_string(),
                        "acme/crm@0.0.1-dev".to_string(),
                    ],
                ),
                ("alice/shop", vec!["ben/sync@0.0.1-dev".to_string()]),
            ],
        );

        let salesforce = VersionSchema::new(store.clone(), "acme/salesforce", VERSION);
        salesforce
            .create_integration_type(IntegrationType {
                name: "salesforce".into(),
                label: "Salesforce".into(),
                collections: vec![account_with_status()],
                actions: vec![set_owner_action()],
                ..IntegrationType::default()
            })
            .unwrap();
        salesforce
            .create_integration(Integration {
                name: "prod".into(),
                r#type: "salesforce".into(),
                ..Integration::default()
            })
            .unwrap();
        salesforce
            .create_integration_type(IntegrationType {
                name: "foo.bar".into(),
                label: "Dotted".into(),
                collections: vec![inline_collection("widget", "Widget", "Widgets")],
                ..IntegrationType::default()
            })
            .unwrap();
        let dotted = salesforce
            .create_integration(Integration {
                name: "dotted".into(),
                r#type: "acme/salesforce.foo.bar".into(),
                ..Integration::default()
            })
            .unwrap();
        assert_eq!(dotted.r#type(), "foo.bar");

        let crm = VersionSchema::new(store.clone(), "acme/crm", VERSION);
        crm.create_collection(Collection {
            name: "foo.bar".into(),
            label: "Foo bar".into(),
            ..Collection::default()
        })
        .unwrap();

        let sync = VersionSchema::new(store.clone(), "ben/sync", VERSION);
        sync.create_integration(Integration {
            name: "sf_east".into(),
            r#type: "acme/salesforce.salesforce".into(),
            collections: vec![IntegrationCollection {
                name: "invoice__c".into(),
                label: "Invoice".into(),
                fields: vec![IntegrationField {
                    name: "amount".into(),
                    r#type: "integer".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Integration::default()
        })
        .unwrap();
        sync.create_integration(Integration {
            name: "sf_west".into(),
            r#type: "acme/salesforce.salesforce".into(),
            ..Integration::default()
        })
        .unwrap();
        let err = sync
            .update_integration(
                "sf_east",
                IntegrationUpdate {
                    collections: Some(vec![
                        inline_collection("invoice__c", "Invoice", "Invoices"),
                        inline_collection("account", "Custom account", "Custom accounts"),
                    ]),
                    ..IntegrationUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "ambiguous address sf_east:account: type 'acme/salesforce.salesforce' offers collection 'account' and integration 'sf_east' declares it"
        );
        assert_eq!(
            sync.integration("sf_east").unwrap().collections()[0].name(),
            "invoice__c"
        );
        assert_eq!(sync.integration("sf_east").unwrap().collections().len(), 1);

        let shop = VersionSchema::new_read_only(store, "alice/shop", VERSION);

        assert_eq!(sync.collections().len(), 1);
        assert!(one_collection(&sync, "account").is_none());
        assert!(one_collection(&sync, "acme/salesforce.account").is_none());
        assert!(sync.collection("sf_east:account").is_none());

        let east = one_collection(&sync, "sf_east:account").unwrap();
        assert_eq!(east.project, "ben/sync");
        assert_eq!(east.owner, "acme/salesforce");
        assert_eq!(east.name, "account");
        assert_eq!(east.local, "sf_east:account");
        assert_eq!(east.integration.as_deref(), Some("sf_east"));
        assert_eq!(east.label, "Account");
        assert_eq!(east.label_plural, "Accounts");
        assert!(matches!(
            east.kind,
            CollectionAddressKind::Standard { ref type_name, .. } if type_name == "salesforce"
        ));
        let fields = sync.address_fields(&east);
        assert_eq!(fields[0]["name"], "status");
        assert_eq!(fields[0]["type"], "string");
        assert!(fields[0].get("project").is_none());

        assert!(one_collection(&sync, "sf_west:invoice__c").is_none());
        let invoice = one_collection(&sync, "sf_east:invoice__c").unwrap();
        assert_eq!(invoice.owner, "ben/sync");
        assert!(matches!(invoice.kind, CollectionAddressKind::Custom));
        let fields = sync.address_fields(&invoice);
        assert_eq!(fields[0]["name"], "amount");
        assert_eq!(fields[0]["type"], "integer");
        assert!(fields[0].get("integration").is_none());

        let prod = one_collection(&sync, "acme/salesforce.prod:account").unwrap();
        assert_eq!(prod.project, "acme/salesforce");
        assert_eq!(prod.owner, "acme/salesforce");
        assert!(one_collection(&sync, "prod:account").is_none());

        let nested = one_collection(&shop, "ben/sync.sf_east:account").unwrap();
        assert_eq!(nested.project, "ben/sync");
        assert_eq!(nested.owner, "acme/salesforce");
        assert!(one_collection(&shop, "sf_east:account").is_none());
        assert!(one_collection(&shop, "acme/salesforce.prod:account").is_none());

        let dotted_collection = one_collection(&sync, "acme/crm.foo.bar").unwrap();
        assert_eq!(dotted_collection.project, "acme/crm");
        assert_eq!(dotted_collection.name, "foo.bar");
        assert_eq!(dotted_collection.owner, "acme/crm");
        assert!(dotted_collection.integration.is_none());
        assert!(sync.collection("acme/crm.foo.bar").is_some());

        assert!(one_collection(&sync, "sf_east:account:extra").is_none());
        assert!(one_collection(&sync, "sf_east:").is_none());
        assert!(one_collection(&sync, ":account").is_none());
        assert!(one_collection(&sync, "SF_EAST:account").is_none());

        let widget = one_collection(&sync, "acme/salesforce.dotted:widget").unwrap();
        assert_eq!(widget.name, "widget");
        assert_eq!(widget.owner, "acme/salesforce");

        let bare = one_collection(&salesforce, "prod:account").unwrap();
        let qualified = one_collection(&salesforce, "acme/salesforce.prod:account").unwrap();
        assert_eq!(bare.local, qualified.local);
        assert_eq!(bare.project, qualified.project);

        let rows = sync.collection_addresses();
        assert_eq!(rows[0].name, "foo.bar");
        assert!(rows[0].integration.is_none());
        let find = |integration: &str, name: &str| {
            rows.iter()
                .find(|row| row.integration.as_deref() == Some(integration) && row.name == name)
                .unwrap()
        };
        let listed = find("sf_east", "account");
        assert_eq!(listed.project, "ben/sync");
        assert_eq!(listed.owner, "acme/salesforce");
        assert_eq!(listed.label, "Account");
        assert_eq!(listed.label_plural, "Accounts");
        assert_eq!(find("sf_east", "invoice__c").owner, "ben/sync");
        assert!(rows
            .iter()
            .any(|row| { row.integration.as_deref() == Some("sf_west") && row.name == "account" }));
        assert!(!rows.iter().any(|row| {
            row.integration.as_deref() == Some("sf_west") && row.name == "invoice__c"
        }));
        assert_eq!(find("prod", "account").project, "acme/salesforce");

        let shop_rows = shop.collection_addresses();
        assert!(shop_rows.iter().any(|row| {
            row.project == "ben/sync"
                && row.integration.as_deref() == Some("sf_east")
                && row.name == "account"
        }));
        assert!(!shop_rows
            .iter()
            .any(|row| row.integration.as_deref() == Some("prod")));

        let action = one_action(&sync, "sf_east:set_owner").unwrap();
        assert_eq!(action.project, "ben/sync");
        assert_eq!(action.owner, "acme/salesforce");
        assert_eq!(action.name, "set_owner");
        assert_eq!(action.params[0]["name"], "owner_id");
        assert!(matches!(
            action.kind,
            ActionAddressKind::Type { ref type_project, ref type_name, .. }
                if type_project == "acme/salesforce" && type_name == "salesforce"
        ));
        assert!(one_action(&sync, "set_owner").is_none());
        assert!(sync.action("sf_east:set_owner").is_none());
        let actions = sync.action_addresses();
        assert!(actions.iter().any(|row| {
            row.integration.as_deref() == Some("sf_west")
                && row.name == "set_owner"
                && row.project == "ben/sync"
                && row.owner == "acme/salesforce"
        }));
    }

    /// One address names one document. Writing a list that would make both
    /// sides visible is 400 and stores nothing. A collision that gets onto
    /// disk anyway resolves to neither document, and the listing omits both.
    #[test]
    fn address_collisions_are_refused_and_fail_closed() {
        let (_dir, store) = draft_schema();
        manifests(
            &store,
            &[
                ("acme/salesforce", Vec::new()),
                ("ben/sync", vec!["acme/salesforce@0.0.1-dev".to_string()]),
                ("alice/shop", vec!["ben/sync@0.0.1-dev".to_string()]),
            ],
        );

        let salesforce = VersionSchema::new(store.clone(), "acme/salesforce", VERSION);
        salesforce
            .create_integration_type(IntegrationType {
                name: "salesforce".into(),
                label: "Salesforce".into(),
                collections: vec![account_with_status()],
                actions: vec![set_owner_action()],
                ..IntegrationType::default()
            })
            .unwrap();
        salesforce
            .create_integration_type(IntegrationType {
                name: "crm".into(),
                label: "CRM".into(),
                ..IntegrationType::default()
            })
            .unwrap();
        salesforce
            .create_integration(Integration {
                name: "prod".into(),
                r#type: "salesforce".into(),
                collections: vec![inline_collection("extra", "Extra", "Extras")],
                ..Integration::default()
            })
            .unwrap();
        // prod is type salesforce, not crm, so crm may offer the custom name.
        salesforce
            .update_integration_type(
                "crm",
                IntegrationTypeUpdate {
                    collections: Some(vec![inline_collection("extra", "Extra", "Extras")]),
                    ..IntegrationTypeUpdate::default()
                },
            )
            .unwrap();

        let same_version = salesforce
            .update_integration_type(
                "salesforce",
                IntegrationTypeUpdate {
                    collections: Some(vec![
                        account_with_status(),
                        inline_collection("extra", "Extra", "Extras"),
                    ]),
                    ..IntegrationTypeUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            same_version,
            "ambiguous address prod:extra: type 'salesforce' offers collection 'extra' and integration 'prod' declares it"
        );
        assert_eq!(
            salesforce
                .integration_type("salesforce")
                .unwrap()
                .collections()
                .len(),
            1
        );
        assert_eq!(
            salesforce
                .integration_type("salesforce")
                .unwrap()
                .collections()[0]
                .name(),
            "account"
        );
        // Re-sending the colliding list refuses again and still stores nothing.
        let resent_type = salesforce
            .update_integration_type(
                "salesforce",
                IntegrationTypeUpdate {
                    collections: Some(vec![
                        account_with_status(),
                        inline_collection("extra", "Extra", "Extras"),
                    ]),
                    ..IntegrationTypeUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(resent_type, same_version);

        let type_change = salesforce
            .update_integration(
                "prod",
                IntegrationUpdate {
                    r#type: Some("crm".into()),
                    ..IntegrationUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            type_change,
            "ambiguous address prod:extra: type 'crm' offers collection 'extra' and integration 'prod' declares it"
        );
        assert_eq!(
            salesforce.integration("prod").unwrap().r#type(),
            "salesforce"
        );
        assert!(matches!(
            salesforce.collection_address("prod:extra"),
            AddressResolution::Resolved(ref address)
                if matches!(address.kind, CollectionAddressKind::Custom)
        ));

        let sync = VersionSchema::new(store.clone(), "ben/sync", VERSION);
        let created = sync
            .create_integration(Integration {
                name: "sf_bad".into(),
                r#type: "acme/salesforce.salesforce".into(),
                collections: vec![inline_collection("account", "Custom account", "Accounts")],
                ..Integration::default()
            })
            .unwrap_err()
            .to_string();
        assert_eq!(
            created,
            "ambiguous address sf_bad:account: type 'acme/salesforce.salesforce' offers collection 'account' and integration 'sf_bad' declares it"
        );
        assert!(sync.integration("sf_bad").is_none());
        sync.create_integration(Integration {
            name: "sf_east".into(),
            r#type: "acme/salesforce.salesforce".into(),
            collections: vec![inline_collection("invoice__c", "Invoice", "Invoices")],
            ..Integration::default()
        })
        .unwrap();
        let custom = sync
            .update_integration(
                "sf_east",
                IntegrationUpdate {
                    collections: Some(vec![
                        inline_collection("invoice__c", "Invoice", "Invoices"),
                        inline_collection("account", "Custom account", "Custom accounts"),
                    ]),
                    ..IntegrationUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            custom,
            "ambiguous address sf_east:account: type 'acme/salesforce.salesforce' offers collection 'account' and integration 'sf_east' declares it"
        );
        assert_eq!(sync.integration("sf_east").unwrap().collections().len(), 1);
        assert!(matches!(
            sync.collection_address("sf_east:invoice__c"),
            AddressResolution::Resolved(_)
        ));
        assert!(matches!(
            sync.action_address("sf_east:invoice__c"),
            AddressResolution::Missing
        ));

        // A later dependency version offers the custom name. The manifest
        // write is refused and the stored dependencies stay. The type
        // document is written on the store: a published version refuses
        // `/schema` writes.
        store
            .manifests()
            .create(Manifest::new(
                "acme/salesforce".into(),
                "1.0.0".into(),
                Vec::new(),
                Vec::new(),
            ))
            .unwrap();
        store
            .integration_types()
            .create(IntegrationType {
                project: "acme/salesforce".into(),
                version: "1.0.0".into(),
                name: "salesforce".into(),
                label: "Salesforce".into(),
                collections: vec![inline_collection("invoice__c", "Invoice", "Invoices")],
                ..IntegrationType::default()
            })
            .unwrap();
        let bumped = sync
            .update_manifest(
                ManifestUpdate {
                    dependencies: Some(vec!["acme/salesforce@1.0.0".into()]),
                    ..ManifestUpdate::default()
                },
                |_| true,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            bumped,
            "ambiguous address sf_east:invoice__c: type 'acme/salesforce.salesforce' offers collection 'invoice__c' and integration 'sf_east' declares it"
        );
        assert_eq!(
            sync.manifest().unwrap().dependencies(),
            &["acme/salesforce@0.0.1-dev".to_string()]
        );

        // The pinned draft later gains the name. That write is on the other
        // project, so it is stored. The collection address resolves with
        // project and local, and picks neither document. The action of the
        // same string is missing until the type declares one, and then it
        // resolves: a collection collision does not make an action ambiguous.
        // Naming `collections` replaces the list, so `account` is sent again.
        salesforce
            .update_integration_type(
                "salesforce",
                IntegrationTypeUpdate {
                    collections: Some(vec![
                        account_with_status(),
                        inline_collection("invoice__c", "Invoice", "Invoices"),
                    ]),
                    ..IntegrationTypeUpdate::default()
                },
            )
            .unwrap();
        assert!(matches!(
            sync.collection_address("sf_east:invoice__c"),
            AddressResolution::Resolved(ref address)
                if address.project == "ben/sync"
                    && address.local == "sf_east:invoice__c"
                    && matches!(address.kind, CollectionAddressKind::Ambiguous)
        ));
        assert!(matches!(
            sync.action_address("sf_east:invoice__c"),
            AddressResolution::Missing
        ));
        salesforce
            .update_integration_type(
                "salesforce",
                IntegrationTypeUpdate {
                    actions: Some(vec![
                        set_owner_action(),
                        IntegrationAction {
                            name: "invoice__c".into(),
                            label: "Invoice".into(),
                            ..IntegrationAction::default()
                        },
                    ]),
                    ..IntegrationTypeUpdate::default()
                },
            )
            .unwrap();
        assert!(matches!(
            sync.action_address("sf_east:invoice__c"),
            AddressResolution::Resolved(ref address)
                if address.project == "ben/sync" && address.local == "sf_east:invoice__c"
        ));
        assert!(matches!(
            sync.action_address("sf_east:set_owner"),
            AddressResolution::Resolved(_)
        ));
        assert!(sync.collection_addresses().iter().all(|row| {
            !(row.integration.as_deref() == Some("sf_east") && row.name == "invoice__c")
        }));
        assert!(sync
            .collection_addresses()
            .iter()
            .any(|row| { row.integration.as_deref() == Some("sf_east") && row.name == "account" }));
        let shop = VersionSchema::new_read_only(store.clone(), "alice/shop", VERSION);
        assert!(matches!(
            shop.collection_address("ben/sync.sf_east:invoice__c"),
            AddressResolution::Resolved(ref address)
                if address.project == "ben/sync"
                    && address.local == "sf_east:invoice__c"
                    && matches!(address.kind, CollectionAddressKind::Ambiguous)
        ));

        // Planted past the write check. A label-only write and a re-send of
        // the stored type leave it. Naming the colliding list refuses it.
        store
            .integrations()
            .create(Integration {
                project: "ben/sync".into(),
                version: VERSION.into(),
                name: "sf_planted".into(),
                r#type: "acme/salesforce.salesforce".into(),
                collections: vec![inline_collection("account", "Planted", "Planted")],
                ..Integration::default()
            })
            .unwrap();
        sync.update_integration(
            "sf_planted",
            IntegrationUpdate {
                label: Some("Planted east".into()),
                ..IntegrationUpdate::default()
            },
        )
        .unwrap();
        sync.update_integration(
            "sf_planted",
            IntegrationUpdate {
                r#type: Some("acme/salesforce.salesforce".into()),
                ..IntegrationUpdate::default()
            },
        )
        .unwrap();
        assert!(matches!(
            sync.collection_address("sf_planted:account"),
            AddressResolution::Resolved(ref address)
                if matches!(address.kind, CollectionAddressKind::Ambiguous)
        ));
        let planted = sync
            .update_integration(
                "sf_planted",
                IntegrationUpdate {
                    collections: Some(vec![inline_collection("account", "Still", "Still")]),
                    ..IntegrationUpdate::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert!(planted.contains("sf_planted:account"), "{planted}");
        assert_eq!(
            sync.integration("sf_planted").unwrap().collections()[0].label(),
            "Planted"
        );

        let config = crate::http::project_config::ProjectConfig::new(store.clone(), "ben", "sync");
        let copied = config
            .copy_version(VERSION, "1.0.0")
            .unwrap_err()
            .to_string();
        assert_eq!(
            copied,
            "ambiguous address sf_east:invoice__c: type 'acme/salesforce.salesforce' offers collection 'invoice__c' and integration 'sf_east' declares it"
        );
        assert!(!store
            .manifests()
            .has(&Manifest::to_path("ben/sync", "1.0.0")));

        let resent = sync
            .update_manifest(
                ManifestUpdate {
                    dependencies: Some(vec!["acme/salesforce@0.0.1-dev".into()]),
                    ..ManifestUpdate::default()
                },
                |_| true,
            )
            .unwrap_err()
            .to_string();
        assert!(resent.contains("sf_east:invoice__c"), "{resent}");
        assert_eq!(
            sync.manifest().unwrap().dependencies(),
            &["acme/salesforce@0.0.1-dev".to_string()]
        );

        // A manifest write that does not set dependencies leaves the collision
        // alone. Dropping the dependency removes the standard side.
        sync.update_manifest(
            ManifestUpdate {
                public_permission_sets: Some(vec!["readers".into()]),
                ..ManifestUpdate::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(
            sync.manifest().unwrap().public_permission_sets(),
            &["readers".to_string()]
        );
        sync.update_manifest(
            ManifestUpdate {
                dependencies: Some(Vec::new()),
                ..ManifestUpdate::default()
            },
            |_| true,
        )
        .unwrap();
        let cleared = VersionSchema::new(store, "ben/sync", VERSION);
        assert!(matches!(
            cleared.collection_address("sf_east:invoice__c"),
            AddressResolution::Resolved(ref address)
                if matches!(address.kind, CollectionAddressKind::Custom)
        ));
    }
}

#[cfg(test)]
mod action_param_checks {
    use super::check_action_params;
    use crate::{ActionParam, ActionParamOption};

    fn param(name: &str, ty: &str, options: &[(&str, &str)]) -> ActionParam {
        ActionParam {
            name: name.into(),
            r#type: ty.into(),
            options: options
                .iter()
                .map(|(value, label)| ActionParamOption {
                    value: (*value).into(),
                    label: (*label).into(),
                })
                .collect(),
            ..ActionParam::default()
        }
    }

    #[test]
    fn names_types_and_options() {
        assert!(check_action_params(&[
            param("qty", "integer", &[]),
            param("size", "string", &[("s", "Small")]),
        ])
        .is_ok());

        let err = check_action_params(&[param("Qty", "string", &[])]).unwrap_err();
        assert!(err.to_string().contains("slug"), "{err}");

        let err = check_action_params(&[param("qty", "integer", &[]), param("qty", "string", &[])])
            .unwrap_err();
        assert!(err.to_string().contains("more than once"), "{err}");

        let err = check_action_params(&[param("tags", "list", &[])]).unwrap_err();
        assert!(
            err.to_string().contains("unknown field type 'list'"),
            "{err}"
        );

        let err = check_action_params(&[param("qty", "integer", &[("1", "One")])]).unwrap_err();
        assert!(
            err.to_string().contains("only meaningful for type string"),
            "{err}"
        );
    }
}
