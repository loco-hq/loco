//! `POST /data/query`: parse a batch of named read queries, resolve their
//! names strictly, type-check them, and turn each into a `LakeQuery`
//! (`docs/query.md`, sections 3–6).
//!
//! The handler (`handlers/data.rs`) owns authorization and the one adapter
//! call per batch. This module is the part that needs no request: the wire
//! shape, name resolution, and cursors.
//!
//! Names follow the rule in `CLAUDE.md` ("Name resolution"), strictly: a bare
//! collection or field name means the project that owns the running version,
//! and a dependency's must be written `{user}/{project}.{name}`. Resolution
//! goes through [`VersionSchema::collection_in`] / [`VersionSchema::field_in`],
//! never the fall-through lookups.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Map, Value as Json};
use sha2::{Digest, Sha256};

use loco_lake::{CompareOp, Direction, FieldRef, Filter, LakeQuery, OrderKey, SystemField, Value};

use crate::http::paths::collection_key;
use crate::http::version_schema::VersionSchema;
pub use crate::validation::Diagnostic;

/// Queries per batch.
pub const MAX_QUERIES: usize = 20;
/// Largest `limit` a query may ask for.
pub const MAX_LIMIT: usize = 500;
/// `limit` when a query does not give one.
pub const DEFAULT_LIMIT: usize = 50;
/// Nesting depth of `where`; a lone comparison is depth 1.
pub const MAX_DEPTH: usize = 8;
/// Comparisons in one `where`.
pub const MAX_COMPARISONS: usize = 100;

/// Diagnostic kinds a query can produce, beside `unknown_field` and
/// `type_mismatch` from `validation::kind`.
pub mod kind {
    pub use crate::validation::kind::{TYPE_MISMATCH, UNKNOWN_FIELD};
    pub const UNKNOWN_COLLECTION: &str = "unknown_collection";
    pub const INVALID_QUERY: &str = "invalid_query";
    pub const FORBIDDEN: &str = "forbidden";
    pub const CURSOR_MISMATCH: &str = "cursor_mismatch";
    pub const LIMIT_EXCEEDED: &str = "limit_exceeded";
}

const QUERY_KEYS: [&str; 6] = ["collection", "where", "fields", "order", "limit", "cursor"];

