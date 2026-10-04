//! `loco/bricklink`: the BrickLink store API as a [`CollectionSource`].
//!
//! The type `bricklink` (`schemas/seed/loco/bricklink/`) offers two standard
//! collections, read live on every call. Nothing is cached, so every read
//! spends BrickLink's daily call budget.
//!
//! - `orders` — incoming orders the store has not filed: `GET /orders?direction=in`.
//!   The record id is the BrickLink order id. A `status` filter (`eq` or `in`)
//!   is passed upstream. `get` of a filed order is not found, so `get` and
//!   `list` agree on what the collection holds.
//! - `order_items` — the lines of one order: `GET /orders/{id}/items`. There
//!   is no all-items endpoint, so a list, or a query without an `order_id`
//!   filter (`eq` or `in`), is [`SourceError::Unsupported`]. It never fans
//!   out over every order. Filtering by a filed order's id still returns its
//!   lines. The record id is `{order_id}-{batch}-{inventory_id}`; see
//!   [`item_records`]. No line is dropped.
//!
//! Read-only: insert, update, and delete are not declared. Requests are
//! signed with OAuth 1.0a ([`oauth`]). BrickLink answers most errors as HTTP
//! 200 with an error `meta.code` in the body. Either way the caller gets
//! [`SourceError::Upstream`] with BrickLink's `meta` message, and with any
//! credential, signature, and nonce the request carried removed from it.
//! Only BrickLink's `meta.code` 404 is not found; a bare HTTP 404 is upstream.

pub mod oauth;

use std::cmp::Ordering;
use std::collections::HashMap;

use loco_lake::{compare_keys, equals, FieldRef, Filter, InsertRequest, LakeQuery, Value};
use loco_lake::{CompareOp, SystemField, UpdatePatch};
use reqwest::Url;
use serde_json::Value as Json;

use crate::actions::Connection;
use crate::source::{
    async_trait, Capabilities, CollectionSource, LiveRecord, SourceCall, SourceError, SourcePage,
    SourceRecord,
};

/// Owning project of the integration type. The source registry key.
pub const PROJECT: &str = "loco/bricklink";
/// The integration type's name. The source registry key.
pub const TYPE: &str = "bricklink";

const ORDERS: &str = "orders";
const ORDER_ITEMS: &str = "order_items";
/// The longest upstream message passed to the caller.
const MESSAGE_MAX: usize = 200;

/// The registered source for `(loco/bricklink, bricklink)`.
pub struct BrickLinkSource;

