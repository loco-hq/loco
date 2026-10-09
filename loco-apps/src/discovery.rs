//! Hand-written discovery document.
//!
//! `GET /.well-known/loco.json` is the interface for an agent that has no
//! copy of this repo. The document is [`discovery.json`]: parsed once, with
//! `version` and `{listen-host}` filled when the request arrives. `routes`
//! is the only route list. Prefix summaries name the reserved prefixes, and
//! the 404 catalog for a prefix is that list filtered to the prefix. The
//! Hurl suite requests every route, and a 405 fails the probe: that is the
//! answer for a path mounted under a different method.

use std::sync::{Arc, OnceLock};

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::server::AppState;

pub const DISCOVERY_PATH: &str = "/.well-known/loco.json";
pub const LLMS_PATH: &str = "/llms.txt";

const LISTEN_HOST: &str = "{listen-host}";

const LLMS_BODY: &str = "\
Loco HTTP API.\n\
\n\
Read /.well-known/loco.json and follow it. That JSON document is the\n\
interface: how to sign up, log in, create a project, declare a collection,\n\
write records, upload a bundle, and pin a site. Do not guess paths.\n\
";

fn template() -> &'static Value {
    static TEMPLATE: OnceLock<Value> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        serde_json::from_str(include_str!("discovery.json")).expect("discovery.json parses")
    })
}

fn routes() -> &'static [Value] {
    template()["routes"]
        .as_array()
        .expect("discovery routes is an array")
}

fn prefix_entries() -> &'static [Value] {
    template()["prefixes"]
        .as_array()
        .expect("discovery prefixes is an array")
}

fn route_method(route: &Value) -> &str {
    route["method"].as_str().expect("route method")
}

fn route_path(route: &Value) -> &str {
    route["path"].as_str().expect("route path")
}

/// `Some` when `path` is a reserved API prefix or a path under one.
/// `/database` and `/action` are not reserved.
pub fn reserved_prefix(path: &str) -> Option<&'static str> {
    prefix_entries().iter().find_map(|item| {
        let prefix = item["prefix"].as_str().expect("prefix");
        let matched = path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'));
        matched.then_some(prefix)
    })
}

fn is_prefix_root(path: &str) -> bool {
    let trimmed = path.trim_end_matches('/');
    prefix_entries()
        .iter()
        .any(|item| item["prefix"].as_str() == Some(trimmed))
}

fn path_in_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

fn prefix_catalog(prefix: &str) -> String {
    let listed = routes()
        .iter()
        .filter(|route| path_in_prefix(route_path(route), prefix))
        .map(|route| format!("{} {}", route_method(route), route_path(route)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{prefix} is an API prefix. Its routes: {listed}")
}

pub fn api_miss_body(path: &str) -> Value {
    let error = if is_prefix_root(path) {
        prefix_catalog(path.trim_end_matches('/'))
    } else {
        format!("no such endpoint: {path}")
    };
    serde_json::json!({
        "ok": false,
        "error": error,
        "see": DISCOVERY_PATH,
    })
}

/// JSON 404 for an API miss. A bare prefix names that prefix's routes.
/// Anything else keeps `no such endpoint` and sets `see`.
pub fn api_miss(path: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(api_miss_body(path))).into_response()
}

/// Replace each `{segment}` with `x`, so a probe is one path segment and
/// does not collide with a static segment such as `list`.
pub fn probe_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start + 1..].find('}') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        out.push('x');
        rest = &rest[start + 1 + end + 1..];
    }
    out.push_str(rest);
    out
}

fn substitute(value: &mut Value, listen_host: &str) {
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                substitute(child, listen_host);
            }
        }
        Value::Array(items) => {
            for child in items {
                substitute(child, listen_host);
            }
        }
        Value::String(text) if text.contains(LISTEN_HOST) => {
            *text = text.replace(LISTEN_HOST, listen_host);
        }
        _ => {}
    }
}