/// Split a request body into `(name, query)` pairs, in the order sent.
///
/// `Err` means the body is not a batch at all, which fails the whole request
/// (400). Everything about an individual query is checked later and fails
/// only that query.
pub fn parse_batch(body: &Json) -> Result<Vec<(String, Json)>, String> {
    let Some(obj) = body.as_object() else {
        return Err("body must be a JSON object".into());
    };
    if let Some(key) = obj.keys().find(|k| *k != "queries") {
        return Err(format!("unknown key '{key}'; a batch has only 'queries'"));
    }
    let Some(queries) = obj.get("queries").and_then(Json::as_object) else {
        return Err("'queries' must be an object of named queries".into());
    };
    if queries.len() > MAX_QUERIES {
        return Err(format!(
            "a batch holds at most {MAX_QUERIES} queries, got {}",
            queries.len()
        ));
    }
    if let Some(bad) = queries.keys().find(|k| !is_query_name(k)) {
        return Err(format!("query name '{bad}' must match [a-z_][a-z0-9_]*"));
    }
    Ok(queries
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

fn is_query_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | '_'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

/// The collection a query names, resolved.
#[derive(Debug, Clone)]
pub struct Target {
    /// Bare name, e.g. `lot`.
    pub name: String,
    /// Owning project: the running project or a direct dependency.
    pub project: String,
}

impl Target {
    pub fn key(&self) -> String {
        collection_key(&self.project, &self.name)
    }
}

/// A query ready for the lake, with what the response needs from it.
#[derive(Debug)]
pub struct Plan {
    pub target: Target,
    pub lake: LakeQuery,
    /// Binds a cursor to this query's collection, `where`, and `order`.
    pub hash: String,
}

/// Resolve a query's `collection`. Done first and on its own because the
/// handler authorizes on the result before looking at anything else.
pub fn target(schema: &VersionSchema, name: &str, raw: &Json) -> Result<Target, Diagnostic> {
    let Some(obj) = raw.as_object() else {
        return Err(error(
            kind::INVALID_QUERY,
            name.to_string(),
            "a query must be a JSON object".into(),
        ));
    };
    let path = format!("{name}/collection");
    let Some(collection) = obj.get("collection").and_then(Json::as_str) else {
        return Err(error(
            kind::INVALID_QUERY,
            path,
            "'collection' is required and must be a string".into(),
        ));
    };
    let (project, bare) = split_name(collection).unwrap_or((schema.project_id(), collection));
    if schema.collection_in(project, bare).is_none() {
        let why = if project == schema.project_id() {
            format!("no collection '{bare}' in {project}")
        } else if schema.visible_version(project).is_none() {
            format!(
                "'{project}' is not a direct dependency of {}",
                schema.project_id()
            )
        } else {
            format!("no collection '{bare}' in {project}")
        };
        return Err(error(
            kind::UNKNOWN_COLLECTION,
            path,
            format!("unknown collection '{collection}': {why}"),
        ));
    }
    Ok(Target {
        name: bare.to_string(),
        project: project.to_string(),
    })
}

/// `{user}/{project}.{name}` → `(project, name)`. `None` for a bare name.
fn split_name(name: &str) -> Option<(&str, &str)> {
    let (project, bare) = name.rsplit_once('.')?;
    Some((project, bare))
}

/// Everything after the collection: `where`, `fields`, `order`, `limit`,
/// `cursor`. Collects every problem before giving up, so a client sees them
/// all at once.
pub fn plan(
    schema: &VersionSchema,
    name: &str,
    raw: &Json,
    target: Target,
) -> Result<Plan, Vec<Diagnostic>> {
    let obj = raw.as_object().expect("target() accepted an object");
    let mut cx = Resolver {
        schema,
        target: &target,
        diags: Vec::new(),
    };

    for key in obj.keys() {
        if !QUERY_KEYS.contains(&key.as_str()) {
            cx.error(
                kind::INVALID_QUERY,
                format!("{name}/{key}"),
                format!("unknown query key '{key}'"),
            );
        }
    }

    let filter = obj.get("where").and_then(|w| {
        let mut count = 0;
        cx.condition(w, &format!("{name}/where"), 1, &mut count)
    });
    let fields = obj.get("fields").and_then(|f| cx.fields(f, name));
    let order = match obj.get("order") {
        Some(o) => cx.order(o, name),
        // The API's default. The lake only appends `id`; it has no default of
        // its own.
        None => Some(vec![
            OrderKey::asc(FieldRef::System(SystemField::CreatedAt)),
            OrderKey::asc(FieldRef::System(SystemField::Id)),
        ]),
    };
    let limit = cx.limit(obj.get("limit"), name);

    let (Some(order), Some(limit)) = (order, limit) else {
        return Err(cx.diags);
    };
    if !cx.diags.is_empty() {
        return Err(cx.diags);
    }

    let mut lake = LakeQuery::new(target.key(), limit);
    lake.filter = filter;
    lake.order = order;
    lake.fields = fields;
    let hash = query_hash(&lake);

    match obj.get("cursor") {
        None | Some(Json::Null) => {}
        Some(Json::String(cursor)) => {
            match decode_cursor(cursor, &hash, lake.effective_order().len()) {
                Ok(after) => lake.after = Some(after),
                Err((kind, message)) => {
                    return Err(vec![error(kind, format!("{name}/cursor"), message)]);
                }
            }
        }
        Some(_) => {
            return Err(vec![error(
                kind::INVALID_QUERY,
                format!("{name}/cursor"),
                "'cursor' must be a string or null".into(),
            )]);
        }
    }

    Ok(Plan { target, lake, hash })
}

fn error(kind: &str, path: String, message: String) -> Diagnostic {
    Diagnostic::error(kind, Some(path), message)
}

/// The declared type of a field, as far as filtering cares.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty {
    String,
    Integer,
    Float,
    Boolean,
    /// Not filterable or orderable in v1 (#18).
    List,
    /// A type string this module does not know. Any scalar passes, as in
    /// `validation.rs`.
    Other,
}

impl Ty {
    fn of(declared: &str) -> Ty {
        match declared {
            "string" => Ty::String,
            "integer" => Ty::Integer,
            "float" => Ty::Float,
            "boolean" => Ty::Boolean,
            "list" => Ty::List,
            _ => Ty::Other,
        }
    }

    /// `None` when `value` fits; otherwise the value's type name. `Null`
    /// always fits: `eq null` is how a query asks for missing.
    fn mismatch(self, value: &Value) -> Option<&'static str> {
        let ok = matches!(
            (self, value),
            (_, Value::Null)
                | (Ty::Other, _)
                | (Ty::String, Value::String(_))
                | (Ty::Integer, Value::Integer(_))
                | (Ty::Float, Value::Integer(_) | Value::Float(_))
                | (Ty::Boolean, Value::Boolean(_))
        );
        (!ok).then(|| type_name(value))
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::Null => "null",
    }
}

