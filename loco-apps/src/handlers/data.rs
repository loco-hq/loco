use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use loco_lake::{InsertRequest, Record, UpdatePatch, Value};
use serde_json::{json, Map};

use crate::actions::{ActionFailure, Connection};
use crate::http::response::{
    error_response, error_response_with_diagnostics, lake_error_to_response,
    validation_error_response, ApiResponse,
};
use crate::http::scope::{CollectionScope, RecordScope, SiteScope};
use crate::http::version_schema::{
    ambiguous_address_message, AddressResolution, CollectionAddress, CollectionAddressKind,
    ConnectionDeclarations, VersionSchema,
};
use crate::integrations::SourceRegistry;
use crate::query::{self as q, Plan};
use crate::server::AppState;
use crate::source::{
    default_order, project_live, unsupported_query, unsupported_verb, verb_allowed,
    CollectionSource, SourceCall, SourceError, SourceRecord, Verb,
};
use crate::validation::{validate_inline_records, Diagnostic, ValidationMode};

pub fn router() -> Router<Arc<AppState>> {
    use axum::routing::{delete as route_delete, get as route_get, post, put};
    Router::new()
        .route("/query", post(query))
        .route("/{name}/add", post(add))
        .route("/{name}/list", route_get(list))
        .route("/{name}/fields", route_get(fields))
        .route("/{name}/get/{id}", route_get(get))
        .route("/{name}/update/{id}", put(update))
        .route("/{name}/delete/{id}", route_delete(delete))
}

pub async fn add(
    scope: CollectionScope,
    State(state): State<Arc<AppState>>,
    Json(fields): Json<HashMap<String, Value>>,
) -> Response {
    if let Err(resp) = scope.require_can_create_data() {
        return resp;
    }
    let route = match open(&scope, &state, Verb::Insert) {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    let report = scope.validate(&fields, ValidationMode::Create);
    if report.has_errors() {
        return validation_error_response(report.diagnostics);
    }

    let dataset_id = scope.dataset_id();
    let req = InsertRequest {
        user: scope.user().username.clone(),
        fields,
    };
    let outcome = match route {
        Route::Lake { key } => {
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: None,
            };
            state.lake.insert(call, &key, req).await
        }
        Route::Live {
            source,
            spec,
            collection,
        } => {
            let connection = match connect(&state, &dataset_id, &spec) {
                Ok(connection) => connection,
                Err(fail) => return fail_response(fail),
            };
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: Some(&connection),
            };
            // Installer fields would be split onto the sidecar (#117). The
            // source receives the upstream patch only. That split is not built.
            source.insert(call, &collection, req).await
        }
    };
    match outcome {
        Ok(record) => (StatusCode::CREATED, ApiResponse::success(record)).into_response(),
        Err(err) => source_error_response(err),
    }
}

pub async fn list(scope: CollectionScope, State(state): State<Arc<AppState>>) -> Response {
    if let Err(resp) = scope.require_can_read_data() {
        return resp;
    }
    let route = match open(&scope, &state, Verb::List) {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    let dataset_id = scope.dataset_id();
    let outcome = match route {
        Route::Lake { key } => {
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: None,
            };
            state.lake.list(call, &key).await
        }
        Route::Live {
            source,
            spec,
            collection,
        } => {
            let connection = match connect(&state, &dataset_id, &spec) {
                Ok(connection) => connection,
                Err(fail) => return fail_response(fail),
            };
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: Some(&connection),
            };
            // A cache would sit in front of this call. The sidecar merge
            // (#117) would run after it, keyed by the integration-qualified
            // address and the upstream id. Neither is built.
            source.list(call, &collection).await
        }
    };
    match outcome {
        Ok(records) => {
            let report =
                scope.validate_records(records.iter().map(record_parts), ValidationMode::Read);
            ApiResponse::success_with_diagnostics(records, report.diagnostics).into_response()
        }
        Err(err) => source_error_response(err),
    }
}

/// The collection's fields in the site's pinned version — the same list, order,
/// and shape as the matching schema field list (ordinary fields, the type's
/// fields, or the custom collection's fields), but the version comes from the
/// site, so a hosted frontend never has to be told which version it runs on.
/// Readable by whoever may read the records. This is metadata, not a source
/// call, so an unambiguous integration address returns its fields. An
/// ambiguous address is 409 after that same read check.
pub async fn fields(scope: CollectionScope) -> Response {
    if let Err(resp) = scope.require_can_read_data() {
        return resp;
    }
    if matches!(scope.address.kind, CollectionAddressKind::Ambiguous) {
        return ambiguous_collection(&scope);
    }
    let fields = scope.site.schema.address_fields(&scope.address);
    ApiResponse::success(fields).into_response()
}