/// The document with `version` and `{listen-host}` filled in.
fn document_for(listen_host: &str) -> Value {
    let mut doc = template().clone();
    doc["version"] = Value::String(env!("CARGO_PKG_VERSION").to_string());
    substitute(&mut doc, listen_host);
    doc
}

/// Hostname an agent should put in front of `{site}.{project}.{account}`.
///
/// A site host drops its first three labels, which are the site. An IP
/// becomes `localhost` with the same port: `*.localhost` resolves on the
/// machine serving the response, and an IP cannot take those labels.
fn request_listen_host(raw_host: &str, site_host: bool) -> String {
    let (name, port) = split_host_port(raw_host);
    let name = if site_host {
        let labels: Vec<&str> = name
            .trim_end_matches('.')
            .split('.')
            .filter(|label| !label.is_empty())
            .collect();
        if labels.len() >= 4 {
            labels[3..].join(".")
        } else {
            name
        }
    } else if is_ip_host(&name) {
        "localhost".to_string()
    } else {
        name.trim_end_matches('.').to_string()
    };
    match port {
        Some(port) => format!("{name}:{port}"),
        None => name,
    }
}

fn split_host_port(host: &str) -> (String, Option<String>) {
    let host = host.trim();
    if host.is_empty() {
        return ("localhost".to_string(), None);
    }
    if let Some(rest) = host.strip_prefix('[') {
        if let Some((addr, port)) = rest.rsplit_once("]:") {
            if port.chars().all(|c| c.is_ascii_digit()) {
                return (format!("[{addr}]"), Some(port.to_string()));
            }
        }
        return (host.to_string(), None);
    }
    if let Some((name, port)) = host.rsplit_once(':') {
        if !name.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return (name.to_string(), Some(port.to_string()));
        }
    }
    (host.to_string(), None)
}

fn is_ip_host(name: &str) -> bool {
    if name.starts_with('[') {
        return true;
    }
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() == 4
        && labels
            .iter()
            .all(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()))
}

pub async fn document(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let raw = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim();
    let site_host = crate::http::host::site_from_host(&state.schema, raw).is_some();
    let listen_host = request_listen_host(raw, site_host);
    let bytes = serde_json::to_vec_pretty(&document_for(&listen_host))
        .expect("discovery document serializes");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response()
}