#[async_trait]
impl CollectionSource for BrickLinkSource {
    /// get, list, and query. A query filter is one `eq` or `in`, and which
    /// field it may name depends on the collection (see the module docs).
    /// Order, limit, and cursor are applied here over the full upstream
    /// result, which BrickLink does not page.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            get: true,
            list: true,
            query: true,
            eq: true,
            op_in: true,
            asc: true,
            desc: true,
            binary: true,
            natural: true,
            limit: true,
            cursor: true,
            system: vec![SystemField::Id],
            ..Capabilities::none()
        }
    }

    async fn get(
        &self,
        call: SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<Option<SourceRecord>, SourceError> {
        let upstream = Upstream::new(&call)?;
        let record = match collection {
            ORDERS => {
                if !is_upstream_id(id) {
                    return Ok(None);
                }
                match upstream.get(&format!("/orders/{id}"), &[]).await? {
                    Reply::Data(order) if !is_filed(&order) => order_record(&order),
                    _ => None,
                }
            }
            ORDER_ITEMS => {
                // The id leads with the order id. Read that order's lines
                // and match the whole id, so get and list agree.
                let Some((order_id, _)) = id.split_once('-') else {
                    return Ok(None);
                };
                if !is_upstream_id(order_id) {
                    return Ok(None);
                }
                upstream
                    .items(order_id)
                    .await?
                    .into_iter()
                    .find(|item| item.id == id)
            }
            other => return Err(unknown_collection(other)),
        };
        Ok(record.map(SourceRecord::Live))
    }

    async fn list(
        &self,
        call: SourceCall<'_>,
        collection: &str,
    ) -> Result<Vec<SourceRecord>, SourceError> {
        let records = match collection {
            ORDERS => Upstream::new(&call)?.orders(None).await?,
            ORDER_ITEMS => return Err(items_need_order()),
            other => return Err(unknown_collection(other)),
        };
        Ok(records.into_iter().map(SourceRecord::Live).collect())
    }

    async fn insert(
        &self,
        _: SourceCall<'_>,
        _: &str,
        _: InsertRequest,
    ) -> Result<SourceRecord, SourceError> {
        Err(read_only())
    }

    async fn update(
        &self,
        _: SourceCall<'_>,
        _: &str,
        _: &str,
        _: UpdatePatch,
    ) -> Result<SourceRecord, SourceError> {
        Err(read_only())
    }

    async fn delete(&self, _: SourceCall<'_>, _: &str, _: &str) -> Result<(), SourceError> {
        Err(read_only())
    }

    async fn query(
        &self,
        call: SourceCall<'_>,
        queries: &[LakeQuery],
    ) -> Result<Vec<SourcePage>, SourceError> {
        let mut pages = Vec::with_capacity(queries.len());
        for query in queries {
            // Shape first, so an unsupported query makes no upstream call.
            let condition = condition(query.filter.as_ref())?;
            let records = match query.collection.as_str() {
                ORDERS => {
                    let statuses = match &condition {
                        None => None,
                        Some((FieldRef::Field(name), values)) if name == "status" => {
                            status_param(values)
                        }
                        Some((FieldRef::System(SystemField::Id), _)) => None,
                        Some(_) => {
                            return Err(unsupported("orders can be filtered by status or $id only"))
                        }
                    };
                    Upstream::new(&call)?.orders(statuses.as_deref()).await?
                }
                ORDER_ITEMS => {
                    let order_ids = match &condition {
                        Some((FieldRef::Field(name), values)) if name == "order_id" => values,
                        _ => return Err(items_need_order()),
                    };
                    let upstream = Upstream::new(&call)?;
                    let mut records = Vec::new();
                    let mut seen = Vec::new();
                    for value in order_ids {
                        let Value::String(order_id) = value else {
                            continue;
                        };
                        if !is_upstream_id(order_id) || seen.contains(order_id) {
                            continue;
                        }
                        seen.push(order_id.clone());
                        records.extend(upstream.items(order_id).await?);
                    }
                    records
                }
                other => return Err(unknown_collection(other)),
            };
            pages.push(page(records, condition.as_ref(), query));
        }
        Ok(pages)
    }
}

/// One signed GET at a time, for one connection.
struct Upstream<'a> {
    connection: &'a Connection,
    base_url: String,
    consumer_key: String,
    consumer_secret: String,
    token_value: String,
    token_secret: String,
}

enum Reply {
    Data(Json),
    /// BrickLink's own `meta.code` 404, with its cleaned message.
    NotFound(String),
}

impl<'a> Upstream<'a> {
    fn new(call: &SourceCall<'a>) -> Result<Self, SourceError> {
        let connection = call.connection.ok_or_else(|| SourceError::Failed {
            message: "the BrickLink source requires a connection".into(),
        })?;
        // The required-value check ran before the call, so these are set.
        // An empty stored secret counts as set; BrickLink will refuse it.
        let secret = |name: &str| -> Result<String, SourceError> {
            Ok(connection.secret(name)?.unwrap_or_default())
        };
        Ok(Self {
            connection,
            base_url: connection.variable("base_url")?.unwrap_or_default(),
            consumer_key: secret("consumer_key")?,
            consumer_secret: secret("consumer_secret")?,
            token_value: secret("token_value")?,
            token_secret: secret("token_secret")?,
        })
    }