pub async fn get(scope: RecordScope, State(state): State<Arc<AppState>>) -> Response {
    if let Err(resp) = scope.collection.require_can_read_data() {
        return resp;
    }
    let route = match open(&scope.collection, &state, Verb::Get) {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    let dataset_id = scope.dataset_id();
    let outcome = match route {
        Route::Lake { key } => {
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: None,
            };
            state.lake.get(call, &key, &scope.id).await
        }
        Route::Live {
            source,
            spec,
            collection,
        } => {
            let connection = match connect(&state, &dataset_id, &spec) {
                Ok(connection) => connection,
                Err(fail) => return fail_response(fail),
            };
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: Some(&connection),
            };
            // A cache would sit in front of this call. The sidecar merge
            // (#117) would run after it, keyed by the integration-qualified
            // address and the upstream id. Neither is built.
            source.get(call, &collection, &scope.id).await
        }
    };
    match outcome {
        Ok(Some(record)) => {
            let report = scope.validate(record_parts(&record).1, ValidationMode::Read);
            ApiResponse::success_with_diagnostics(record, report.diagnostics).into_response()
        }
        Ok(None) => error_response(StatusCode::NOT_FOUND, "record not found"),
        Err(err) => source_error_response(err),
    }
}

pub async fn delete(scope: RecordScope, State(state): State<Arc<AppState>>) -> Response {
    if let Err(resp) = scope.collection.require_can_delete_data() {
        return resp;
    }
    let route = match open(&scope.collection, &state, Verb::Delete) {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    let dataset_id = scope.dataset_id();
    let outcome = match route {
        Route::Lake { key } => {
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: None,
            };
            state.lake.delete(call, &key, &scope.id).await
        }
        Route::Live {
            source,
            spec,
            collection,
        } => {
            let connection = match connect(&state, &dataset_id, &spec) {
                Ok(connection) => connection,
                Err(fail) => return fail_response(fail),
            };
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: Some(&connection),
            };
            source.delete(call, &collection, &scope.id).await
        }
    };
    match outcome {
        Ok(()) => ApiResponse::success("deleted").into_response(),
        Err(err) => source_error_response(err),
    }
}

pub async fn update(
    scope: RecordScope,
    State(state): State<Arc<AppState>>,
    Json(fields): Json<HashMap<String, Value>>,
) -> Response {
    if let Err(resp) = scope.collection.require_can_update_data() {
        return resp;
    }
    let route = match open(&scope.collection, &state, Verb::Update) {
        Ok(route) => route,
        Err(resp) => return resp,
    };
    let report = scope.validate(&fields, ValidationMode::Update);
    if report.has_errors() {
        return validation_error_response(report.diagnostics);
    }

    let dataset_id = scope.dataset_id();
    let patch = UpdatePatch {
        user: scope.user().username.clone(),
        fields,
    };
    let outcome = match route {
        Route::Lake { key } => {
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: None,
            };
            state.lake.update(call, &key, &scope.id, patch).await
        }
        Route::Live {
            source,
            spec,
            collection,
        } => {
            let connection = match connect(&state, &dataset_id, &spec) {
                Ok(connection) => connection,
                Err(fail) => return fail_response(fail),
            };
            let call = SourceCall {
                dataset_id: &dataset_id,
                connection: Some(&connection),
            };
            // Installer fields would be split onto the sidecar (#117). The
            // source receives the upstream patch only. That split is not built.
            source.update(call, &collection, &scope.id, patch).await
        }
    };
    match outcome {
        Ok(record) => ApiResponse::success(record).into_response(),
        Err(err) => source_error_response(err),
    }
}

