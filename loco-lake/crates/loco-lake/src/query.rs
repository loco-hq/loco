//! Resolved read queries over one collection (`docs/query.md`, "Lake").
//!
//! `loco-apps` resolves names, checks types, and authorizes; the lake only
//! evaluates. The semantics here are the reference: `InMemoryAdapter`
//! evaluates them directly and `SqliteAdapter` compiles to SQL that must agree.
//!
//! - A missing field is `Null` for every op except `Exists`.
//! - `Eq(Null)` matches null or missing. `Ne` is exactly `Not(Eq)`.
//! - Ordering ops (`Lt` … `Gte`) never match null, and only compare a number
//!   with a number or a string with a string (bytewise).
//! - Integers and floats are one kind: `3 == 3.0`. Other kinds never match
//!   each other, so a value that drifted to another type does not match.
//! - Sorting is by kind first — null, boolean, number, string — then value.
//!   So nulls sort first ascending and last descending.
//! - The effective order always ends with `id`; `after` holds one value per
//!   effective order key and selects records strictly after it.

use std::cmp::Ordering;

use crate::error::Error;
use crate::record::Record;
use crate::value::Value;

/// Record metadata. Never a key in `Record::fields`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemField {
    Id,
    CreatedAt,
    CreatedBy,
    UpdatedAt,
    UpdatedBy,
    Owner,
}

impl SystemField {
    pub(crate) fn column(self) -> &'static str {
        match self {
            SystemField::Id => "id",
            SystemField::CreatedAt => "created_at",
            SystemField::CreatedBy => "created_by",
            SystemField::UpdatedAt => "updated_at",
            SystemField::UpdatedBy => "updated_by",
            SystemField::Owner => "owner",
        }
    }

    fn get(self, record: &Record) -> &str {
        match self {
            SystemField::Id => &record.id,
            SystemField::CreatedAt => &record.created_at,
            SystemField::CreatedBy => &record.created_by,
            SystemField::UpdatedAt => &record.updated_at,
            SystemField::UpdatedBy => &record.updated_by,
            SystemField::Owner => &record.owner,
        }
    }
}

/// What a filter or order key reads: a system field, or a key in `fields`.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldRef {
    System(SystemField),
    /// A storage key in `Record::fields`.
    Field(String),
}

impl FieldRef {
    pub fn field(name: impl Into<String>) -> Self {
        FieldRef::Field(name.into())
    }

