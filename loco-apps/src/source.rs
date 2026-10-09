//! `CollectionSource`: one async read/write interface for every collection.
//!
//! The lake implements it ([`LakeSource`]) by calling the synchronous
//! [`loco_lake::DataAdapter`] and finishing that call before the future
//! yields, so the adapter lock is not held across an `.await`. An integration
//! source awaits HTTP on the request task. [`LakeSource::purge_dataset`] is
//! not a collection verb: dataset delete calls the secret store and the
//! variable store first, then this. [`LakeSource::adapter`] is the raw
//! adapter action handlers still take, pending #120.
//!
//! A source declares what it will do. A verb, filter, order, limit, or cursor
//! it does not declare is an error, never a widened or truncated page.
//! `fields` projection stays in front of the source.
//!
//! A cache would sit in front of an integration read, and the sidecar merge
//! (#117) would run after it. Neither is built here.

pub use async_trait::async_trait;

use std::collections::HashMap;
use std::sync::Arc;

use loco_lake::{
    Collation, CompareOp, DataAdapter, Direction, FieldRef, Filter, InsertRequest, LakeQuery,
    OrderKey, SystemField, UpdatePatch, Value,
};
use serde::{Deserialize, Serialize};

use crate::actions::{ConfigReadError, Connection};
use crate::validation::{kind, Diagnostic};

/// What a source will honor. Anything else is [`kind::UNSUPPORTED`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub get: bool,
    pub list: bool,
    pub insert: bool,
    pub update: bool,
    pub delete: bool,
    pub query: bool,
    pub eq: bool,
    pub ne: bool,
    pub lt: bool,
    pub lte: bool,
    pub gt: bool,
    pub gte: bool,
    pub op_in: bool,
    pub exists: bool,
    pub and: bool,
    pub or: bool,
    pub not: bool,
    pub asc: bool,
    pub desc: bool,
    pub binary: bool,
    pub natural: bool,
    pub limit: bool,
    pub cursor: bool,
    /// System fields this source will filter or order by. A live source
    /// declares `$id` when it supports query: the server appends `$id` as a
    /// tie-breaker, and that append is a capability error when `$id` is absent.
    pub system: Vec<SystemField>,
}

impl Capabilities {
    /// Every verb and every query operation, including the six lake system fields.
    pub fn lake() -> Self {
        Self {
            get: true,
            list: true,
            insert: true,
            update: true,
            delete: true,
            query: true,
            eq: true,
            ne: true,
            lt: true,
            lte: true,
            gt: true,
            gte: true,
            op_in: true,
            exists: true,
            and: true,
            or: true,
            not: true,
            asc: true,
            desc: true,
            binary: true,
            natural: true,
            limit: true,
            cursor: true,
            system: vec![
                SystemField::Id,
                SystemField::CreatedAt,
                SystemField::CreatedBy,
                SystemField::UpdatedAt,
                SystemField::UpdatedBy,
                SystemField::Owner,
            ],
        }
    }

    /// Nothing. A registered source that forgot to declare capabilities
    /// refuses every call.
    pub fn none() -> Self {
        Self {
            get: false,
            list: false,
            insert: false,
            update: false,
            delete: false,
            query: false,
            eq: false,
            ne: false,
            lt: false,
            lte: false,
            gt: false,
            gte: false,
            op_in: false,
            exists: false,
            and: false,
            or: false,
            not: false,
            asc: false,
            desc: false,
            binary: false,
            natural: false,
            limit: false,
            cursor: false,
            system: Vec::new(),
        }
    }

    fn allows_verb(&self, verb: Verb) -> bool {
        match verb {
            Verb::Get => self.get,
            Verb::List => self.list,
            Verb::Insert => self.insert,
            Verb::Update => self.update,
            Verb::Delete => self.delete,
        }
    }
}

/// A `/data` verb. Query is a capability of its own, checked per query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    Get,
    List,
    Insert,
    Update,
    Delete,
}

impl Verb {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::List => "list",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

/// One call. `connection` is `None` for a lake collection.
pub struct SourceCall<'a> {
    pub dataset_id: &'a str,
    pub connection: Option<&'a Connection>,
}

/// An upstream record. `id` is the upstream id. There are no system fields
/// beside that id: they are not on the JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRecord {
    pub id: String,
    pub fields: HashMap<String, Value>,
}

/// Lake JSON stays [`loco_lake::Record`]. A live record is [`LiveRecord`].
/// Untagged so a lake page serializes exactly as it did before this trait.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SourceRecord {
    Lake(loco_lake::Record),
    Live(LiveRecord),
}

/// One query page. `next` is present only when the source says more rows follow.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePage {
    pub records: Vec<SourceRecord>,
    pub next: Option<Vec<Value>>,
}