/// `POST /data/query` — a batch of named read queries (`docs/query.md`).
///
/// Each query is resolved, type-checked, and authorized on its own; one that
/// fails gets an error result and the rest still run. Lake queries share one
/// adapter snapshot. An integration query runs on that type's source and does
/// not share the snapshot. One integration failure stays inside that query's
/// result. A failure of that snapshot fails the request. A lake error while
/// reading an integration's connection values is a per-query `failed`.
/// The status is 400 only when the body is not a batch at all.
pub async fn query(
    scope: SiteScope,
    State(state): State<Arc<AppState>>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.body_text()),
    };
    let queries = match q::parse_batch(&body) {
        Ok(queries) => queries,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };

    let dataset_id = scope.dataset_id();
    let mut slots: Vec<(String, Slot)> = Vec::new();
    for (name, raw) in queries {
        match plan_one(&scope, &state, &dataset_id, &name, &raw) {
            Ok(Ok(ready)) => slots.push((name, Slot::Ready(Box::new(ready)))),
            Ok(Err(diagnostics)) => slots.push((name, Slot::Failed(diagnostics))),
            Err(resp) => return resp,
        }
    }

    let lake_at: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter(|(_, (_, slot))| {
            matches!(slot, Slot::Ready(ready) if matches!(ready.route, QueryRoute::Lake))
        })
        .map(|(index, _)| index)
        .collect();
    if !lake_at.is_empty() {
        let lake_queries: Vec<_> = lake_at
            .iter()
            .map(|index| match &slots[*index].1 {
                Slot::Ready(ready) => ready.plan.lake.clone(),
                _ => unreachable!("lake index points at a ready lake query"),
            })
            .collect();
        let call = SourceCall {
            dataset_id: &dataset_id,
            connection: None,
        };
        let pages = match state.lake.query(call, &lake_queries).await {
            Ok(pages) if pages.len() == lake_queries.len() => pages,
            Ok(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "source returned the wrong number of pages",
                );
            }
            // A failure of this snapshot fails the request and drops every
            // result, the same as the adapter error did before sources. A
            // lake error while reading connection values stays in `connect`.
            Err(SourceError::Lake(err)) => return lake_error_to_response(err),
            Err(err) => return source_error_response(err),
        };
        for (index, page) in lake_at.into_iter().zip(pages) {
            let name = slots[index].0.clone();
            let Slot::Ready(ready) =
                std::mem::replace(&mut slots[index].1, Slot::Failed(Vec::new()))
            else {
                continue;
            };
            let records = lake_records(&page.records);
            let diagnostics = q::record_diagnostics(
                &scope.schema,
                &name,
                &ready.plan.target.project,
                &ready.plan.target.name,
                &records,
                ready.plan.lake.fields.as_deref(),
            );
            let cursor = page
                .next
                .as_ref()
                .map(|next| q::encode_cursor(&ready.plan.hash, next));
            slots[index].1 = Slot::Done {
                value: json!({ "records": page.records, "cursor": cursor }),
                diagnostics,
            };
        }
    }

    for (name, slot) in &mut slots {
        let is_live = matches!(
            slot,
            Slot::Ready(ready) if matches!(ready.route, QueryRoute::Live { .. })
        );
        if !is_live {
            continue;
        }
        let Slot::Ready(ready) = std::mem::replace(slot, Slot::Failed(Vec::new())) else {
            continue;
        };
        *slot = run_live(&scope.schema, &dataset_id, name, *ready).await;
    }

    // Errors first, then successes, each group in the batch's original order.
    let mut results = Map::new();
    let mut diagnostics = Vec::new();
    for (name, slot) in &slots {
        if let Slot::Failed(failed) = slot {
            results.insert(
                name.clone(),
                json!({ "error": "query failed", "diagnostics": failed }),
            );
        }
    }
    for (name, slot) in &slots {
        if let Slot::Done {
            value,
            diagnostics: found,
        } = slot
        {
            diagnostics.extend(found.clone());
            results.insert(name.clone(), value.clone());
        }
    }

    ApiResponse::success_with_diagnostics(results, diagnostics).into_response()
}

enum Route {
    Lake {
        key: String,
    },
    Live {
        source: Arc<dyn CollectionSource>,
        spec: ConnectionDeclarations,
        collection: String,
    },
}

enum QueryRoute {
    Lake,
    Live {
        source: Arc<dyn CollectionSource>,
        connection: Connection,
    },
}

struct Ready {
    plan: Plan,
    route: QueryRoute,
}

enum Slot {
    Failed(Vec<Diagnostic>),
    Ready(Box<Ready>),
    Done {
        value: serde_json::Value,
        diagnostics: Vec<Diagnostic>,
    },
}