    /// `GET /orders?direction=in`, with `status` when given.
    async fn orders(&self, status: Option<&str>) -> Result<Vec<LiveRecord>, SourceError> {
        let mut params = vec![("direction", "in")];
        if let Some(status) = status {
            params.push(("status", status));
        }
        // The collection endpoint itself cannot be "not found". Reading that
        // as no orders would tell a pulling app there is nothing to pick.
        let data = match self.get("/orders", &params).await? {
            Reply::Data(data) => data,
            Reply::NotFound(message) => {
                return Err(SourceError::Upstream {
                    status: 404,
                    message,
                })
            }
        };
        Ok(data
            .as_array()
            .map(|orders| orders.iter().filter_map(order_record).collect())
            .unwrap_or_default())
    }

    /// `GET /orders/{order_id}/items`. An unknown order has no items.
    async fn items(&self, order_id: &str) -> Result<Vec<LiveRecord>, SourceError> {
        match self.get(&format!("/orders/{order_id}/items"), &[]).await? {
            Reply::Data(data) => Ok(item_records(order_id, &data)),
            Reply::NotFound(_) => Ok(Vec::new()),
        }
    }

    async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Reply, SourceError> {
        let mut url = Url::parse(&format!("{}{path}", self.base_url.trim_end_matches('/')))
            .map_err(|_| SourceError::Upstream {
                status: 0,
                message: "variable 'base_url' is not a URL".into(),
            })?;
        if !params.is_empty() {
            url.query_pairs_mut().extend_pairs(params);
        }
        let creds = oauth::Credentials {
            consumer_key: &self.consumer_key,
            consumer_secret: &self.consumer_secret,
            token_value: &self.token_value,
            token_secret: &self.token_secret,
        };
        let (nonce, timestamp) = oauth::fresh_nonce_and_timestamp();
        let signature = oauth::signature("GET", &url, &creds, &nonce, &timestamp);
        let header = oauth::header(&creds, &signature, &nonce, &timestamp);
        let response = self
            .connection
            .http()
            .get(url)
            .header(reqwest::header::AUTHORIZATION, header)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        // Everything the request carried that is not public: the four
        // credentials, this request's signature and nonce, raw and as they
        // appear percent-encoded in the header.
        let mut scrub = Vec::new();
        for value in [
            self.consumer_key.as_str(),
            self.consumer_secret.as_str(),
            self.token_value.as_str(),
            self.token_secret.as_str(),
            signature.as_str(),
            nonce.as_str(),
        ] {
            scrub.push(value.to_string());
            scrub.push(oauth::encode(value));
        }
        let scrub: Vec<&str> = scrub.iter().map(String::as_str).collect();
        read_envelope(status.as_u16(), status.canonical_reason(), &body, &scrub)
    }
}

/// BrickLink's `{meta, data}` envelope. Only BrickLink's own `meta.code` 404
/// is not found. A bare HTTP 404 with no envelope (a wrong `base_url`, a
/// path the server does not have) is upstream, like any other `meta.code` or
/// HTTP status outside 2xx.
fn read_envelope(
    status: u16,
    reason: Option<&str>,
    body: &str,
    scrub: &[&str],
) -> Result<Reply, SourceError> {
    let parsed: Option<Json> = serde_json::from_str(body).ok();
    let meta = parsed.as_ref().and_then(|json| json.get("meta"));
    let code = meta
        .and_then(|meta| meta.get("code"))
        .and_then(Json::as_u64)
        .and_then(|code| u16::try_from(code).ok());
    if code == Some(404) {
        let message = meta
            .and_then(meta_message)
            .map(|message| clean(&message, scrub))
            .unwrap_or_else(|| "not found".to_string());
        return Ok(Reply::NotFound(message));
    }
    let failed_code = code.filter(|code| !(200..300).contains(code));
    if failed_code.is_some() || !(200..300).contains(&status) {
        let message = meta
            .and_then(meta_message)
            .map(|message| clean(&message, scrub))
            .unwrap_or_else(|| reason.unwrap_or("unexpected response").to_string());
        return Err(SourceError::Upstream {
            status: failed_code.unwrap_or(status),
            message,
        });
    }
    match parsed.and_then(|mut json| json.get_mut("data").map(Json::take)) {
        Some(data) => Ok(Reply::Data(data)),
        None => Err(SourceError::Upstream {
            status,
            message: "response is not a BrickLink envelope".into(),
        }),
    }
}