/// Why a source call failed. The HTTP layer maps these the way action
/// failures map: lake errors keep today's status, upstream is 502, a missing
/// key is 503, and anything else is 500. `Unsupported` is the same 400 (or
/// per-query `unsupported`) a capability failure gives.
#[derive(Debug)]
pub enum SourceError {
    Lake(loco_lake::Error),
    /// The source cannot honor this call's shape, though its declared
    /// capabilities admit it: [`Capabilities`] are per source, not per
    /// collection. The source returns this before it calls upstream.
    Unsupported {
        message: String,
    },
    /// `status` is the upstream status, or `0` when the call got no response.
    /// `message` must not contain the request URL or a credential.
    Upstream {
        status: u16,
        message: String,
    },
    Unavailable {
        message: String,
    },
    Failed {
        message: String,
    },
}

impl From<loco_lake::Error> for SourceError {
    fn from(err: loco_lake::Error) -> Self {
        Self::Lake(err)
    }
}

impl From<reqwest::Error> for SourceError {
    fn from(err: reqwest::Error) -> Self {
        let status = err.status().map(|status| status.as_u16()).unwrap_or(0);
        // `Display` appends ` for url ({url})`, query string included.
        Self::Upstream {
            status,
            message: err.without_url().to_string(),
        }
    }
}

impl From<ConfigReadError> for SourceError {
    fn from(err: ConfigReadError) -> Self {
        match err {
            ConfigReadError::Unavailable(message) => Self::Unavailable { message },
            // Asking for a name the type does not declare is a bug in the source.
            ConfigReadError::Undeclared { .. } | ConfigReadError::Failed(_) => Self::Failed {
                message: err.to_string(),
            },
        }
    }
}

/// Async collection reads and writes. Object-safe so a registry can hold it.
#[async_trait]
pub trait CollectionSource: Send + Sync {
    fn capabilities(&self) -> Capabilities;