pub async fn llms_txt() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        LLMS_BODY,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HURL: &str = include_str!("../tests/suites/discovery/discovery.hurl");
    /// The example page an agent saves as `index.html` and uploads. The same
    /// bytes are embedded in `discovery.json` and zipped for the Hurl suite.
    const INDEX_HTML: &str = include_str!("../tests/suites/discovery/index.html");

    #[test]
    fn probe_path_replaces_each_placeholder_with_one_segment() {
        assert_eq!(probe_path("/auth/users/{id}"), "/auth/users/x");
        assert_eq!(
            probe_path("/schema/{account}/{project}/{version}/field/{collection}/{name}"),
            "/schema/x/x/x/field/x/x"
        );
        assert_eq!(probe_path("/data/{collection}/list"), "/data/x/list");
        assert_eq!(probe_path(DISCOVERY_PATH), DISCOVERY_PATH);
        assert_eq!(
            probe_path("/config/dataset/{account}/{project}/list"),
            "/config/dataset/x/x/list"
        );
    }

    #[test]
    fn listen_host_comes_from_the_request_host() {
        assert_eq!(
            request_listen_host("127.0.0.1:3917", false),
            "localhost:3917"
        );
        assert_eq!(request_listen_host("[::1]:3917", false), "localhost:3917");
        assert_eq!(
            request_listen_host("localhost:3917", false),
            "localhost:3917"
        );
        assert_eq!(request_listen_host("example.com", false), "example.com");
        assert_eq!(
            request_listen_host("api.example.com:443", false),
            "api.example.com:443"
        );
        assert_eq!(
            request_listen_host("dev.inventory.reseller.localhost:3917", true),
            "localhost:3917"
        );
        assert_eq!(
            request_listen_host("www.blog.ben.example.com", true),
            "example.com"
        );
        assert_eq!(
            request_listen_host("dev.inventory.lego_reseller.api.example.com:8443", true),
            "api.example.com:8443"
        );
        assert_eq!(request_listen_host("", false), "localhost");
    }

    #[test]
    fn prefix_roots_name_routes_and_other_misses_keep_the_pointer() {
        assert!(!prefix_entries().is_empty());
        for item in prefix_entries() {
            let prefix = item["prefix"].as_str().unwrap();
            assert!(item.get("routes").is_none(), "{prefix} repeats routes");
            let body = api_miss_body(prefix);
            let error = body["error"].as_str().unwrap();
            assert!(!error.contains("no such endpoint"), "{prefix}: {error}");
            assert_eq!(body["see"], DISCOVERY_PATH);
            assert!(is_prefix_root(&format!("{prefix}/")));
            let mut count = 0;
            for route in routes() {
                if path_in_prefix(route_path(route), prefix) {
                    count += 1;
                    let listed = format!("{} {}", route_method(route), route_path(route));
                    assert!(error.contains(&listed), "{prefix} catalog missing {listed}");
                }
            }
            assert!(count > 0, "{prefix} has no routes");
        }
        assert!(!is_prefix_root("/auth/login"));
        assert_eq!(reserved_prefix("/database"), None);
        assert_eq!(reserved_prefix("/action"), None);
        assert_eq!(reserved_prefix("/actionable"), None);

        let miss = api_miss_body("/schema/foo");
        assert_eq!(miss["error"], "no such endpoint: /schema/foo");
        assert_eq!(miss["see"], DISCOVERY_PATH);
        let root = api_miss_body("/");
        assert_eq!(root["error"], "no such endpoint: /");
        assert_eq!(root["see"], DISCOVERY_PATH);
    }

    #[test]
    fn document_lists_the_route_table_and_describes_signup_on_main() {
        assert_eq!(template()["version"], "");
        let doc = document_for("api.example:3917");
        assert_eq!(doc["name"], "loco");
        assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(doc["discovery"], DISCOVERY_PATH);
        assert_eq!(doc["docs"].as_array().unwrap().len(), 0);
        assert_eq!(
            doc["site"]["example"],
            "http://dev.inventory.lego_reseller.api.example:3917/"
        );
        assert_eq!(
            doc["site"]["url"],
            "http://{site}.{project}.{account}.api.example:3917/"
        );
        let note = doc["site"]["note"].as_str().unwrap();
        assert!(note.contains("wildcard DNS"), "{note}");
        assert!(note.contains("LOCO_DEFAULT_SITE"), "{note}");
        assert!(note.contains("#145"), "{note}");
        let open = doc["guide"][8]["do"].as_str().unwrap();
        let publish = doc["guide"][9]["do"].as_str().unwrap();
        assert!(open.contains("http://dev.inventory.lego_reseller.api.example:3917/"));
        assert!(publish.contains("http://www.inventory.lego_reseller.api.example:3917/"));
        assert!(
            !open.contains(":3000") && !open.contains("{port}"),
            "{open}"
        );
        assert!(!publish.contains(":3000") && !publish.contains("{port}"));
        let handle = doc["signup"]["handle"].as_str().unwrap();
        assert!(handle.contains("1-63"), "{handle}");
        assert!(handle.contains("a-z"), "{handle}");
        assert!(handle.contains("lego_reseller"), "{handle}");
        let rejected = doc["signup"]["rejected_handle"].as_str().unwrap();
        assert!(rejected.contains("400"), "{rejected}");
        assert!(rejected.contains("1-63"), "{rejected}");
        assert!(
            rejected.contains("handle name \"lego-reseller\""),
            "{rejected}"
        );
        let missing = doc["signup"]["missing_field"].as_str().unwrap();
        assert!(missing.contains("400"), "{missing}");
        assert!(missing.contains("username"), "{missing}");
        assert!(missing.contains("password is required"), "{missing}");
        let login = doc["login"]["failure"].as_str().unwrap();
        assert!(login.contains("401"), "{login}");
        assert!(login.contains("invalid credentials"), "{login}");
        assert!(login.contains("illegal handle"), "{login}");
        assert!(login.contains("org handle"), "{login}");
        let org = doc["names"]["org"].as_str().unwrap();
        assert!(org.contains("POST /config/org"), "{org}");
        assert!(org.contains("400") && org.contains("409"), "{org}");
        assert!(org.contains("handle name \"my-org\""), "{org}");
        let member = doc["names"]["member"].as_str().unwrap();
        assert!(member.contains("400") && member.contains("404"), "{member}");
        assert!(member.contains("unknown account:"), "{member}");
        assert!(member.contains("bad-handle"), "{member}");
        assert!(member.contains("201"), "{member}");
        assert!(member.contains("user already exists"), "{member}");
        assert!(member.contains("POST /config/project"), "{member}");
        assert!(!member.contains("lists project members"), "{member}");
        assert!(!member.contains("lists org members"), "{member}");
        assert_eq!(doc["routes"].as_array().unwrap().len(), routes().len());
        for route in doc["routes"].as_array().unwrap() {
            assert!(!route_method(route).is_empty());
            assert!(route_path(route).starts_with('/'));
            assert!(!route["auth"].as_str().unwrap().is_empty());
            assert!(!route["summary"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn guide_requests_are_listed_routes() {
        let doc = document_for("localhost");
        let guide = doc["guide"].as_array().unwrap();
        assert!(!guide.is_empty());
        let mut saw_page = false;
        for step in guide {
            if step.get("index_html").and_then(Value::as_str).is_some() {
                saw_page = true;
                assert_eq!(step["index_html"].as_str().unwrap(), INDEX_HTML);
            }
            for request in step["requests"].as_array().unwrap() {
                let method = request["method"].as_str().unwrap();
                let path = request["path"].as_str().unwrap();
                assert!(
                    routes()
                        .iter()
                        .any(|route| route_method(route) == method && route_path(route) == path),
                    "guide request {method} {path} is not in routes"
                );
            }
        }
        assert!(saw_page, "guide does not include the page to upload");
        assert!(INDEX_HTML.contains("discovery page"));
        assert!(INDEX_HTML.contains("/data/parts/list"));
        assert!(INDEX_HTML.contains("/data/parts/add"));
        assert!(INDEX_HTML.contains("/data/parts/update/"));
        assert!(!INDEX_HTML.contains("Authorization"));
    }

    #[test]
    fn hurl_probes_every_route_and_the_route_count() {
        let lines: Vec<&str> = HURL.lines().collect();
        let count_line = format!("jsonpath \"$.routes\" count == {}", routes().len());
        assert!(
            lines.iter().any(|line| *line == count_line),
            "discovery.hurl is missing {count_line}"
        );
        for route in routes() {
            let line = format!(
                "{} http://127.0.0.1:{{{{port}}}}{}",
                route_method(route),
                probe_path(route_path(route))
            );
            let Some(idx) = lines.iter().rposition(|candidate| *candidate == line) else {
                panic!("discovery.hurl is missing {line}");
            };
            let end = (idx + 6).min(lines.len());
            assert!(
                lines[idx + 1..end]
                    .iter()
                    .any(|candidate| candidate.trim() == "status != 405"),
                "discovery.hurl probe {line} does not assert status != 405"
            );
        }
    }

    #[test]
    fn fixture_zip_is_the_documented_page() {
        let zip = include_bytes!("../tests/suites/discovery/dist.zip");
        let html = INDEX_HTML.as_bytes();
        assert!(
            zip.windows(html.len()).any(|window| window == html),
            "dist.zip does not contain index.html uncompressed"
        );
    }

    #[test]
    fn llms_txt_points_at_the_document_and_does_not_describe_a_miss() {
        assert!(LLMS_BODY.contains(DISCOVERY_PATH));
        assert!(!LLMS_BODY.contains("no such endpoint"));
    }

    struct AbortOnDrop(tokio::task::JoinHandle<()>);

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    #[tokio::test]
    async fn every_listed_route_is_mounted() {
        let tmp = tempfile::tempdir().unwrap();
        let options = crate::server::AppOptions {
            sqlite_path: Some(tmp.path().join("probe.db")),
            ..crate::server::AppOptions::default()
        };
        let app = crate::server::build_app_with_options(tmp.path(), options);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let _abort = AbortOnDrop(server);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .no_proxy()
            .build()
            .unwrap();
        let discovery_url = format!("http://127.0.0.1:{port}{DISCOVERY_PATH}");
        let mut ready = false;
        for _ in 0..20 {
            if client.get(&discovery_url).send().await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(ready, "probe server did not accept a connection");

        let discovery = client.get(&discovery_url).send().await.unwrap();
        assert_eq!(discovery.status(), 200);
        let discovery_body: Value = serde_json::from_str(&discovery.text().await.unwrap()).unwrap();
        assert_eq!(
            discovery_body["site"]["example"],
            format!("http://dev.inventory.lego_reseller.localhost:{port}/")
        );
        assert!(discovery_body["site"]["note"]
            .as_str()
            .unwrap()
            .contains("LOCO_DEFAULT_SITE"));

        for route in routes() {
            let method_name = route_method(route);
            let path = route_path(route);
            let url = format!("http://127.0.0.1:{port}{}", probe_path(path));
            let method = reqwest::Method::from_bytes(method_name.as_bytes()).unwrap();
            let response = client
                .request(method, &url)
                .send()
                .await
                .unwrap_or_else(|err| panic!("{method_name} {url}: {err}"));
            let status = response.status();
            let body = response.text().await.unwrap();
            assert_ne!(
                status.as_u16(),
                405,
                "{method_name} {path} is not mounted (405): {body}"
            );
            if path == DISCOVERY_PATH || path == LLMS_PATH {
                assert_eq!(status, 200, "{method_name} {path} -> {status} {body}");
            } else {
                assert!(
                    !body.contains("no such endpoint"),
                    "{method_name} {path} fell through ({status}): {body}"
                );
            }
        }

        let auth = client
            .get(format!("http://127.0.0.1:{port}/auth"))
            .send()
            .await
            .unwrap();
        assert_eq!(auth.status(), 404);
        let auth_body: Value = serde_json::from_str(&auth.text().await.unwrap()).unwrap();
        let auth_error = auth_body["error"].as_str().unwrap();
        assert!(auth_error.contains("POST /auth/users"), "{auth_error}");
        assert!(auth_error.contains("POST /auth/login"), "{auth_error}");
        assert!(!auth_error.contains("no such endpoint"), "{auth_error}");
        assert_eq!(auth_body["see"], DISCOVERY_PATH);

        let unknown = client
            .get(format!("http://127.0.0.1:{port}/schema/foo"))
            .send()
            .await
            .unwrap();
        let unknown_body: Value = serde_json::from_str(&unknown.text().await.unwrap()).unwrap();
        assert_eq!(unknown_body["error"], "no such endpoint: /schema/foo");
        assert_eq!(unknown_body["see"], DISCOVERY_PATH);

        let actions = client
            .get(format!("http://127.0.0.1:{port}/actions"))
            .send()
            .await
            .unwrap();
        assert_eq!(actions.status(), 400);
        let actions_body = actions.text().await.unwrap();
        assert!(actions_body.contains("missing project"), "{actions_body}");
    }
}