    pub(crate) fn value(&self, record: &Record) -> Value {
        match self {
            FieldRef::System(s) => Value::String(s.get(record).to_string()),
            FieldRef::Field(name) => record.fields.get(name).cloned().unwrap_or(Value::Null),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl CompareOp {
    pub(crate) fn is_ordering(self) -> bool {
        !matches!(self, CompareOp::Eq | CompareOp::Ne)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Compare {
        field: FieldRef,
        op: CompareOp,
        value: Value,
    },
    /// Equal to one of `values`. Must be non-empty.
    In {
        field: FieldRef,
        values: Vec<Value>,
    },
    /// `true`: present and not null. `false`: absent or null.
    Exists {
        field: FieldRef,
        exists: bool,
    },
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderKey {
    pub field: FieldRef,
    pub dir: Direction,
}

impl OrderKey {
    pub fn asc(field: FieldRef) -> Self {
        OrderKey {
            field,
            dir: Direction::Asc,
        }
    }

    pub fn desc(field: FieldRef) -> Self {
        OrderKey {
            field,
            dir: Direction::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LakeQuery {
    /// Collection key, e.g. `brickos/inventory.lot`.
    pub collection: String,
    pub filter: Option<Filter>,
    /// `id` ascending is appended unless the last key is already `id`.
    pub order: Vec<OrderKey>,
    /// At least 1.
    pub limit: usize,
    /// Order-key values of the last record already seen, one per effective
    /// order key (including the trailing `id`).
    pub after: Option<Vec<Value>>,
    /// Keep only these keys in each returned record's `fields`.
    pub fields: Option<Vec<String>>,
}

impl LakeQuery {
    pub fn new(collection: impl Into<String>, limit: usize) -> Self {
        LakeQuery {
            collection: collection.into(),
            filter: None,
            order: Vec::new(),
            limit,
            after: None,
            fields: None,
        }
    }

    /// `order` with the `id` tie-breaker that makes it total.
    pub fn effective_order(&self) -> Vec<OrderKey> {
        let mut order = self.order.clone();
        if !matches!(
            order.last(),
            Some(OrderKey {
                field: FieldRef::System(SystemField::Id),
                ..
            })
        ) {
            order.push(OrderKey::asc(FieldRef::System(SystemField::Id)));
        }
        order
    }

    /// Structural checks both adapters apply before running anything.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.limit == 0 {
            return Err(invalid("limit must be at least 1"));
        }
        if let Some(filter) = &self.filter {
            validate_filter(filter)?;
        }
        let order = self.effective_order();
        for key in &order {
            validate_field(&key.field)?;
        }
        if let Some(after) = &self.after {
            if after.len() != order.len() {
                return Err(invalid(&format!(
                    "after has {} values, order has {} keys",
                    after.len(),
                    order.len()
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub records: Vec<Record>,
    /// Effective order-key values of the last record returned, present only
    /// when more records follow. Pass it back as `after` for the next page.
    pub next: Option<Vec<Value>>,
}

fn invalid(msg: &str) -> Error {
    Error::InvalidQuery(msg.to_string())
}

fn validate_field(field: &FieldRef) -> Result<(), Error> {
    match field {
        // Field names go into a quoted sqlite JSON path, which cannot escape `"`.
        FieldRef::Field(name) if name.contains('"') => {
            Err(invalid(&format!("field name {name:?} contains '\"'")))
        }
        _ => Ok(()),
    }
}

fn validate_filter(filter: &Filter) -> Result<(), Error> {
    match filter {
        Filter::Compare { field, op, value } => {
            validate_field(field)?;
            if op.is_ordering() && !matches!(kind(value), Kind::Number | Kind::String) {
                return Err(invalid("ordering ops need a number or string value"));
            }
            Ok(())
        }
        Filter::In { field, values } => {
            validate_field(field)?;
            if values.is_empty() {
                return Err(invalid("in needs at least one value"));
            }
            Ok(())
        }
        Filter::Exists { field, .. } => validate_field(field),
        Filter::And(filters) | Filter::Or(filters) => filters.iter().try_for_each(validate_filter),
        Filter::Not(inner) => validate_filter(inner),
    }
}

/// Sort class, in sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Null = 0,
    Boolean = 1,
    Number = 2,
    String = 3,
}

pub(crate) fn kind(value: &Value) -> Kind {
    match value {
        Value::Null => Kind::Null,
        Value::Boolean(_) => Kind::Boolean,
        Value::Integer(_) | Value::Float(_) => Kind::Number,
        Value::String(_) => Kind::String,
    }
}

/// Order within one kind; `None` across kinds.
fn compare_same_kind(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        (Value::Boolean(x), Value::Boolean(y)) => Some(x.cmp(y)),
        (Value::String(x), Value::String(y)) => Some(x.as_bytes().cmp(y.as_bytes())),
        (Value::Integer(x), Value::Integer(y)) => Some(x.cmp(y)),
        (Value::Integer(x), Value::Float(y)) => int_float_cmp(*x, *y),
        (Value::Float(x), Value::Integer(y)) => int_float_cmp(*y, *x).map(Ordering::reverse),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        _ => None,
    }
}

/// Exact, as sqlite compares them: no rounding of large integers through f64.
fn int_float_cmp(i: i64, f: f64) -> Option<Ordering> {
    if f.is_nan() {
        return None;
    }
    // 2^63: every i64 is below it, and every f64 at or above it.
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    if f >= TWO_63 {
        return Some(Ordering::Less);
    }
    if f < -TWO_63 {
        return Some(Ordering::Greater);
    }
    let whole = f.trunc();
    Some(i.cmp(&(whole as i64)).then_with(|| {
        // Same integer part: the fraction decides.
        0.0_f64.partial_cmp(&(f - whole)).unwrap_or(Ordering::Equal)
    }))
}

/// Total sort order: kind, then value.
pub(crate) fn sort_cmp(a: &Value, b: &Value) -> Ordering {
    kind(a)
        .cmp(&kind(b))
        .then_with(|| compare_same_kind(a, b).unwrap_or(Ordering::Equal))
}

fn equals(stored: &Value, value: &Value) -> bool {
    compare_same_kind(stored, value) == Some(Ordering::Equal)
}

pub(crate) fn matches(filter: &Filter, record: &Record) -> bool {
    match filter {
        Filter::Compare { field, op, value } => {
            let stored = field.value(record);
            match op {
                CompareOp::Eq => equals(&stored, value),
                CompareOp::Ne => !equals(&stored, value),
                _ => {
                    if kind(&stored) != kind(value) || kind(value) == Kind::Null {
                        return false;
                    }
                    let Some(ord) = compare_same_kind(&stored, value) else {
                        return false;
                    };
                    match op {
                        CompareOp::Lt => ord == Ordering::Less,
                        CompareOp::Lte => ord != Ordering::Greater,
                        CompareOp::Gt => ord == Ordering::Greater,
                        CompareOp::Gte => ord != Ordering::Less,
                        CompareOp::Eq | CompareOp::Ne => unreachable!(),
                    }
                }
            }
        }
        Filter::In { field, values } => {
            let stored = field.value(record);
            values.iter().any(|v| equals(&stored, v))
        }
        Filter::Exists { field, exists } => (field.value(record) != Value::Null) == *exists,
        Filter::And(filters) => filters.iter().all(|f| matches(f, record)),
        Filter::Or(filters) => filters.iter().any(|f| matches(f, record)),
        Filter::Not(inner) => !matches(inner, record),
    }
}

pub(crate) fn order_values(order: &[OrderKey], record: &Record) -> Vec<Value> {
    order.iter().map(|key| key.field.value(record)).collect()
}

/// Compare two effective order-key tuples under `order`.
pub(crate) fn compare_keys(order: &[OrderKey], a: &[Value], b: &[Value]) -> Ordering {
    for ((key, x), y) in order.iter().zip(a).zip(b) {
        let ord = sort_cmp(x, y);
        let ord = match key.dir {
            Direction::Asc => ord,
            Direction::Desc => ord.reverse(),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// Trim to `limit`, compute `next`, and apply the `fields` projection.
/// `rows` is sorted and already past `after`; it may hold one extra record.
pub(crate) fn finish_page(query: &LakeQuery, order: &[OrderKey], mut rows: Vec<Record>) -> Page {
    let more = rows.len() > query.limit;
    rows.truncate(query.limit);
    let next = if more {
        rows.last().map(|r| order_values(order, r))
    } else {
        None
    };
    if let Some(keep) = &query.fields {
        for record in &mut rows {
            record.fields.retain(|k, _| keep.contains(k));
        }
    }
    Page {
        records: rows,
        next,
    }
}