    async fn get(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<Option<SourceRecord>, SourceError>;

    async fn list(
        &self,
        call: SourceCall<'_>,
        collection: &str,
    ) -> Result<Vec<SourceRecord>, SourceError>;

    async fn insert(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        req: InsertRequest,
    ) -> Result<SourceRecord, SourceError>;

    async fn update(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
        patch: UpdatePatch,
    ) -> Result<SourceRecord, SourceError>;

    async fn delete(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<(), SourceError>;

    /// One page per query, in order. A lake source runs the slice as one snapshot.
    async fn query(
        &self,
        call: SourceCall<'_>,
        queries: &[LakeQuery],
    ) -> Result<Vec<SourcePage>, SourceError>;
}

/// The lake behind [`CollectionSource`]. Methods call the adapter synchronously.
pub struct LakeSource {
    adapter: Arc<dyn DataAdapter>,
}

impl LakeSource {
    pub fn new(adapter: Arc<dyn DataAdapter>) -> Self {
        Self { adapter }
    }

    /// The raw adapter. Action handlers take this so they can patch records.
    /// Pending #120. Nothing else in `handlers/` calls it.
    pub(crate) fn adapter(&self) -> Arc<dyn DataAdapter> {
        Arc::clone(&self.adapter)
    }

    /// Remove every lake row for `dataset_id`, including `$secrets` and
    /// `$variables`. Dataset purge calls the stores first, so a store that
    /// is not the lake is cleaned up on the same path. Not a collection verb.
    pub(crate) fn purge_dataset(&self, dataset_id: &str) -> Result<(), loco_lake::Error> {
        self.adapter.delete_dataset(dataset_id)
    }
}

#[async_trait]
#[allow(clippy::unused_async)]
impl CollectionSource for LakeSource {
    fn capabilities(&self) -> Capabilities {
        Capabilities::lake()
    }

    async fn get(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<Option<SourceRecord>, SourceError> {
        // The adapter call finishes before this future yields.
        Ok(self
            .adapter
            .get(call.dataset_id, collection, id)?
            .map(SourceRecord::Lake))
    }

    async fn list(
        &self,
        call: SourceCall<'_>,
        collection: &str,
    ) -> Result<Vec<SourceRecord>, SourceError> {
        Ok(self
            .adapter
            .list(call.dataset_id, collection)?
            .into_iter()
            .map(SourceRecord::Lake)
            .collect())
    }

    async fn insert(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        req: InsertRequest,
    ) -> Result<SourceRecord, SourceError> {
        Ok(SourceRecord::Lake(self.adapter.insert(
            call.dataset_id,
            collection,
            req,
        )?))
    }

    async fn update(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
        patch: UpdatePatch,
    ) -> Result<SourceRecord, SourceError> {
        Ok(SourceRecord::Lake(self.adapter.update(
            call.dataset_id,
            collection,
            id,
            patch,
        )?))
    }

    async fn delete(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<(), SourceError> {
        Ok(self.adapter.delete(call.dataset_id, collection, id)?)
    }

    async fn query(
        &self,
        call: SourceCall<'_>,
        queries: &[LakeQuery],
    ) -> Result<Vec<SourcePage>, SourceError> {
        Ok(self
            .adapter
            .query(call.dataset_id, queries)?
            .into_iter()
            .map(|page| SourcePage {
                records: page.records.into_iter().map(SourceRecord::Lake).collect(),
                next: page.next,
            })
            .collect())
    }
}

/// Order used when a query omits `order`.
///
/// The lake declares `$created_at` and `$id`, so this is the pair cursors
/// already hash. A source that declares `$id` and not `$created_at` gets
/// `$id` alone. Anything else gets an empty list; [`LakeQuery::effective_order`]
/// still appends `$id`, and that append is then a capability error.
pub(crate) fn default_order(caps: &Capabilities) -> Vec<OrderKey> {
    let has = |field: SystemField| caps.system.contains(&field);
    if has(SystemField::CreatedAt) && has(SystemField::Id) {
        vec![
            OrderKey::asc(FieldRef::System(SystemField::CreatedAt)),
            OrderKey::asc(FieldRef::System(SystemField::Id)),
        ]
    } else if has(SystemField::Id) {
        vec![OrderKey::asc(FieldRef::System(SystemField::Id))]
    } else {
        Vec::new()
    }
}

/// Drop live fields the query did not ask for. Lake projection stays inside
/// the adapter, so this is only for [`SourceRecord::Live`].
pub(crate) fn project_live(record: &mut LiveRecord, fields: Option<&[String]>) {
    let Some(keep) = fields else {
        return;
    };
    record
        .fields
        .retain(|name, _| keep.iter().any(|kept| kept == name));
}

pub(crate) fn unsupported_verb(address: &str, verb: Verb) -> Diagnostic {
    unsupported(format!(
        "collection {address} does not support {}",
        verb.name()
    ))
}

pub(crate) fn verb_allowed(caps: &Capabilities, verb: Verb) -> bool {
    caps.allows_verb(verb)
}

/// Capability problems for one planned query. Empty means the source can run it.
///
/// When `query` itself is off, this is one diagnostic and the rest are skipped.
/// Otherwise every problem is reported. The walk uses [`LakeQuery::effective_order`],
/// so the `$id` tie-breaker counts.
pub(crate) fn unsupported_query(
    name: &str,
    address: &str,
    caps: &Capabilities,
    query: &LakeQuery,
) -> Vec<Diagnostic> {
    if !caps.query {
        return vec![at(
            name,
            format!("collection {address} does not support query"),
        )];
    }
    let mut out = Vec::new();
    if let Some(filter) = &query.filter {
        walk_filter(name, address, caps, filter, &mut out);
    }
    for key in query.effective_order() {
        check_system(name, address, caps, &key.field, "ordering", &mut out);
        match key.dir {
            Direction::Asc if !caps.asc => {
                out.push(at(
                    name,
                    format!("direction 'asc' is not supported on {address}"),
                ));
            }
            Direction::Desc if !caps.desc => {
                out.push(at(
                    name,
                    format!("direction 'desc' is not supported on {address}"),
                ));
            }
            _ => {}
        }
        match key.collation {
            Collation::Binary if !caps.binary => {
                out.push(at(
                    name,
                    format!("collation 'binary' is not supported on {address}"),
                ));
            }
            Collation::Natural if !caps.natural => {
                out.push(at(
                    name,
                    format!("collation 'natural' is not supported on {address}"),
                ));
            }
            _ => {}
        }
    }
    if !caps.limit {
        out.push(at(name, format!("limit is not supported on {address}")));
    }
    if query.after.is_some() && !caps.cursor {
        out.push(at(name, format!("cursor is not supported on {address}")));
    }
    out
}

fn walk_filter(
    name: &str,
    address: &str,
    caps: &Capabilities,
    filter: &Filter,
    out: &mut Vec<Diagnostic>,
) {
    match filter {
        Filter::Compare { field, op, .. } => {
            check_system(name, address, caps, field, "filtering", out);
            if !compare_allowed(caps, *op) {
                out.push(at(
                    name,
                    format!("op '{}' is not supported on {address}", op_name(*op)),
                ));
            }
        }
        Filter::In { field, .. } => {
            check_system(name, address, caps, field, "filtering", out);
            if !caps.op_in {
                out.push(at(name, format!("op 'in' is not supported on {address}")));
            }
        }
        Filter::Exists { field, .. } => {
            check_system(name, address, caps, field, "filtering", out);
            if !caps.exists {
                out.push(at(
                    name,
                    format!("op 'exists' is not supported on {address}"),
                ));
            }
        }
        Filter::And(filters) => {
            if !caps.and {
                out.push(at(name, format!("op 'and' is not supported on {address}")));
            }
            for filter in filters {
                walk_filter(name, address, caps, filter, out);
            }
        }
        Filter::Or(filters) => {
            if !caps.or {
                out.push(at(name, format!("op 'or' is not supported on {address}")));
            }
            for filter in filters {
                walk_filter(name, address, caps, filter, out);
            }
        }
        Filter::Not(filter) => {
            if !caps.not {
                out.push(at(name, format!("op 'not' is not supported on {address}")));
            }
            walk_filter(name, address, caps, filter, out);
        }
    }
}

fn check_system(
    name: &str,
    address: &str,
    caps: &Capabilities,
    field: &FieldRef,
    verb: &str,
    out: &mut Vec<Diagnostic>,
) {
    let FieldRef::System(system) = field else {
        return;
    };
    if !caps.system.contains(system) {
        out.push(at(
            name,
            format!(
                "{verb} by {} is not supported on {address}",
                system_name(*system)
            ),
        ));
    }
}

fn compare_allowed(caps: &Capabilities, op: CompareOp) -> bool {
    match op {
        CompareOp::Eq => caps.eq,
        CompareOp::Ne => caps.ne,
        CompareOp::Lt => caps.lt,
        CompareOp::Lte => caps.lte,
        CompareOp::Gt => caps.gt,
        CompareOp::Gte => caps.gte,
    }
}

fn op_name(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "eq",
        CompareOp::Ne => "ne",
        CompareOp::Lt => "lt",
        CompareOp::Lte => "lte",
        CompareOp::Gt => "gt",
        CompareOp::Gte => "gte",
    }
}

fn system_name(field: SystemField) -> &'static str {
    match field {
        SystemField::Id => "$id",
        SystemField::CreatedAt => "$created_at",
        SystemField::CreatedBy => "$created_by",
        SystemField::UpdatedAt => "$updated_at",
        SystemField::UpdatedBy => "$updated_by",
        SystemField::Owner => "$owner",
    }
}

fn unsupported(message: String) -> Diagnostic {
    Diagnostic::error(kind::UNSUPPORTED, None, message)
}

fn at(name: &str, message: String) -> Diagnostic {
    Diagnostic::error(kind::UNSUPPORTED, Some(name.to_string()), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn narrow() -> Capabilities {
        Capabilities {
            query: true,
            eq: true,
            asc: true,
            binary: true,
            limit: true,
            cursor: true,
            system: vec![SystemField::Id],
            ..Capabilities::none()
        }
    }

    #[test]
    fn narrow_source_rejects_gt_and_created_at() {
        let mut query = LakeQuery::new("east:items", 50);
        query.filter = Some(Filter::Compare {
            field: FieldRef::System(SystemField::CreatedAt),
            op: CompareOp::Gt,
            value: Value::String("a".into()),
        });
        query.order = vec![OrderKey::asc(FieldRef::System(SystemField::CreatedAt))];
        let diags = unsupported_query("q", "east:items", &narrow(), &query);
        assert!(diags.iter().all(|diag| diag.kind == kind::UNSUPPORTED));
        assert!(diags.iter().any(|diag| diag.message.contains("op 'gt'")));
        assert!(diags
            .iter()
            .any(|diag| diag.message.contains("filtering by $created_at")));
        assert!(diags
            .iter()
            .any(|diag| diag.message.contains("ordering by $created_at")));
    }

    #[test]
    fn id_tie_breaker_is_a_capability_when_undeclared() {
        let caps = Capabilities {
            query: true,
            asc: true,
            binary: true,
            limit: true,
            ..Capabilities::none()
        };
        let query = LakeQuery::new("east:items", 50);
        let diags = unsupported_query("q", "east:items", &caps, &query);
        assert!(diags
            .iter()
            .any(|diag| diag.message.contains("ordering by $id")));
    }

    #[test]
    fn lake_accepts_its_default_order() {
        let caps = Capabilities::lake();
        let mut query = LakeQuery::new("alice/shop.local", 50);
        query.order = default_order(&caps);
        assert!(unsupported_query("q", "alice/shop.local", &caps, &query).is_empty());
        assert_eq!(
            default_order(&narrow()),
            vec![OrderKey::asc(FieldRef::System(SystemField::Id))]
        );
    }

    #[test]
    fn query_capability_off_skips_the_rest() {
        let query = LakeQuery::new("east:items", 50);
        let diags = unsupported_query("q", "east:items", &Capabilities::none(), &query);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("does not support query"));
    }
}