/// Field names this module will not send to the lake. The lake rejects `"`
/// with `Error::InvalidQuery`, which fails the whole batch, not one query.
/// Until #70's path fix, sqlite also misreads `\` (silent miss) and NUL
/// (`Error::Internal`). Schema field names are slugs and never contain any
/// of these; the check makes sure none ever reaches the lake.
// TODO(#70): match what #70 lands — drop `\` and NUL if it escapes them,
// and `"` too if it stops rejecting it.
fn lake_accepts_field_name(name: &str) -> bool {
    !name.contains(['"', '\\', '\0'])
}

const SYSTEM_FIELDS: [(&str, SystemField); 6] = [
    ("$id", SystemField::Id),
    ("$created_at", SystemField::CreatedAt),
    ("$created_by", SystemField::CreatedBy),
    ("$updated_at", SystemField::UpdatedAt),
    ("$updated_by", SystemField::UpdatedBy),
    ("$owner", SystemField::Owner),
];

fn system_name(field: SystemField) -> &'static str {
    SYSTEM_FIELDS
        .iter()
        .find(|(_, f)| *f == field)
        .map(|(n, _)| *n)
        .expect("every system field is listed")
}

struct Resolver<'a> {
    schema: &'a VersionSchema,
    target: &'a Target,
    diags: Vec<Diagnostic>,
}