enum ConnectFail {
    Missing(Vec<Diagnostic>),
    Unavailable(String),
    Failed(String),
}

/// After auth: ambiguous is 409, no registration is 501, an undeclared verb
/// is 400. This does not read connection values and does not validate the body.
fn open(scope: &CollectionScope, state: &AppState, verb: Verb) -> Result<Route, Response> {
    if matches!(scope.address.kind, CollectionAddressKind::Ambiguous) {
        return Err(ambiguous_collection(scope));
    }
    let route = route_of(scope, state)?;
    let caps = match &route {
        Route::Lake { .. } => state.lake.capabilities(),
        Route::Live { source, .. } => source.capabilities(),
    };
    if !verb_allowed(&caps, verb) {
        return Err(unsupported_response(&scope.canonical(), verb));
    }
    Ok(route)
}

fn route_of(scope: &CollectionScope, state: &AppState) -> Result<Route, Response> {
    match scope.address.kind {
        CollectionAddressKind::Ambiguous => Err(ambiguous_collection(scope)),
        CollectionAddressKind::Ordinary => {
            let key = scope.lake_key().ok_or_else(|| no_source(scope))?;
            Ok(Route::Lake { key })
        }
        CollectionAddressKind::Standard { .. } | CollectionAddressKind::Custom => {
            match source_for(&scope.site.schema, &state.sources, &scope.address) {
                Some((source, spec)) => Ok(Route::Live {
                    source,
                    spec,
                    collection: scope.address.name.clone(),
                }),
                None => Err(no_source(scope)),
            }
        }
    }
}

/// The type's source, plus the declarations the required-value check uses.
/// A standard collection's registry key is on the address. A custom
/// collection's type is the integration's `type_ref`, which may name a
/// package the caller does not depend on.
fn source_for(
    schema: &VersionSchema,
    sources: &SourceRegistry,
    address: &CollectionAddress,
) -> Option<(Arc<dyn CollectionSource>, ConnectionDeclarations)> {
    let integration = address.integration.as_deref()?;
    let spec = schema.connection_declarations(&address.project, integration)?;
    let source = match &address.kind {
        CollectionAddressKind::Standard {
            type_project,
            type_name,
            ..
        } => sources.get(type_project, type_name)?,
        CollectionAddressKind::Custom => {
            let (project, name) = schema.split(&spec.type_ref);
            sources.get(project, name)?
        }
        CollectionAddressKind::Ordinary | CollectionAddressKind::Ambiguous => return None,
    };
    Some((source, spec))
}

fn connect(
    state: &AppState,
    dataset_id: &str,
    spec: &ConnectionDeclarations,
) -> Result<Connection, ConnectFail> {
    let connection = Connection::new(
        dataset_id.to_string(),
        spec.clone(),
        state.secrets.clone(),
        state.data_adapter.clone(),
        state.http.clone(),
    );
    match connection.missing_required() {
        Ok(missing) if missing.is_empty() => Ok(connection),
        Ok(missing) => Err(ConnectFail::Missing(missing)),
        Err(ActionFailure::Unavailable { message }) => Err(ConnectFail::Unavailable(message)),
        Err(failure) => Err(ConnectFail::Failed(failure.message().to_string())),
    }
}

fn fail_response(fail: ConnectFail) -> Response {
    match fail {
        ConnectFail::Missing(diagnostics) => error_response_with_diagnostics(
            StatusCode::BAD_REQUEST,
            "required configuration is not set",
            diagnostics,
        ),
        ConnectFail::Unavailable(message) => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, &message)
        }
        ConnectFail::Failed(message) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &message),
    }
}

fn connect_query_diags(name: &str, fail: ConnectFail) -> Vec<Diagnostic> {
    match fail {
        ConnectFail::Missing(diagnostics) => diagnostics,
        ConnectFail::Unavailable(message) => {
            vec![Diagnostic::error(
                q::kind::UNAVAILABLE,
                Some(name.to_string()),
                message,
            )]
        }
        ConnectFail::Failed(message) => vec![Diagnostic::error(
            q::kind::FAILED,
            Some(name.to_string()),
            message,
        )],
    }
}

