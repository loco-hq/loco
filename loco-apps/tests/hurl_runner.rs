use std::path::Path;
use std::process::Command;
use std::sync::Once;

use loco_apps::server::AppOptions;

// `std::env::set_var` is not thread-safe; guard it so parallel suites set it exactly once.
static ADAPTER_ENV_ONCE: Once = Once::new();

/// Copy a directory tree recursively.
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

/// Returns the path to the `tests/suites/` directory.
fn suites_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/suites")
}

/// Returns the crate root (loco-apps/).
fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Run all `.hurl` files in a suite directory against a server backed by that
/// suite's `fixtures/` folder.
///
/// The server root is assembled in a tempdir:
/// - `schemas/types/` always comes from the real crate (core type definitions)
/// - `schemas/instances/`, `auth/` come from the suite's `fixtures/` folder
///   if present, otherwise empty dirs are created
fn run_suite(suite_dir: &Path) -> tempfile::TempDir {
    run_suite_with(suite_dir, AppOptions::default())
}

/// As `run_suite`, with app options pinned instead of read from the
/// environment. Returns the server root so a caller can assert on what the
/// suite did (or did not) write to disk.
fn run_suite_with(suite_dir: &Path, options: AppOptions) -> tempfile::TempDir {
    run_suite_in(suite_dir, options, &[], &[])
}

/// As `run_suite`, with the named accounts' committed seed trees
/// (`schemas/seed/{account}/`) copied into the root's `schemas/seed/`, so the
/// server seeds its store from them on boot exactly as it does for real.
fn run_suite_over_seed(suite_dir: &Path, accounts: &[&str]) -> tempfile::TempDir {
    run_suite_in(suite_dir, AppOptions::default(), accounts, &[])
}

fn run_suite_in(
    suite_dir: &Path,
    options: AppOptions,
    seed_accounts: &[&str],
    variables: &[(&str, &str)],
) -> tempfile::TempDir {
    // 1. Build server root in a tempdir
    let tmp = tempfile::TempDir::new().unwrap();

    // Always use the real type definitions
    let types_dst = tmp.path().join("schemas/types");
    copy_dir_all(&crate_dir().join("schemas/types"), &types_dst);

    // Copy suite-specific fixtures (instances, config, auth) if provided
    let fixtures_src = suite_dir.join("fixtures");
    for subdir in ["schemas/instances", "auth"] {
        let src = fixtures_src.join(subdir);
        let dst = tmp.path().join(subdir);
        if src.exists() {
            copy_dir_all(&src, &dst);
        } else {
            std::fs::create_dir_all(&dst).ok();
        }
    }
    for account in seed_accounts {
        copy_dir_all(
            &crate_dir().join("schemas/seed").join(account),
            &tmp.path().join("schemas/seed").join(account),
        );
    }

    // 2. Use in-memory adapter (no SQLite needed for tests). Set once across all suites.
    ADAPTER_ENV_ONCE.call_once(|| unsafe {
        std::env::set_var("LOCO_ADAPTER", "memory");
        std::env::set_var("LOCO_AUTH_AUTO_CREATE", "1");
        // Tests only: standard base64 of 32 zero bytes. Secret writes are
        // 503 without LOCO_SECRET_KEY. Unit tests parse keys themselves and
        // do not read this variable.
        std::env::set_var(
            "LOCO_SECRET_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        );
    });

    // 3. Build the app rooted at the tempdir
    let app = loco_apps::server::build_app_with_options(tmp.path(), options);

    // 4. Start server on a random available port
    let rt = tokio::runtime::Runtime::new().unwrap();
    let port = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    });

    // 5. Collect .hurl files from the suite directory
    let mut hurl_files: Vec<_> = std::fs::read_dir(suite_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "hurl").unwrap_or(false))
        .collect();
    hurl_files.sort();

    assert!(
        !hurl_files.is_empty(),
        "no .hurl files found in {}",
        suite_dir.display()
    );

    // 6. Run hurl
    // `--jobs 1`: hurl 5+ parallelizes by default in test mode, but our suites
    // share one in-memory server, so parallel files race on schema state.
    let mut command = Command::new("hurl");
    command
        .arg("--test")
        .arg("--jobs")
        .arg("1")
        .arg("--variable")
        .arg(format!("port={port}"));
    for (key, value) in variables {
        command.arg("--variable").arg(format!("{key}={value}"));
    }
    let output = command
        .args(&hurl_files)
        .output()
        .expect("failed to run hurl — is it installed? (brew install hurl)");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.is_empty() {
        println!("{stdout}");
    }
    if !stderr.is_empty() {
        eprintln!("{stderr}");
    }

    assert!(
        output.status.success(),
        "hurl tests failed in {}",
        suite_dir.display()
    );

    tmp
}

