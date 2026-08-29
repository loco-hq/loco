//! The router fallback: files from the site's **pinned version's** bundle.
//!
//! This is what makes a site a URL. Everything the API owns is nested above
//! it; whatever is left over is a request for the frontend that the pinned
//! version ships, read straight out of that version's file tree. Rolling a
//! site forward or back is repinning the site — no files move.
//!
//! Rules, in order:
//!
//! 1. **Reserved prefixes always win.** `/data` `/schema` `/config` `/auth`
//!    answer JSON here, never HTML, so a mistyped API path stays a JSON 404
//!    instead of quietly returning an SPA shell that a client then tries to
//!    parse.
//! 2. **No site, no files.** Without a [`RequestSite`] (the apex with no
//!    `LOCO_DEFAULT_SITE`) this is the API-only process it has always been.
//! 3. **SPA fallback** to the tree's `index.html` when the miss looks like a
//!    navigation: an extensionless path, or one whose `Accept` asks for HTML.
//!    A missing hashed asset is a 404 — answering it with HTML turns a bad
//!    deploy into a syntax error in the browser console.
//! 4. **A missing bundle 404s.** It is not a boot failure; a version that has
//!    not been built yet is an ordinary state.
//!
//! Nothing here knows about any particular app. Studio is a bundle on a
//! `loco/studio` version like any other frontend.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::bundle::ENTRY_FILE;
use crate::http::authz::is_draft_version;
use crate::http::host::RequestSite;
use crate::http::response::error_response;
use crate::server::AppState;
use crate::{Bundle, Site};

/// Paths the API owns. A request under one of these never sees the bundle.
const RESERVED_PREFIXES: [&str; 4] = ["/data", "/schema", "/config", "/auth"];

pub async fn serve_site_files(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let path = req.uri().path().to_string();

    if is_reserved(&path) || !matches!(*req.method(), Method::GET | Method::HEAD) {
        return no_such_endpoint(&path);
    }

    let Some(site_ref) = req.extensions().get::<RequestSite>().cloned() else {
        return no_such_endpoint(&path);
    };

    let Some(site) = state
        .schema
        .sites()
        .get(&Site::to_path(&site_ref.project_id, &site_ref.site_name))
    else {
        return no_such_endpoint(&path);
    };
    let version = site.version();
    if version.is_empty() {
        return no_such_endpoint(&path);
    }
    let key = Bundle::to_path(&site_ref.project_id, version);

    let Some(requested) = request_path(&path) else {
        return no_such_endpoint(&path);
    };

    // A read error here is a traversal attempt or a symlink in the tree, both
    // of which the adapter refuses. Either way the file is not servable.
    if let Ok(Some(bytes)) = state.schema.bundles().read_file(&key, &requested) {
        return file_response(&requested, bytes, is_draft_version(version));
    }

    if !wants_html(&req, &requested) {
        return no_such_endpoint(&path);
    }
    match state.schema.bundles().read_file(&key, ENTRY_FILE) {
        Ok(Some(bytes)) => file_response(ENTRY_FILE, bytes, is_draft_version(version)),
        _ => no_such_endpoint(&path),
    }
}

fn is_reserved(path: &str) -> bool {
    RESERVED_PREFIXES
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// JSON, always — the one 404 shape every client already parses.
fn no_such_endpoint(path: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, &format!("no such endpoint: {path}"))
}

/// The URL path as a tree-relative file path. `/` is the entry file. `None`
/// when the path cannot be a member of a tree at all; the adapter re-checks
/// `..` and friends, so this only has to decode.
fn request_path(path: &str) -> Option<String> {
    let decoded = percent_decode(path.trim_start_matches('/'))?;
    if decoded.is_empty() || decoded.ends_with('/') {
        return Some(ENTRY_FILE.to_string());
    }
    Some(decoded)
}

/// Percent-decode a URL path segment sequence. `None` when the escape is
/// malformed or decodes to a NUL or a byte sequence that is not UTF-8 —
/// tree member paths are UTF-8 strings.
fn percent_decode(raw: &str) -> Option<String> {
    if !raw.contains('%') {
        return Some(raw.to_string());
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = raw.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Does this miss look like a navigation the SPA router should answer?
///
/// Extensionless (`/settings`) or an explicit HTML `Accept` (a browser
/// address-bar load). `fetch` of a hashed asset is neither, and gets its 404.
fn wants_html(req: &Request, requested: &str) -> bool {
    let last = requested.rsplit('/').next().unwrap_or(requested);
    if !last.contains('.') {
        return true;
    }
    req.headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"))
}

fn file_response(path: &str, bytes: Vec<u8>, is_draft: bool) -> Response {
    // Published version bytes never change, so hashed assets can be cached
    // forever. `index.html` is the *pointer* a visitor reloads to see a pin
    // move, so it always revalidates — and a draft's bytes move under a
    // running site, so nothing in one is cacheable.
    let cache = if path == ENTRY_FILE || is_draft {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(path)),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        Body::from(bytes),
    )
        .into_response()
}

fn content_type(path: &str) -> &'static str {
    let ext = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_prefixes_are_reserved_but_lookalikes_are_not() {
        for reserved in [
            "/data",
            "/data/x/list",
            "/schema/a/b/c",
            "/config",
            "/auth/me",
        ] {
            assert!(is_reserved(reserved), "{reserved} should be reserved");
        }
        for served in [
            "/",
            "/database",
            "/authors/1",
            "/assets/data.js",
            "/schemas",
        ] {
            assert!(!is_reserved(served), "{served} should be servable");
        }
    }

    #[test]
    fn root_and_directory_paths_are_the_entry_file() {
        assert_eq!(request_path("/").unwrap(), ENTRY_FILE);
        assert_eq!(request_path("/docs/").unwrap(), ENTRY_FILE);
        assert_eq!(request_path("/assets/app.js").unwrap(), "assets/app.js");
    }

    #[test]
    fn percent_escapes_decode_and_malformed_ones_are_refused() {
        assert_eq!(request_path("/a%20b.png").unwrap(), "a b.png");
        assert_eq!(percent_decode("plain/path.js").unwrap(), "plain/path.js");
        // An encoded traversal decodes to one the adapter then refuses; what
        // must never happen is it decoding into something that looks safe.
        assert_eq!(percent_decode("%2e%2e/x").unwrap(), "../x");
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%2"), None);
    }

    #[test]
    fn content_types_cover_a_vite_dist() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("assets/index-a1b2c3.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type("assets/index-a1b2c3.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type("logo.SVG"), "image/svg+xml");
        assert_eq!(content_type("noextension"), "application/octet-stream");
    }
}