/// One query → a plan, or the diagnostics that stop it. The outer `Err` is a
/// site-level failure (the membership lookup), which fails the whole request.
///
/// An integration query is planned before the capability check, so a bad
/// field is `invalid_query` rather than a missing secret. The capability
/// check comes before the required-value check. No registration is
/// `no_source` and does not ask for configuration.
fn plan_one(
    scope: &SiteScope,
    state: &AppState,
    dataset_id: &str,
    name: &str,
    raw: &serde_json::Value,
) -> Result<Result<Ready, Vec<Diagnostic>>, Response> {
    let target = match q::target(&scope.schema, name, raw) {
        Ok(target) => target,
        Err(diagnostic) => return Ok(Err(vec![diagnostic])),
    };
    if !scope.may_read_collection(&target.local, &target.address_project)? {
        let named = if target.integration.is_some() {
            scope
                .schema
                .reference(&target.address_project, &target.local)
        } else {
            target.key()
        };
        return Ok(Err(vec![q::Diagnostic::error(
            q::kind::FORBIDDEN,
            Some(name.to_string()),
            format!("no read grant on {named}"),
        )]));
    }
    if target.ambiguous {
        let address = scope
            .schema
            .reference(&target.address_project, &target.local);
        return Ok(Err(vec![q::Diagnostic::error(
            q::kind::AMBIGUOUS_ADDRESS,
            Some(name.to_string()),
            ambiguous_address_message(&address),
        )]));
    }
    if target.integration.is_some() {
        let address_name = scope
            .schema
            .reference(&target.address_project, &target.local);
        let Some(address) = resolve_address(&scope.schema, &address_name) else {
            return Ok(Err(vec![no_source_diag(name, &address_name)]));
        };
        let Some((source, spec)) = source_for(&scope.schema, &state.sources, &address) else {
            return Ok(Err(vec![no_source_diag(name, &address_name)]));
        };
        let caps = source.capabilities();
        let plan = match q::plan(&scope.schema, name, raw, target, &default_order(&caps)) {
            Ok(plan) => plan,
            Err(diagnostics) => return Ok(Err(diagnostics)),
        };
        let problems = unsupported_query(name, &address_name, &caps, &plan.lake);
        if !problems.is_empty() {
            return Ok(Err(problems));
        }
        let connection = match connect(state, dataset_id, &spec) {
            Ok(connection) => connection,
            Err(fail) => return Ok(Err(connect_query_diags(name, fail))),
        };
        return Ok(Ok(Ready {
            plan,
            route: QueryRoute::Live { source, connection },
        }));
    }

    let caps = state.lake.capabilities();
    let plan = match q::plan(&scope.schema, name, raw, target, &default_order(&caps)) {
        Ok(plan) => plan,
        Err(diagnostics) => return Ok(Err(diagnostics)),
    };
    let problems = unsupported_query(name, &plan.target.key(), &caps, &plan.lake);
    if !problems.is_empty() {
        return Ok(Err(problems));
    }
    Ok(Ok(Ready {
        plan,
        route: QueryRoute::Lake,
    }))
}

async fn run_live(schema: &VersionSchema, dataset_id: &str, name: &str, ready: Ready) -> Slot {
    let QueryRoute::Live { source, connection } = ready.route else {
        return Slot::Failed(vec![Diagnostic::error(
            q::kind::FAILED,
            Some(name.to_string()),
            "internal error: integration query was not live".into(),
        )]);
    };
    let call = SourceCall {
        dataset_id,
        connection: Some(&connection),
    };
    let mut lake = ready.plan.lake.clone();
    lake.collection = ready.plan.target.name.clone();
    // A cache would sit in front of this call. The sidecar merge (#117)
    // would run after it, keyed by the integration-qualified address and the
    // upstream id. Neither is built.
    let pages = match source.query(call, &[lake]).await {
        Ok(pages) => pages,
        Err(err) => return Slot::Failed(source_query_diags(name, err)),
    };
    let Some(mut page) = one_page(pages) else {
        return Slot::Failed(vec![Diagnostic::error(
            q::kind::FAILED,
            Some(name.to_string()),
            "source returned the wrong number of pages".into(),
        )]);
    };
    for record in &mut page.records {
        if let SourceRecord::Live(live) = record {
            project_live(live, ready.plan.lake.fields.as_deref());
        }
    }
    let (canonical, specs) = inline_specs(schema, &ready.plan);
    let diagnostics = validate_inline_records(
        &specs,
        &canonical,
        schema.version(),
        page.records.iter().map(record_parts),
        ValidationMode::Read,
        ready.plan.lake.fields.as_deref(),
    )
    .prefix_paths(name)
    .diagnostics;
    let cursor = page
        .next
        .as_ref()
        .map(|next| q::encode_cursor(&ready.plan.hash, next));
    Slot::Done {
        value: json!({ "records": page.records, "cursor": cursor }),
        diagnostics,
    }
}