#[test]
fn suite_schema_crud() {
    run_suite(&suites_dir().join("schema_crud"));
}

#[test]
fn suite_schema_declarations() {
    run_suite(&suites_dir().join("schema_declarations"));
}

#[test]
fn suite_integrations() {
    run_suite(&suites_dir().join("integrations"));
}

#[test]
fn suite_integration_addresses() {
    run_suite(&suites_dir().join("integration_addresses"));
}

#[test]
fn suite_actions() {
    // Fixture handlers are registered here, not in the library and not in
    // the server binary. `build_app` keeps an empty registry. `pull`'s
    // upstream URL arrives as a variable the store sets; the default in the
    // declaration points nowhere.
    let upstream_port = start_upstream();
    let upstream = format!("http://127.0.0.1:{upstream_port}");
    let mut actions = loco_apps::actions::HandlerRegistry::default();
    actions.register("alice/fixture", "echo", |ctx| async move {
        Ok(serde_json::json!({
            "ran": true,
            "dataset_id": ctx.dataset_id,
            "caller": ctx.caller.username,
            "input": ctx.input,
        }))
    });
    actions.register("alice/pkg", "pull", |ctx| async move { pull(ctx).await });
    run_suite_in(
        &suites_dir().join("actions"),
        AppOptions {
            actions,
            ..AppOptions::default()
        },
        &[],
        &[("upstream", upstream.as_str())],
    );
}

#[test]
fn suite_connection_values() {
    // Registered here, not in the server binary. The addresses suite keeps
    // an empty type-action registry, so `sf_east:set_owner` stays 501.
    let mut type_actions = loco_apps::integrations::TypeActionRegistry::default();
    type_actions.register("alice/pkg", "warehouse", "read", |ctx| async move {
        read_connection(ctx).await
    });
    run_suite_with(
        &suites_dir().join("connection_values"),
        AppOptions {
            type_actions,
            ..AppOptions::default()
        },
    );
}

#[test]
fn suite_collection_source() {
    // Registered here, not in the server binary. The addresses suite keeps
    // an empty source registry, so an integration collection there stays 501.
    let east = start_record_upstream(&[("e1", "open"), ("e2", "closed")], &[]);
    let west = start_record_upstream(&[("w1", "open")], &[]);
    let hub = start_record_upstream(&[("h1", "open"), ("h2", "closed")], &[("n1", "hi")]);
    let mut sources = loco_apps::integrations::SourceRegistry::default();
    sources.register("alice/pkg", "warehouse", WarehouseSource);
    run_suite_in(
        &suites_dir().join("collection_source"),
        AppOptions {
            sources,
            ..AppOptions::default()
        },
        &[],
        &[
            ("east", east.as_str()),
            ("west", west.as_str()),
            ("hub", hub.as_str()),
        ],
    );
}

async fn read_connection(
    ctx: loco_apps::integrations::TypeActionContext,
) -> Result<serde_json::Value, loco_apps::actions::ActionFailure> {
    let integration = ctx.connection.integration.clone();
    let loose = match ctx.secret("license") {
        Ok(value) => serde_json::json!(value),
        Err(err) => serde_json::json!({ "error": err.to_string() }),
    };
    Ok(serde_json::json!({
        "integration": integration,
        "token": ctx.secret("token")?,
        "consumer_key": ctx.secret("consumer_key")?,
        "region": ctx.variable("region")?,
        "loose": loose,
    }))
}