impl Resolver<'_> {
    fn error(&mut self, kind: &str, path: String, message: String) {
        self.diags.push(error(kind, path, message));
    }

    /// A field name → what the lake reads and its declared type. Bare means
    /// the running project's field on this collection; `{user}/{project}.x`
    /// means that direct dependency's.
    fn field(&mut self, raw: &Json, path: String) -> Option<(FieldRef, Ty)> {
        let Some(name) = raw.as_str() else {
            self.error(
                kind::INVALID_QUERY,
                path,
                "a field name must be a string".into(),
            );
            return None;
        };
        if name.starts_with('$') {
            return match SYSTEM_FIELDS.iter().find(|(n, _)| *n == name) {
                Some((_, f)) => Some((FieldRef::System(*f), Ty::String)),
                None => {
                    self.error(
                        kind::UNKNOWN_FIELD,
                        path,
                        format!("unknown system field '{name}'"),
                    );
                    None
                }
            };
        }
        let (project, bare) = split_name(name).unwrap_or((self.schema.project_id(), name));
        match self.schema.field_in(project, &self.target.name, bare) {
            Some(_) if !lake_accepts_field_name(bare) => {
                self.error(
                    kind::INVALID_QUERY,
                    path,
                    format!("field '{name}' has a name that cannot be queried"),
                );
                None
            }
            Some(field) => Some((FieldRef::field(bare), Ty::of(field.r#type()))),
            None => {
                let collection = format!("{}.{}", self.target.project, self.target.name);
                let message = if self.schema.visible_version(project).is_none() {
                    format!(
                        "unknown field '{name}': '{project}' is not a direct dependency of {}",
                        self.schema.project_id()
                    )
                } else {
                    format!(
                        "unknown field '{name}': {project} declares no '{bare}' on {collection}"
                    )
                };
                self.error(kind::UNKNOWN_FIELD, path, message);
                None
            }
        }
    }

    fn scalar(&mut self, raw: &Json, path: &str) -> Option<Value> {
        match raw {
            Json::Null => Some(Value::Null),
            Json::Bool(b) => Some(Value::Boolean(*b)),
            Json::String(s) => Some(Value::String(s.clone())),
            Json::Number(n) => match n.as_i64() {
                Some(i) => Some(Value::Integer(i)),
                None => n.as_f64().map(Value::Float),
            },
            Json::Array(_) | Json::Object(_) => {
                self.error(
                    kind::INVALID_QUERY,
                    path.to_string(),
                    "value must be a scalar".into(),
                );
                None
            }
        }
    }

    /// One `where` node. `count` is comparisons seen so far in this query.
    fn condition(
        &mut self,
        raw: &Json,
        path: &str,
        depth: usize,
        count: &mut usize,
    ) -> Option<Filter> {
        if depth > MAX_DEPTH {
            self.error(
                kind::LIMIT_EXCEEDED,
                path.to_string(),
                format!("'where' may nest at most {MAX_DEPTH} deep"),
            );
            return None;
        }
        let Some(obj) = raw.as_object() else {
            self.error(
                kind::INVALID_QUERY,
                path.to_string(),
                "a condition must be a JSON object".into(),
            );
            return None;
        };

        for combinator in ["and", "or", "not"] {
            let Some(inner) = obj.get(combinator) else {
                continue;
            };
            if obj.len() != 1 {
                self.error(
                    kind::INVALID_QUERY,
                    path.to_string(),
                    format!("'{combinator}' must be the only key in its condition"),
                );
                return None;
            }
            let path = format!("{path}/{combinator}");
            if combinator == "not" {
                return self
                    .condition(inner, &path, depth + 1, count)
                    .map(|f| Filter::Not(Box::new(f)));
            }
            let Some(items) = inner.as_array() else {
                self.error(
                    kind::INVALID_QUERY,
                    path,
                    format!("'{combinator}' takes a list of conditions"),
                );
                return None;
            };
            let parts: Vec<Option<Filter>> = items
                .iter()
                .enumerate()
                .map(|(i, c)| self.condition(c, &format!("{path}/{i}"), depth + 1, count))
                .collect();
            let parts: Option<Vec<Filter>> = parts.into_iter().collect();
            return parts.map(|p| {
                if combinator == "and" {
                    Filter::And(p)
                } else {
                    Filter::Or(p)
                }
            });
        }

        *count += 1;
        if *count == MAX_COMPARISONS + 1 {
            self.error(
                kind::LIMIT_EXCEEDED,
                path.to_string(),
                format!("'where' may hold at most {MAX_COMPARISONS} comparisons"),
            );
        }
        self.comparison(obj, path)
    }

    fn comparison(&mut self, obj: &Map<String, Json>, path: &str) -> Option<Filter> {
        for key in obj.keys() {
            if !matches!(key.as_str(), "field" | "op" | "value") {
                self.error(
                    kind::INVALID_QUERY,
                    format!("{path}/{key}"),
                    format!("unknown condition key '{key}'"),
                );
            }
        }
        let (Some(field), Some(op), Some(value)) =
            (obj.get("field"), obj.get("op"), obj.get("value"))
        else {
            self.error(
                kind::INVALID_QUERY,
                path.to_string(),
                "a comparison needs 'field', 'op', and 'value'; a combinator is 'and', 'or', or 'not'"
                    .into(),
            );
            return None;
        };
        let field = self.field(field, format!("{path}/field"));
        let op_path = format!("{path}/op");
        let value_path = format!("{path}/value");

        let Some(op) = op.as_str() else {
            self.error(kind::INVALID_QUERY, op_path, "'op' must be a string".into());
            return None;
        };
        let (field, ty) = field?;
        if ty == Ty::List {
            self.error(
                kind::INVALID_QUERY,
                format!("{path}/field"),
                "list fields cannot be filtered yet".into(),
            );
            return None;
        }

        match op {
            "exists" => match value {
                Json::Bool(exists) => Some(Filter::Exists {
                    field,
                    exists: *exists,
                }),
                _ => {
                    self.error(
                        kind::INVALID_QUERY,
                        value_path,
                        "'exists' takes true or false".into(),
                    );
                    None
                }
            },
            "in" => {
                let Some(items) = value.as_array().filter(|a| !a.is_empty()) else {
                    self.error(
                        kind::INVALID_QUERY,
                        value_path,
                        "'in' takes a non-empty list of scalars".into(),
                    );
                    return None;
                };
                let values: Vec<Option<Value>> = items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| self.typed(v, ty, &format!("{value_path}/{i}")))
                    .collect();
                let values: Option<Vec<Value>> = values.into_iter().collect();
                values.map(|values| Filter::In { field, values })
            }
            _ => {
                let Some(op) = compare_op(op) else {
                    self.error(
                        kind::INVALID_QUERY,
                        op_path,
                        format!(
                            "unknown op '{op}'; expected eq, ne, lt, lte, gt, gte, in, or exists"
                        ),
                    );
                    return None;
                };
                let value = self.typed(value, ty, &value_path)?;
                if !matches!(op, CompareOp::Eq | CompareOp::Ne)
                    && !matches!(
                        value,
                        Value::Integer(_) | Value::Float(_) | Value::String(_)
                    )
                {
                    self.error(
                        kind::INVALID_QUERY,
                        value_path,
                        "ordering ops take a number or a string".into(),
                    );
                    return None;
                }
                Some(Filter::Compare { field, op, value })
            }
        }
    }

    /// A scalar checked against the field's declared type.
    fn typed(&mut self, raw: &Json, ty: Ty, path: &str) -> Option<Value> {
        let value = self.scalar(raw, path)?;
        if let Some(actual) = ty.mismatch(&value) {
            self.error(
                kind::TYPE_MISMATCH,
                path.to_string(),
                format!("expected {}, got {actual}", ty_name(ty)),
            );
            return None;
        }
        Some(value)
    }

    fn fields(&mut self, raw: &Json, name: &str) -> Option<Vec<String>> {
        let path = format!("{name}/fields");
        let Some(items) = raw.as_array() else {
            self.error(
                kind::INVALID_QUERY,
                path,
                "'fields' must be a list of field names".into(),
            );
            return None;
        };
        let mut keys = Vec::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let path = format!("{path}/{i}");
            match self.field(item, path.clone()) {
                Some((FieldRef::Field(key), _)) => keys.push(key),
                Some((FieldRef::System(_), _)) => {
                    self.error(
                        kind::INVALID_QUERY,
                        path,
                        "system fields are always returned; leave them out of 'fields'".into(),
                    );
                    ok = false;
                }
                None => ok = false,
            }
        }
        ok.then_some(keys)
    }

    fn order(&mut self, raw: &Json, name: &str) -> Option<Vec<OrderKey>> {
        let path = format!("{name}/order");
        let Some(items) = raw.as_array() else {
            self.error(
                kind::INVALID_QUERY,
                path,
                "'order' must be a list of {\"field\", \"dir\"}".into(),
            );
            return None;
        };
        let mut keys = Vec::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let path = format!("{path}/{i}");
            let Some(obj) = item.as_object() else {
                self.error(
                    kind::INVALID_QUERY,
                    path,
                    "an order key must be a JSON object".into(),
                );
                ok = false;
                continue;
            };
            if let Some(key) = obj.keys().find(|k| !matches!(k.as_str(), "field" | "dir")) {
                self.error(
                    kind::INVALID_QUERY,
                    format!("{path}/{key}"),
                    format!("unknown order key '{key}'"),
                );
                ok = false;
            }
            let dir = match obj.get("dir").map(|d| d.as_str()) {
                None | Some(Some("asc")) => Some(Direction::Asc),
                Some(Some("desc")) => Some(Direction::Desc),
                Some(_) => {
                    self.error(
                        kind::INVALID_QUERY,
                        format!("{path}/dir"),
                        "'dir' must be \"asc\" or \"desc\"".into(),
                    );
                    None
                }
            };
            let Some(field) = obj.get("field") else {
                self.error(
                    kind::INVALID_QUERY,
                    path,
                    "an order key needs 'field'".into(),
                );
                ok = false;
                continue;
            };
            let field = self.field(field, format!("{path}/field"));
            match (field, dir) {
                (Some((_, Ty::List)), _) => {
                    self.error(
                        kind::INVALID_QUERY,
                        format!("{path}/field"),
                        "list fields cannot be ordered by".into(),
                    );
                    ok = false;
                }
                (Some((field, _)), Some(dir)) => keys.push(OrderKey { field, dir }),
                _ => ok = false,
            }
        }
        ok.then_some(keys)
    }

    fn limit(&mut self, raw: Option<&Json>, name: &str) -> Option<usize> {
        let path = format!("{name}/limit");
        let Some(raw) = raw else {
            return Some(DEFAULT_LIMIT);
        };
        match raw.as_u64() {
            Some(0) | None => {
                self.error(
                    kind::INVALID_QUERY,
                    path,
                    "'limit' must be a positive integer".into(),
                );
                None
            }
            Some(n) if n > MAX_LIMIT as u64 => {
                self.error(
                    kind::LIMIT_EXCEEDED,
                    path,
                    format!("'limit' may be at most {MAX_LIMIT}, got {n}"),
                );
                None
            }
            Some(n) => Some(n as usize),
        }
    }
}