fn one_page(mut pages: Vec<crate::source::SourcePage>) -> Option<crate::source::SourcePage> {
    if pages.len() == 1 {
        pages.pop()
    } else {
        None
    }
}

fn resolve_address(schema: &VersionSchema, name: &str) -> Option<CollectionAddress> {
    match schema.collection_address(name) {
        AddressResolution::Resolved(address) => Some(address),
        AddressResolution::Missing => None,
    }
}

fn inline_specs(
    schema: &VersionSchema,
    plan: &Plan,
) -> (String, Vec<crate::http::version_schema::InlineField>) {
    let canonical = schema.reference(&plan.target.address_project, &plan.target.local);
    let specs = match schema.collection_address(&canonical) {
        AddressResolution::Resolved(address) => schema.integration_fields(&address),
        AddressResolution::Missing => Vec::new(),
    };
    (canonical, specs)
}

fn record_parts(record: &SourceRecord) -> (&str, &HashMap<String, Value>) {
    match record {
        SourceRecord::Lake(record) => (record.id.as_str(), &record.fields),
        SourceRecord::Live(record) => (record.id.as_str(), &record.fields),
    }
}

fn lake_records(records: &[SourceRecord]) -> Vec<Record> {
    records
        .iter()
        .filter_map(|record| match record {
            SourceRecord::Lake(record) => Some(record.clone()),
            SourceRecord::Live(_) => None,
        })
        .collect()
}

fn source_query_diags(name: &str, err: SourceError) -> Vec<Diagnostic> {
    let (kind, message) = match err {
        SourceError::Upstream { status, message } => {
            (q::kind::UPSTREAM, format!("upstream {status}: {message}"))
        }
        SourceError::Unavailable { message } => (q::kind::UNAVAILABLE, message),
        SourceError::Unsupported { message } => (q::kind::UNSUPPORTED, message),
        SourceError::Failed { message } => (q::kind::FAILED, message),
        SourceError::Lake(err) => (q::kind::FAILED, err.to_string()),
    };
    vec![Diagnostic::error(kind, Some(name.to_string()), message)]
}

fn source_error_response(err: SourceError) -> Response {
    match err {
        SourceError::Lake(err) => lake_error_to_response(err),
        SourceError::Upstream { status, message } => error_response(
            StatusCode::BAD_GATEWAY,
            &format!("upstream {status}: {message}"),
        ),
        SourceError::Unavailable { message } => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, &message)
        }
        SourceError::Unsupported { message } => {
            unsupported_diagnostic_response(Diagnostic::error(q::kind::UNSUPPORTED, None, message))
        }
        SourceError::Failed { message } => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &message)
        }
    }
}

fn unsupported_response(address: &str, verb: Verb) -> Response {
    unsupported_diagnostic_response(unsupported_verb(address, verb))
}

fn unsupported_diagnostic_response(diagnostic: Diagnostic) -> Response {
    error_response_with_diagnostics(StatusCode::BAD_REQUEST, "unsupported", vec![diagnostic])
}

fn no_source_diag(name: &str, address: &str) -> Diagnostic {
    Diagnostic::error(
        q::kind::NO_SOURCE,
        Some(name.to_string()),
        format!("no source for collection {address}"),
    )
}

/// 409 after auth. The address resolved to `project` and `local`, and both
/// documents share the name, so neither is read.
fn ambiguous_collection(scope: &CollectionScope) -> Response {
    error_response(
        StatusCode::CONFLICT,
        &ambiguous_address_message(&scope.canonical()),
    )
}

/// 501 after auth. The address resolved, so this is not 404. The request is
/// well formed, so this is not 400. The process is not missing a key, so
/// this is not 503. A lake read would look like an empty collection. A
/// declared action with no handler is the same status.
fn no_source(scope: &CollectionScope) -> Response {
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        &format!("no source for collection {}", scope.canonical()),
    )
}