/// `GET /ok` is 200 `upstream ok`. `GET /fail` is 503 `upstream is down`.
fn start_upstream() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = answer_upstream(stream);
        }
    });
    port
}

fn answer_upstream(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 8192 {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err),
        }
    }
    let req = String::from_utf8_lossy(&buf);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let (status, reason, body) = match path {
        "/ok" => (200, "OK", "upstream ok"),
        "/fail" => (503, "Service Unavailable", "upstream is down"),
        _ => (404, "Not Found", "no"),
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

async fn pull(
    ctx: loco_apps::actions::ActionContext,
) -> Result<serde_json::Value, loco_apps::actions::ActionFailure> {
    use loco_apps::actions::{ActionFailure, ConfigReadError};
    use loco_lake::Value;

    fn read_config(
        result: Result<Option<String>, ConfigReadError>,
    ) -> Result<serde_json::Value, ActionFailure> {
        match result {
            Ok(value) => Ok(serde_json::json!(value)),
            Err(ConfigReadError::Undeclared { kind, name, .. }) => Ok(serde_json::json!({
                "error": format!("{kind} '{name}' is not declared by this package")
            })),
            Err(err) => Err(err.into()),
        }
    }

    let mut body = serde_json::json!({
        "ran": true,
        "dataset_id": ctx.dataset_id,
        "token": read_config(ctx.secret("token"))?,
        "store_only": read_config(ctx.secret("store_only"))?,
        "upstream": read_config(ctx.variable("upstream"))?,
        "label": read_config(ctx.variable("label"))?,
        "region": read_config(ctx.variable("region"))?,
    });
    let call = matches!(ctx.input.get("call"), Some(Value::Boolean(true)));
    if call {
        let base = ctx
            .variable("upstream")?
            .ok_or_else(|| ActionFailure::BadInput {
                message: "variable 'upstream' is not set".into(),
                diagnostics: Vec::new(),
            })?;
        let path = match ctx.input.get("path") {
            Some(Value::String(path)) => path.clone(),
            _ => "/ok".to_string(),
        };
        let response = ctx.http().get(format!("{base}{path}")).send().await?;
        let status = response.status().as_u16();
        let message = response.text().await.unwrap_or_default().trim().to_string();
        // This fixture's failure body is a fixed string. A handler must not
        // forward an upstream body that echoes the request.
        if !(200..300).contains(&status) {
            return Err(ActionFailure::Upstream { status, message });
        }
        body["http_body"] = serde_json::json!(message);
    }
    Ok(body)
}

/// Fixture source for `alice/pkg`'s `warehouse` type. get, list, and query
/// only. `$id` is the only system field, so `$created_at` and `gt` are
/// capability errors. Two integrations reach different upstreams because
/// each call reads that integration's `base_url` and `token`.
struct WarehouseSource;

#[loco_apps::source::async_trait]
impl loco_apps::source::CollectionSource for WarehouseSource {
    fn capabilities(&self) -> loco_apps::source::Capabilities {
        loco_apps::source::Capabilities {
            get: true,
            list: true,
            query: true,
            eq: true,
            asc: true,
            binary: true,
            limit: true,
            cursor: true,
            system: vec![loco_lake::SystemField::Id],
            ..loco_apps::source::Capabilities::none()
        }
    }

    async fn get(
        &self,
        call: loco_apps::source::SourceCall<'_>,
        collection: &str,
        id: &str,
    ) -> Result<Option<loco_apps::source::SourceRecord>, loco_apps::source::SourceError> {
        let response = http_get(&call, &format!("/{collection}/{id}")).await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        let body = success_body(response).await?;
        Ok(Some(loco_apps::source::SourceRecord::Live(parse_one(
            &body,
        )?)))
    }

    async fn list(
        &self,
        call: loco_apps::source::SourceCall<'_>,
        collection: &str,
    ) -> Result<Vec<loco_apps::source::SourceRecord>, loco_apps::source::SourceError> {
        let response = http_get(&call, &format!("/{collection}")).await?;
        let body = success_body(response).await?;
        Ok(parse_list(&body)?
            .into_iter()
            .map(loco_apps::source::SourceRecord::Live)
            .collect())
    }

    async fn insert(
        &self,
        _: loco_apps::source::SourceCall<'_>,
        _: &str,
        _: loco_lake::InsertRequest,
    ) -> Result<loco_apps::source::SourceRecord, loco_apps::source::SourceError> {
        Err(loco_apps::source::SourceError::Failed {
            message: "warehouse does not insert".into(),
        })
    }

    async fn update(
        &self,
        _: loco_apps::source::SourceCall<'_>,
        _: &str,
        _: &str,
        _: loco_lake::UpdatePatch,
    ) -> Result<loco_apps::source::SourceRecord, loco_apps::source::SourceError> {
        Err(loco_apps::source::SourceError::Failed {
            message: "warehouse does not update".into(),
        })
    }

    async fn delete(
        &self,
        _: loco_apps::source::SourceCall<'_>,
        _: &str,
        _: &str,
    ) -> Result<(), loco_apps::source::SourceError> {
        Err(loco_apps::source::SourceError::Failed {
            message: "warehouse does not delete".into(),
        })
    }

    async fn query(
        &self,
        call: loco_apps::source::SourceCall<'_>,
        queries: &[loco_lake::LakeQuery],
    ) -> Result<Vec<loco_apps::source::SourcePage>, loco_apps::source::SourceError> {
        let mut pages = Vec::new();
        for query in queries {
            let response = http_get(&call, &format!("/{}", query.collection)).await?;
            let body = success_body(response).await?;
            pages.push(apply_query(parse_list(&body)?, query)?);
        }
        Ok(pages)
    }
}

async fn http_get(
    call: &loco_apps::source::SourceCall<'_>,
    path: &str,
) -> Result<reqwest::Response, loco_apps::source::SourceError> {
    let connection = call
        .connection
        .ok_or_else(|| loco_apps::source::SourceError::Failed {
            message: "a collection source requires a connection".into(),
        })?;
    let base = connection.variable("base_url")?.unwrap_or_default();
    let token = connection.secret("token")?.unwrap_or_default();
    let base = base.trim_end_matches('/');
    Ok(connection
        .http()
        .get(format!("{base}{path}"))
        .header("X-Token", token)
        .send()
        .await?)
}

async fn success_body(
    response: reqwest::Response,
) -> Result<String, loco_apps::source::SourceError> {
    let status = response.status().as_u16();
    let message = response.text().await?.trim().to_string();
    if !(200..300).contains(&status) {
        return Err(loco_apps::source::SourceError::Upstream { status, message });
    }
    Ok(message)
}

fn parse_one(body: &str) -> Result<loco_apps::source::LiveRecord, loco_apps::source::SourceError> {
    serde_json::from_str(body).map_err(|_| loco_apps::source::SourceError::Failed {
        message: "upstream record was not JSON".into(),
    })
}

fn parse_list(
    body: &str,
) -> Result<Vec<loco_apps::source::LiveRecord>, loco_apps::source::SourceError> {
    serde_json::from_str(body).map_err(|_| loco_apps::source::SourceError::Failed {
        message: "upstream record was not JSON".into(),
    })
}

fn apply_query(
    records: Vec<loco_apps::source::LiveRecord>,
    query: &loco_lake::LakeQuery,
) -> Result<loco_apps::source::SourcePage, loco_apps::source::SourceError> {
    use std::cmp::Ordering;

    let mut rows = Vec::new();
    for record in records {
        if let Some(filter) = &query.filter {
            if !matches_filter(&record, filter)? {
                continue;
            }
        }
        rows.push(record);
    }
    let order = query.effective_order();
    if order
        .iter()
        .any(|key| key.dir == loco_lake::Direction::Desc)
    {
        return Err(loco_apps::source::SourceError::Failed {
            message: "unexpected direction".into(),
        });
    }
    rows.sort_by(|left, right| cmp_records(left, right, &order));
    if let Some(after) = &query.after {
        rows.retain(|record| after_cmp(record, after, &order) == Ordering::Greater);
    }
    let more = rows.len() > query.limit;
    rows.truncate(query.limit);
    let next = if more {
        rows.last().map(|record| {
            order
                .iter()
                .map(|key| field_value(record, &key.field))
                .collect()
        })
    } else {
        None
    };
    Ok(loco_apps::source::SourcePage {
        records: rows
            .into_iter()
            .map(loco_apps::source::SourceRecord::Live)
            .collect(),
        next,
    })
}

fn matches_filter(
    record: &loco_apps::source::LiveRecord,
    filter: &loco_lake::Filter,
) -> Result<bool, loco_apps::source::SourceError> {
    match filter {
        loco_lake::Filter::Compare { field, op, value } => {
            if !matches!(op, loco_lake::CompareOp::Eq) {
                return Err(loco_apps::source::SourceError::Failed {
                    message: "unexpected op".into(),
                });
            }
            Ok(values_eq(&field_value(record, field), value))
        }
        loco_lake::Filter::And(filters) => {
            for filter in filters {
                if !matches_filter(record, filter)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Err(loco_apps::source::SourceError::Failed {
            message: "unexpected op".into(),
        }),
    }
}

fn field_value(
    record: &loco_apps::source::LiveRecord,
    field: &loco_lake::FieldRef,
) -> loco_lake::Value {
    match field {
        loco_lake::FieldRef::System(loco_lake::SystemField::Id) => {
            loco_lake::Value::String(record.id.clone())
        }
        loco_lake::FieldRef::System(_) => loco_lake::Value::Null,
        loco_lake::FieldRef::Field(name) => record
            .fields
            .get(name)
            .cloned()
            .unwrap_or(loco_lake::Value::Null),
    }
}

fn values_eq(left: &loco_lake::Value, right: &loco_lake::Value) -> bool {
    match (left, right) {
        (loco_lake::Value::Null, loco_lake::Value::Null) => true,
        (loco_lake::Value::String(left), loco_lake::Value::String(right)) => left == right,
        (loco_lake::Value::Integer(left), loco_lake::Value::Integer(right)) => left == right,
        (loco_lake::Value::Boolean(left), loco_lake::Value::Boolean(right)) => left == right,
        (loco_lake::Value::Float(left), loco_lake::Value::Float(right)) => left == right,
        (loco_lake::Value::Integer(left), loco_lake::Value::Float(right)) => *left as f64 == *right,
        (loco_lake::Value::Float(left), loco_lake::Value::Integer(right)) => *left == *right as f64,
        _ => false,
    }
}

fn cmp_records(
    left: &loco_apps::source::LiveRecord,
    right: &loco_apps::source::LiveRecord,
    order: &[loco_lake::OrderKey],
) -> std::cmp::Ordering {
    for key in order {
        let cmp = cmp_value(
            &field_value(left, &key.field),
            &field_value(right, &key.field),
        );
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
    }
    std::cmp::Ordering::Equal
}

fn after_cmp(
    record: &loco_apps::source::LiveRecord,
    after: &[loco_lake::Value],
    order: &[loco_lake::OrderKey],
) -> std::cmp::Ordering {
    for (key, value) in order.iter().zip(after) {
        let cmp = cmp_value(&field_value(record, &key.field), value);
        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }
    }
    std::cmp::Ordering::Equal
}

fn cmp_value(left: &loco_lake::Value, right: &loco_lake::Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let kind = |value: &loco_lake::Value| match value {
        loco_lake::Value::Null => 0,
        loco_lake::Value::Boolean(_) => 1,
        loco_lake::Value::Integer(_) | loco_lake::Value::Float(_) => 2,
        loco_lake::Value::String(_) => 3,
    };
    match kind(left).cmp(&kind(right)) {
        Ordering::Equal => {}
        other => return other,
    }
    match (left, right) {
        (loco_lake::Value::String(left), loco_lake::Value::String(right)) => left.cmp(right),
        (loco_lake::Value::Integer(left), loco_lake::Value::Integer(right)) => left.cmp(right),
        (loco_lake::Value::Boolean(left), loco_lake::Value::Boolean(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

/// `items` are `(id, status)`. `notes` are `(id, body)` for the custom
/// collection. `GET /fail` is 503. The token header is copied into `seen`.
fn start_record_upstream(items: &[(&str, &str)], notes: &[(&str, &str)]) -> String {
    let items: Vec<(String, String)> = items
        .iter()
        .map(|(id, status)| ((*id).to_string(), (*status).to_string()))
        .collect();
    let notes: Vec<(String, String)> = notes
        .iter()
        .map(|(id, body)| ((*id).to_string(), (*body).to_string()))
        .collect();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = answer_records(stream, &items, &notes);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn answer_records(
    mut stream: std::net::TcpStream,
    items: &[(String, String)],
    notes: &[(String, String)],
) -> std::io::Result<()> {
    use std::io::Read;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|window| window == b"\r\n\r\n") || buf.len() > 8192 {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err),
        }
    }
    let req = String::from_utf8_lossy(&buf);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    let token = header_value(&req, "x-token");
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let collection = parts.next().unwrap_or("");
    let id = parts.next();
    if collection == "fail" {
        return write_http(
            &mut stream,
            503,
            "Service Unavailable",
            "text/plain",
            "upstream is down",
        );
    }
    let rows: &[(String, String)] = match collection {
        "items" => items,
        "note" => notes,
        _ => return write_http(&mut stream, 404, "Not Found", "text/plain", "no"),
    };
    let body = match id {
        None => {
            let records: Vec<_> = rows
                .iter()
                .map(|(id, value)| record_json(collection, id, value, token))
                .collect();
            serde_json::to_string(&records).unwrap_or_else(|_| "[]".into())
        }
        Some(id) => match rows.iter().find(|(row_id, _)| row_id == id) {
            Some((id, value)) => serde_json::to_string(&record_json(collection, id, value, token))
                .unwrap_or_else(|_| "{}".into()),
            None => return write_http(&mut stream, 404, "Not Found", "text/plain", "no"),
        },
    };
    write_http(&mut stream, 200, "OK", "application/json", &body)
}

fn record_json(collection: &str, id: &str, value: &str, token: &str) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    match collection {
        "note" => {
            fields.insert("body".into(), serde_json::Value::String(value.to_string()));
        }
        _ => {
            fields.insert(
                "status".into(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }
    fields.insert("seen".into(), serde_json::Value::String(token.to_string()));
    serde_json::json!({ "id": id, "fields": fields })
}

fn header_value<'a>(req: &'a str, name: &str) -> &'a str {
    for line in req.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case(name) {
                return value.trim();
            }
        }
    }
    ""
}

fn write_http(
    stream: &mut std::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

#[test]
fn suite_bundle() {
    let tmp = run_suite(&suites_dir().join("bundle"));
    // `missing_version.hurl`: every refused write left nothing behind.
    let orphan = tmp
        .path()
        .join("schemas/instances/alice/testapp/versions/9.9.9-dev");
    assert!(
        !orphan.exists(),
        "a write to a missing version left {} on disk",
        orphan.display()
    );
}

#[test]
fn suite_hosting() {
    run_suite(&suites_dir().join("hosting"));
}

#[test]
fn suite_hosting_apex() {
    // The apex serves a bundle only when a default site names one. Pinned
    // here rather than through the environment so this suite and
    // `suite_hosting` (which asserts the apex is API-only) can run in the
    // same process without racing on a process-global variable.
    run_suite_with(
        &suites_dir().join("hosting_apex"),
        AppOptions {
            default_site: Some("alice/blog/www".to_string()),
            ..AppOptions::default()
        },
    );
}

#[test]
fn suite_brickos_inventory() {
    run_suite_over_seed(&suites_dir().join("brickos_inventory"), &["brickos"]);
}

#[test]
fn suite_seed_store() {
    let tmp = run_suite_over_seed(&suites_dir().join("seed_store"), &["brickos"]);
    let seed = tmp.path().join("schemas/seed");
    let store = tmp.path().join("schemas/instances/brickos/inventory");

    // The writes landed in the store...
    let version = store.join("versions/0.0.1-dev");
    assert!(version.join("fields/lot/note.yaml").is_file());
    assert!(version.join("bundle/index.html").is_file());
    // ...and the seed is exactly what was committed.
    assert_eq!(
        tree(&seed),
        tree(&crate_dir().join("schemas/seed").join("brickos"))
            .into_iter()
            .map(|(path, bytes)| (Path::new("brickos").join(path), bytes))
            .collect::<Vec<_>>(),
        "a write to a seeded project changed schemas/seed/"
    );

    // Delete the project from the store; the next boot restores it from the
    // seed, without the edits made to the deleted copy.
    std::fs::remove_dir_all(&store).unwrap();
    let _app = loco_apps::server::build_app_with_root(tmp.path());
    assert!(store.join("project.yaml").is_file());
    assert!(store
        .join("versions/0.0.1-dev/collections/lot.yaml")
        .is_file());
    assert!(!store
        .join("versions/0.0.1-dev/fields/lot/note.yaml")
        .exists());
}

/// Every file under `root`, as (relative path, contents), sorted.
fn tree(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn suite_data_crud() {
    run_suite(&suites_dir().join("data_crud"));
}

#[test]
fn suite_data_fields() {
    run_suite(&suites_dir().join("data_fields"));
}

#[test]
fn suite_data_query() {
    run_suite(&suites_dir().join("data_query"));
}

#[test]
fn suite_data_version_pinning() {
    run_suite(&suites_dir().join("data_version_pinning"));
}

#[test]
fn suite_schema_introspect() {
    run_suite(&suites_dir().join("schema_introspect"));
}

#[test]
fn suite_authorization() {
    run_suite(&suites_dir().join("authorization"));
}

#[test]
fn suite_org_role() {
    run_suite(&suites_dir().join("org_role"));
}

#[test]
fn suite_public_policy_on_manifest() {
    run_suite(&suites_dir().join("public_policy_on_manifest"));
}

#[test]
fn suite_qualified_names() {
    run_suite(&suites_dir().join("qualified_names"));
}

#[test]
fn suite_auth_stale_token() {
    run_suite(&suites_dir().join("auth_stale_token"));
}

#[test]
fn suite_project_lifecycle() {
    let tmp = run_suite(&suites_dir().join("project_lifecycle"));
    assert_no_fieldset_files(tmp.path(), "alice/newapp");
}

#[test]
fn suite_site_pins() {
    run_suite(&suites_dir().join("site_pins"));
}

#[test]
fn suite_manifest_deps() {
    run_suite(&suites_dir().join("manifest_deps"));
}

#[test]
fn suite_dependency_access() {
    run_suite(&suites_dir().join("dependency_access"));
}

#[test]
fn suite_data_validation_writes() {
    run_suite(&suites_dir().join("data_validation_writes"));
}

#[test]
fn suite_data_validation_reads() {
    run_suite(&suites_dir().join("data_validation_reads"));
}

#[test]
fn suite_config_names() {
    run_suite(&suites_dir().join("config_names"));
}

#[test]
fn suite_config_values() {
    run_suite(&suites_dir().join("config_values"));
}

#[test]
fn suite_version_lifecycle() {
    let tmp = run_suite(&suites_dir().join("version_lifecycle"));
    assert_no_fieldset_files(tmp.path(), "alice/lab");
}

/// A delete that only clears the in-memory store would pass the Hurl
/// assertions and still leave YAML for the next `SchemaStore::load` to pick
/// up. Check the disk: no fieldset file may survive under `project`.
fn assert_no_fieldset_files(root: &Path, project: &str) {
    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.components().any(|c| c.as_os_str() == "fieldsets") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(&root.join("schemas/instances").join(project), &mut found);
    assert!(
        found.is_empty(),
        "fieldset YAML survived deleting {project}: {found:?}"
    );
}

#[test]
fn suite_auth_credentials() {
    let tmp = run_suite_with(
        &suites_dir().join("auth_credentials"),
        AppOptions::default(),
    );
    let auth = tmp.path().join("auth");

    // Signup password must not be recoverable from the identity file.
    let dora = std::fs::read_to_string(auth.join("identities/dora.json"))
        .expect("auth/identities/dora.json");
    assert!(
        !dora.contains("correct-horse-battery-staple"),
        "identity file holds the login password in plaintext: {dora}"
    );
    assert!(
        !dora.contains("\"password\":"),
        "identity file still has a plaintext `password` field: {dora}"
    );

    // Every identity on disk, seeded ones included, stores an argon2 hash.
    for entry in std::fs::read_dir(auth.join("identities")).unwrap() {
        let path = entry.unwrap().path();
        let identity: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hash = identity["password_hash"].as_str().unwrap_or_default();
        assert!(
            hash.starts_with("$argon2"),
            "{} does not store an argon2 hash: {hash}",
            path.display()
        );
    }

    // The bearer token the suite used must not be recoverable from the key
    // file — only its SHA-256 digest is stored.
    let mut keys = 0;
    for entry in std::fs::read_dir(auth.join("api_keys")).unwrap() {
        let path = entry.unwrap().path();
        let key: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let hash = key["key_hash"].as_str().unwrap_or_default();
        assert!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{} stores something other than a sha256 digest: {hash}",
            path.display()
        );
        keys += 1;
    }
    assert_eq!(keys, 1, "expected the suite to leave one api key on disk");
}

#[test]
fn suite_auth_sessions() {
    let tmp = run_suite_with(&suites_dir().join("auth_sessions"), AppOptions::default());
    let sessions = tmp.path().join("auth/sessions");

    // Two logins, one logout: the logged-out session leaves nothing behind.
    let files: Vec<_> = std::fs::read_dir(&sessions)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        files.len(),
        1,
        "expected the suite to leave one live session on disk, found {files:?}"
    );

    // What it does leave carries an absolute expiry a TTL out, so a restart
    // knows when to stop honoring it.
    let session: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&files[0]).unwrap()).unwrap();
    let created_at = parse_rfc3339(&session, "created_at");
    let expires_at = parse_rfc3339(&session, "expires_at");
    assert!(
        expires_at > chrono::Utc::now(),
        "live session is already expired: {session}"
    );
    assert_eq!(
        expires_at - created_at,
        chrono::Duration::days(loco_apps::auth::local::SESSION_TTL_DAYS),
        "session expiry is not one TTL past creation: {session}"
    );
}

fn parse_rfc3339(session: &serde_json::Value, field: &str) -> chrono::DateTime<chrono::Utc> {
    let raw = session[field]
        .as_str()
        .unwrap_or_else(|| panic!("session file has no {field}: {session}"));
    chrono::DateTime::parse_from_rfc3339(raw)
        .unwrap_or_else(|e| panic!("session {field} is not rfc3339 ({raw}): {e}"))
        .with_timezone(&chrono::Utc)
}

#[test]
fn suite_auth_no_auto_create() {
    // The rest of the suites set LOCO_AUTH_AUTO_CREATE=1 process-wide; this
    // one pins the production default off and checks nothing was squatted.
    let tmp = run_suite_with(
        &suites_dir().join("auth_no_auto_create"),
        AppOptions {
            auth_auto_create: Some(false),
            ..AppOptions::default()
        },
    );
    for dir in ["accounts", "identities"] {
        assert!(
            !tmp.path().join("auth").join(dir).join("acme.json").exists(),
            "login of an unknown handle wrote auth/{dir}/acme.json"
        );
    }
}