/// `{message}: {description}`, or whichever of the two is present.
fn meta_message(meta: &Json) -> Option<String> {
    let part = |key: &str| {
        meta.get(key)
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    match (part("message"), part("description")) {
        (Some(message), Some(description)) if message != description => {
            Some(format!("{message}: {description}"))
        }
        (Some(text), _) | (None, Some(text)) => Some(text.to_string()),
        (None, None) => None,
    }
}

/// Remove every credential the request carried, then cap the length. The
/// message is returned to the caller verbatim.
fn clean(message: &str, scrub: &[&str]) -> String {
    let mut out = message.to_string();
    for secret in scrub.iter().filter(|secret| !secret.is_empty()) {
        out = out.replace(secret, "[redacted]");
    }
    if out.chars().count() > MESSAGE_MAX {
        out = out.chars().take(MESSAGE_MAX).collect::<String>() + "…";
    }
    out
}

/// One order summary or detail → a live record. `None` without an `order_id`.
pub(crate) fn order_record(order: &Json) -> Option<LiveRecord> {
    let id = upstream_id(order.get("order_id")?)?;
    let mut fields = HashMap::new();
    put_string(&mut fields, "status", order.get("status"));
    put_string(&mut fields, "buyer", order.get("buyer_name"));
    put_string(&mut fields, "date", order.get("date_ordered"));
    let cost = order.get("cost");
    let total = cost.and_then(|cost| cost.get("grand_total")).and_then(text);
    let currency = cost
        .and_then(|cost| cost.get("currency_code"))
        .and_then(text);
    if let Some(total) = total {
        let totals = match currency {
            Some(currency) => format!("{currency} {total}"),
            None => total,
        };
        fields.insert("totals".into(), Value::String(totals));
    }
    Some(LiveRecord { id, fields })
}

/// `data` of `GET /orders/{id}/items` → live records, one per line. No line
/// is dropped: a missing line on a pick list is the failure that matters.
///
/// BrickLink returns the lines grouped in batches (a list of lists); a flat
/// list is read as batch 1. One lot can be in more than one batch, so the
/// inventory id alone does not name a line. The id is:
///
/// - `{order_id}-{batch}-{inventory_id}`, `batch` counted from 1;
/// - `{order_id}-{batch}-x{position}` when the line has no inventory id,
///   `position` counted from 1 within the batch;
/// - either, then `-{position}`, if that id is already taken in this order
///   (the same lot twice in one batch).
///
/// Each depends only on that order's response, so the same lines get the
/// same ids on every read, and `get` finds a line by reading its order.
pub(crate) fn item_records(order_id: &str, data: &Json) -> Vec<LiveRecord> {
    let Some(entries) = data.as_array() else {
        return Vec::new();
    };
    let batches: Vec<Vec<&Json>> = if entries.iter().all(Json::is_array) {
        entries
            .iter()
            .map(|batch| {
                batch
                    .as_array()
                    .map(|b| b.iter().collect())
                    .unwrap_or_default()
            })
            .collect()
    } else {
        vec![entries.iter().collect()]
    };
    let mut records: Vec<LiveRecord> = Vec::new();
    for (batch_index, lines) in batches.iter().enumerate() {
        let batch = batch_index + 1;
        for (line_index, item) in lines.iter().enumerate() {
            let position = line_index + 1;
            let inventory_id = item.get("inventory_id").and_then(upstream_id);
            let mut id = match &inventory_id {
                Some(inventory_id) => format!("{order_id}-{batch}-{inventory_id}"),
                None => format!("{order_id}-{batch}-x{position}"),
            };
            if records.iter().any(|record| record.id == id) {
                id = format!("{id}-{position}");
            }
            records.push(item_record(order_id, id, inventory_id, item));
        }
    }
    records
}

fn item_record(
    order_id: &str,
    id: String,
    inventory_id: Option<String>,
    item: &Json,
) -> LiveRecord {
    let mut fields = HashMap::new();
    let part = item.get("item");
    put_string(&mut fields, "item_no", part.and_then(|part| part.get("no")));
    put_string(
        &mut fields,
        "item_type",
        part.and_then(|part| part.get("type")),
    );
    put_integer(&mut fields, "color_id", item.get("color_id"));
    put_string(&mut fields, "color_name", item.get("color_name"));
    put_string(&mut fields, "condition", item.get("new_or_used"));
    put_integer(&mut fields, "qty", item.get("quantity"));
    put_string(&mut fields, "remarks", item.get("remarks"));
    if let Some(inventory_id) = inventory_id {
        fields.insert("inventory_id".into(), Value::String(inventory_id));
    }
    fields.insert("order_id".into(), Value::String(order_id.to_string()));
    LiveRecord { id, fields }
}

fn put_string(fields: &mut HashMap<String, Value>, name: &str, value: Option<&Json>) {
    if let Some(text) = value.and_then(text) {
        fields.insert(name.into(), Value::String(text));
    }
}

fn put_integer(fields: &mut HashMap<String, Value>, name: &str, value: Option<&Json>) {
    if let Some(number) = value.and_then(Json::as_i64) {
        fields.insert(name.into(), Value::Integer(number));
    }
}

/// A JSON string as is, a number as its decimal text. Anything else is absent.
fn text(value: &Json) -> Option<String> {
    match value {
        Json::String(text) => Some(text.clone()),
        Json::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// An order or inventory id: a non-negative integer, as JSON number or digits.
fn upstream_id(value: &Json) -> Option<String> {
    let id = text(value)?;
    is_upstream_id(&id).then_some(id)
}

/// Digits only. Anything else never reaches a URL path.
fn is_upstream_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_filed(order: &Json) -> bool {
    order.get("is_filed").and_then(Json::as_bool) == Some(true)
}

/// The query's one condition as `(field, values)`. The declared capabilities
/// admit only `eq` and `in`, so anything else is a bug upstream of this.
fn condition(filter: Option<&Filter>) -> Result<Option<(FieldRef, Vec<Value>)>, SourceError> {
    match filter {
        None => Ok(None),
        Some(Filter::Compare {
            field,
            op: CompareOp::Eq,
            value,
        }) => Ok(Some((field.clone(), vec![value.clone()]))),
        Some(Filter::In { field, values }) => Ok(Some((field.clone(), values.clone()))),
        Some(_) => Err(unsupported("BrickLink takes one eq or in condition")),
    }
}

/// `status` for BrickLink: comma-separated, as the API takes it. `None`
/// (fetch every order and filter here) when a value is not a plain status
/// token, since `,` and a leading `-` mean something to BrickLink.
fn status_param(values: &[Value]) -> Option<String> {
    let mut out = Vec::new();
    for value in values {
        let Value::String(status) = value else {
            return None;
        };
        if status.is_empty() || !status.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') {
            return None;
        }
        out.push(status.as_str());
    }
    (!out.is_empty()).then(|| out.join(","))
}

/// Filter, order, cursor, and limit over the whole upstream result, with the
/// lake's equality and order, so a page here means what a lake page means.
fn page(
    records: Vec<LiveRecord>,
    condition: Option<&(FieldRef, Vec<Value>)>,
    query: &LakeQuery,
) -> SourcePage {
    let order = query.effective_order();
    let mut rows: Vec<(Vec<Value>, LiveRecord)> = records
        .into_iter()
        .filter(|record| match condition {
            None => true,
            Some((field, values)) => {
                let stored = field_value(record, field);
                values.iter().any(|value| equals(&stored, value))
            }
        })
        .map(|record| {
            let key = order
                .iter()
                .map(|key| field_value(&record, &key.field))
                .collect();
            (key, record)
        })
        .collect();
    rows.sort_by(|a, b| compare_keys(&order, &a.0, &b.0));
    if let Some(after) = &query.after {
        rows.retain(|(key, _)| compare_keys(&order, key, after) == Ordering::Greater);
    }
    let more = rows.len() > query.limit;
    rows.truncate(query.limit);
    let next = if more {
        rows.last().map(|(key, _)| key.clone())
    } else {
        None
    };
    SourcePage {
        records: rows
            .into_iter()
            .map(|(_, record)| SourceRecord::Live(record))
            .collect(),
        next,
    }
}

/// `$id` is the upstream id. No other system field is declared.
fn field_value(record: &LiveRecord, field: &FieldRef) -> Value {
    match field {
        FieldRef::System(SystemField::Id) => Value::String(record.id.clone()),
        FieldRef::System(_) => Value::Null,
        FieldRef::Field(name) => record.fields.get(name).cloned().unwrap_or(Value::Null),
    }
}

fn unsupported(message: &str) -> SourceError {
    SourceError::Unsupported {
        message: message.to_string(),
    }
}

fn items_need_order() -> SourceError {
    unsupported(
        "order_items needs an order_id filter (eq or in): BrickLink reads items one order at a time",
    )
}

fn read_only() -> SourceError {
    unsupported("BrickLink collections are read-only")
}

fn unknown_collection(name: &str) -> SourceError {
    unsupported(&format!("BrickLink has no collection {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loco_lake::OrderKey;
    use serde_json::json;

    fn fixture(path: &str) -> Json {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bricklink")
            .join(path);
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap()
    }

    fn data(reply: Result<Reply, SourceError>) -> Json {
        match reply {
            Ok(Reply::Data(data)) => data,
            Ok(Reply::NotFound(_)) => panic!("not found"),
            Err(err) => panic!("{err:?}"),
        }
    }

    #[test]
    fn maps_an_order_summary() {
        let body = fixture("orders.json");
        let orders = body["data"].as_array().unwrap();
        let record = order_record(&orders[0]).unwrap();
        assert_eq!(record.id, "29471234");
        assert_eq!(record.fields["status"], Value::String("PENDING".into()));
        assert_eq!(record.fields["buyer"], Value::String("ada_bricks".into()));
        assert_eq!(
            record.fields["date"],
            Value::String("2026-10-03T14:05:11.000Z".into())
        );
        assert_eq!(record.fields["totals"], Value::String("USD 41.2300".into()));
        assert_eq!(record.fields.len(), 4);
    }

    #[test]
    fn maps_batched_items_with_integer_quantities() {
        let body = fixture("orders/29471234/items.json");
        let items = item_records("29471234", &body["data"]);
        assert_eq!(items.len(), 5);
        let first = &items[0];
        assert_eq!(first.id, "29471234-1-358001234");
        assert_eq!(first.fields["item_no"], Value::String("3001".into()));
        assert_eq!(first.fields["item_type"], Value::String("PART".into()));
        assert_eq!(first.fields["color_id"], Value::Integer(11));
        assert_eq!(first.fields["color_name"], Value::String("Black".into()));
        assert_eq!(first.fields["condition"], Value::String("N".into()));
        assert_eq!(first.fields["qty"], Value::Integer(250));
        assert_eq!(first.fields["remarks"], Value::String("A-12-3".into()));
        assert_eq!(
            first.fields["inventory_id"],
            Value::String("358001234".into())
        );
        assert_eq!(first.fields["order_id"], Value::String("29471234".into()));
    }

    #[test]
    fn item_ids_are_unique_and_no_line_is_dropped() {
        let body = fixture("orders/29471234/items.json");
        let items = item_records("29471234", &body["data"]);
        let ids: Vec<_> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "29471234-1-358001234",
                "29471234-1-358001240",
                "29471234-2-358001299",
                // The same lot in a second batch is its own line.
                "29471234-2-358001234",
                // No inventory id: still a line, named by its position.
                "29471234-2-x3",
            ]
        );
        assert_eq!(items[3].fields["qty"], Value::Integer(30));
        assert!(!items[4].fields.contains_key("inventory_id"));
        assert_eq!(items[4].fields["qty"], Value::Integer(8));
        // Stable: the same response gives the same ids.
        assert_eq!(items, item_records("29471234", &body["data"]));

        // The same lot twice in one batch, and a flat list read as batch 1.
        let flat = json!([
            {"inventory_id": 7, "quantity": 1},
            {"inventory_id": 7, "quantity": 2},
            {"quantity": 3}
        ]);
        let ids: Vec<_> = item_records("5", &flat)
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(ids, ["5-1-7", "5-1-7-2", "5-1-x3"]);
    }

    #[test]
    fn a_meta_error_inside_http_200_is_upstream_without_credentials() {
        let body = json!({
            "meta": {
                "code": 401,
                "message": "BAD_OAUTH_REQUEST",
                "description": "consumer key ck-secret-1 is unknown"
            },
            "data": {}
        })
        .to_string();
        match read_envelope(200, Some("OK"), &body, &["ck-secret-1", ""]) {
            Err(SourceError::Upstream { status, message }) => {
                assert_eq!(status, 401);
                assert_eq!(
                    message,
                    "BAD_OAUTH_REQUEST: consumer key [redacted] is unknown"
                );
            }
            _ => panic!("expected upstream"),
        }
    }

    #[test]
    fn not_found_and_success_envelopes() {
        let missing = json!({"meta": {"code": 404, "message": "RESOURCE_NOT_FOUND"}, "data": {}});
        assert!(matches!(
            read_envelope(200, None, &missing.to_string(), &[]),
            Ok(Reply::NotFound(message)) if message == "RESOURCE_NOT_FOUND"
        ));
        // A bare 404 is a wrong base_url or path, not an empty answer.
        match read_envelope(404, Some("Not Found"), "Cannot GET /nope/orders", &[]) {
            Err(SourceError::Upstream { status, message }) => {
                assert_eq!(status, 404);
                assert_eq!(message, "Not Found");
            }
            _ => panic!("expected upstream"),
        }
        let ok = fixture("orders/29471234.json").to_string();
        assert_eq!(
            data(read_envelope(200, None, &ok, &[]))["order_id"],
            json!(29471234)
        );
    }

    #[test]
    fn a_non_json_failure_does_not_echo_the_body() {
        match read_envelope(
            503,
            Some("Service Unavailable"),
            "GET /orders?oauth_token=t",
            &[],
        ) {
            Err(SourceError::Upstream { status, message }) => {
                assert_eq!(status, 503);
                assert_eq!(message, "Service Unavailable");
            }
            _ => panic!("expected upstream"),
        }
    }

    #[test]
    fn status_param_is_passed_only_for_plain_tokens() {
        let s = |text: &str| Value::String(text.into());
        assert_eq!(
            status_param(&[s("PENDING"), s("PAID")]).as_deref(),
            Some("PENDING,PAID")
        );
        assert_eq!(status_param(&[s("-PENDING")]), None);
        assert_eq!(status_param(&[s("PENDING,PAID")]), None);
        assert_eq!(status_param(&[Value::Integer(1)]), None);
    }

    #[test]
    fn page_filters_orders_and_pages_with_the_lake_order() {
        let body = fixture("orders.json");
        let records: Vec<_> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(order_record)
            .collect();
        let mut query = LakeQuery::new("orders", 1);
        query.order = vec![OrderKey::desc(FieldRef::field("date"))];
        let pending = (
            FieldRef::field("status"),
            vec![Value::String("PENDING".into())],
        );
        let first = page(records.clone(), Some(&pending), &query);
        assert_eq!(first.records.len(), 1);
        let next = first.next.clone().expect("a second pending order follows");
        query.after = Some(next);
        let second = page(records, Some(&pending), &query);
        assert_eq!(second.records.len(), 1);
        assert!(second.next.is_none());
        let id = |page: &SourcePage| match &page.records[0] {
            SourceRecord::Live(record) => record.id.clone(),
            SourceRecord::Lake(_) => unreachable!(),
        };
        assert_ne!(id(&first), id(&second));
    }

    #[test]
    fn ids_are_digits_only() {
        assert!(is_upstream_id("29471234"));
        assert!(!is_upstream_id(""));
        assert!(!is_upstream_id("1/../2"));
        assert!(!is_upstream_id("12?x=1"));
    }
}