fn ty_name(ty: Ty) -> &'static str {
    match ty {
        Ty::String => "string",
        Ty::Integer => "integer",
        Ty::Float => "float",
        Ty::Boolean => "boolean",
        Ty::List => "list",
        Ty::Other => "scalar",
    }
}

fn compare_op(op: &str) -> Option<CompareOp> {
    Some(match op {
        "eq" => CompareOp::Eq,
        "ne" => CompareOp::Ne,
        "lt" => CompareOp::Lt,
        "lte" => CompareOp::Lte,
        "gt" => CompareOp::Gt,
        "gte" => CompareOp::Gte,
        _ => return None,
    })
}

// --- Cursors ---
//
// A cursor is base64url (no padding) of `{"h": hash, "k": [values]}`: the
// effective order-key values of the last record returned, plus a hash of the
// resolved collection, filter, and order. It is opaque and unsigned — a
// tampered cursor can only select another page of a query the caller may
// already run. `limit` and `fields` are not in the hash, so they may change
// between pages.

/// Hash of what a cursor is only valid for. Taken over the *resolved* query,
/// so `lot` and `brickos/inventory.lot`, or an omitted `dir` and `"asc"`,
/// are the same query.
fn query_hash(lake: &LakeQuery) -> String {
    let canonical = json!({
        "collection": lake.collection,
        "where": lake.filter.as_ref().map(filter_json),
        "order": lake
            .effective_order()
            .iter()
            .map(|k| json!([field_json(&k.field), matches!(k.dir, Direction::Desc)]))
            .collect::<Vec<_>>(),
    });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn field_json(field: &FieldRef) -> Json {
    match field {
        FieldRef::System(s) => json!(system_name(*s)),
        FieldRef::Field(name) => json!(["field", name]),
    }
}

fn filter_json(filter: &Filter) -> Json {
    match filter {
        Filter::Compare { field, op, value } => {
            json!(["cmp", field_json(field), format!("{op:?}"), value])
        }
        Filter::In { field, values } => json!(["in", field_json(field), values]),
        Filter::Exists { field, exists } => json!(["exists", field_json(field), exists]),
        Filter::And(fs) => json!(["and", fs.iter().map(filter_json).collect::<Vec<_>>()]),
        Filter::Or(fs) => json!(["or", fs.iter().map(filter_json).collect::<Vec<_>>()]),
        Filter::Not(f) => json!(["not", filter_json(f)]),
    }
}

pub fn encode_cursor(hash: &str, next: &[Value]) -> String {
    URL_SAFE_NO_PAD.encode(json!({ "h": hash, "k": next }).to_string())
}

/// The `after` values in `cursor`, if it was issued for a query with `hash`.
fn decode_cursor(
    cursor: &str,
    hash: &str,
    keys: usize,
) -> Result<Vec<Value>, (&'static str, String)> {
    #[derive(serde::Deserialize)]
    struct Cursor {
        h: String,
        k: Vec<Value>,
    }
    let malformed = || (kind::INVALID_QUERY, "malformed cursor".to_string());
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| malformed())?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
    if cursor.h != hash {
        return Err((
            kind::CURSOR_MISMATCH,
            "cursor was issued for a different collection, where, or order".into(),
        ));
    }
    if cursor.k.len() != keys {
        return Err(malformed());
    }
    Ok(cursor.k)
}

/// Read-drift warnings for one query's records, with paths
/// `{query}/{record id}/{field}`.
pub fn record_diagnostics(
    schema: &VersionSchema,
    query: &str,
    collection: &str,
    records: &[loco_lake::Record],
) -> Vec<Diagnostic> {
    crate::validation::validate_records(
        schema,
        collection,
        records.iter().map(|r| (r.id.as_str(), &r.fields)),
        crate::validation::ValidationMode::Read,
    )
    .prefix_paths(query)
    .diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names() {
        assert!(is_query_name("lots"));
        assert!(is_query_name("_x9"));
        assert!(!is_query_name(""));
        assert!(!is_query_name("9lots"));
        assert!(!is_query_name("Lots"));
        assert!(!is_query_name("a-b"));
    }

    #[test]
    fn batch_shape() {
        assert!(parse_batch(&json!([])).is_err());
        assert!(parse_batch(&json!({})).is_err());
        assert!(parse_batch(&json!({"queries": [], "x": 1})).is_err());
        assert!(parse_batch(&json!({"queries": {"Bad": {}}})).is_err());
        let many: Map<String, Json> = (0..=MAX_QUERIES)
            .map(|i| (format!("q{i}"), json!({})))
            .collect();
        assert!(parse_batch(&json!({ "queries": many })).is_err());
        let ok = parse_batch(&json!({"queries": {"a": 1, "b": {}}})).unwrap();
        assert_eq!(ok.len(), 2);
    }

    #[test]
    fn cursor_round_trips_and_binds_to_hash() {
        let next = vec![
            Value::Integer(3),
            Value::Float(0.1),
            Value::Null,
            Value::String("a".into()),
        ];
        let c = encode_cursor("abc", &next);
        assert_eq!(decode_cursor(&c, "abc", 4).unwrap(), next);
        assert_eq!(
            decode_cursor(&c, "abd", 4).unwrap_err().0,
            kind::CURSOR_MISMATCH
        );
        assert_eq!(
            decode_cursor(&c, "abc", 3).unwrap_err().0,
            kind::INVALID_QUERY
        );
        assert_eq!(
            decode_cursor("!!", "abc", 4).unwrap_err().0,
            kind::INVALID_QUERY
        );
    }

    #[test]
    fn hash_covers_where_and_order_not_limit_or_fields() {
        let mut a = LakeQuery::new("p/q.lot", 10);
        a.filter = Some(Filter::Compare {
            field: FieldRef::field("qty"),
            op: CompareOp::Gt,
            value: Value::Integer(0),
        });
        let mut b = a.clone();
        b.limit = 99;
        b.fields = Some(vec!["qty".into()]);
        assert_eq!(query_hash(&a), query_hash(&b));

        let mut c = a.clone();
        c.filter = Some(Filter::Compare {
            field: FieldRef::field("qty"),
            op: CompareOp::Gt,
            value: Value::Integer(1),
        });
        assert_ne!(query_hash(&a), query_hash(&c));

        let mut d = a.clone();
        d.order = vec![OrderKey::desc(FieldRef::field("qty"))];
        assert_ne!(query_hash(&a), query_hash(&d));

        // The trailing `id` the lake appends is the same query either way.
        let mut e = a.clone();
        e.order = vec![OrderKey::asc(FieldRef::System(SystemField::Id))];
        assert_eq!(query_hash(&a), query_hash(&e));
    }

    #[test]
    fn field_names_the_lake_rejects_are_caught_here() {
        assert!(lake_accepts_field_name("qty"));
        assert!(!lake_accepts_field_name("a\"b"));
        assert!(!lake_accepts_field_name("a\\b"));
        assert!(!lake_accepts_field_name("a\0b"));
    }

    #[test]
    fn float_field_accepts_integers() {
        assert_eq!(Ty::Float.mismatch(&Value::Integer(1)), None);
        assert_eq!(Ty::Integer.mismatch(&Value::Float(1.5)), Some("float"));
        assert_eq!(Ty::String.mismatch(&Value::Integer(3)), Some("integer"));
        assert_eq!(Ty::Boolean.mismatch(&Value::Null), None);
    }
}
