//! Which site a request is for, decided from `Host` before routing.
//!
//! v1 gives every site a derived subdomain of the process it runs on:
//!
//! ```text
//! {site}.{project}.{account}.<listen-host>
//! ```
//!
//! `www` on `ben/blog` at `:3000` is `www.blog.ben.localhost:3000`. Nothing
//! stores that name — it is computed from the instance path
//! (`{account}/{project}/sites/{name}`), so there is no second uniqueness
//! table to keep honest.
//!
//! The listen host itself is not configured. A host is a *site host* when its
//! first three labels name a site that exists; anything else — the apex, an
//! IP, a stale subdomain — is "no site", and `LOCO_DEFAULT_SITE` decides
//! whether that still serves something. That keeps `localhost:3000`,
//! `127.0.0.1:3000`, and a real domain all working with no extra flag.
//!
//! Either way the site's identity is pushed into the request as
//! `X-Project-Id` / `X-Site-Id` when they are absent, so `/data` and
//! `/schema` behave exactly as they do for a local Vite app that sends the
//! headers itself — a hosted frontend does not have to know its own address.
//!
//! What differs is what happens when the client *does* send them:
//!
//! - **Subdomain**: the URL is an assertion about which site this is, so the
//!   headers must agree. One site's URL can never reach another site's data.
//! - **`LOCO_DEFAULT_SITE`**: the apex has no site of its own, so this is a
//!   default, not a constraint. Headers that name another site win — that is
//!   what keeps the apex usable by Studio and by local Vite apps, which talk
//!   to it about whichever site they are browsing.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::http::response::error_response;
use crate::server::AppState;
use crate::{SchemaStore, Site};

// The same two names `http/scope/helpers.rs` reads. They are spelled
// lowercase because that is how they come back out of `HeaderMap`, and they
// appear verbatim in the 400 a disagreeing client gets.
const PROJECT_HEADER: &str = "x-project-id";
const SITE_HEADER: &str = "x-site-id";

/// The site a request resolved to, attached to the request extensions by
/// [`resolve_site`]. Absent when the request is for no site at all (the apex
/// with no `LOCO_DEFAULT_SITE`), which is the API-only process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestSite {
    /// `{account}/{project}`.
    pub project_id: String,
    pub site_name: String,
    /// True when the subdomain named the site, false when it came from
    /// `LOCO_DEFAULT_SITE`. Only the first pins the site headers.
    pub from_host: bool,
}

/// `{account}/{project}/{site}` → `({account}/{project}, {site})`.
///
/// The value `LOCO_DEFAULT_SITE` takes. `None` when it is not that shape;
/// the caller logs and carries on API-only rather than failing boot.
pub fn parse_site_ref(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    let (project_id, site) = raw.rsplit_once('/')?;
    let mut segments = project_id.split('/');
    let account = segments.next().filter(|s| !s.is_empty())?;
    let project = segments.next().filter(|s| !s.is_empty())?;
    if segments.next().is_some() || site.is_empty() {
        return None;
    }
    Some((format!("{account}/{project}"), site.to_string()))
}

/// The site a `Host` value *claims*, from its first three labels alone. Port,
/// case, a trailing root dot, and a bracketed IPv6 literal are handled here.
///
/// Says nothing about whether that site exists — [`site_from_host`] is what
/// decides that, and existence is the whole test for "is this a site host".
fn site_ref_from_host(host: &str) -> Option<(String, String)> {
    // `[::1]:3000` — a bracketed literal never holds labels.
    if host.starts_with('[') {
        return None;
    }
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    // Three labels for the site plus at least one for the host it runs on:
    // `blog.ben.localhost` is an apex, not site `blog` with an empty suffix.
    if labels.len() < 4 || labels.iter().any(|l| l.is_empty()) {
        return None;
    }
    // `127.0.0.1` splits into four labels and would otherwise read as site
    // `127` on `0/0`. A real top-level label is never all digits, so this is
    // what tells an address apart from a name.
    if labels
        .last()
        .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some((
        format!("{}/{}", labels[2], labels[1]),
        labels[0].to_string(),
    ))
}

/// The site named by a `Host` value, if its first three labels name one that
/// exists.
pub fn site_from_host(schema: &SchemaStore, host: &str) -> Option<(String, String)> {
    let (project_id, site_name) = site_ref_from_host(host)?;
    schema
        .sites()
        .has(&Site::to_path(&project_id, &site_name))
        .then_some((project_id, site_name))
}

/// Middleware: resolve the request's site and record it.
///
/// Runs before routing, so the API routes and the bundle fallback agree on
/// which site the URL is.
pub async fn resolve_site(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    // HTTP/1.1 puts it in `Host`; HTTP/2 sends `:authority`, which axum has
    // already folded into the URI.
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| req.uri().host().map(|h| h.to_string()));

    let resolved = host
        .as_deref()
        .and_then(|h| site_from_host(&state.schema, h))
        .map(|(project_id, site_name)| (project_id, site_name, true))
        .or_else(|| {
            state
                .default_site
                .clone()
                .map(|(project_id, site_name)| (project_id, site_name, false))
        });

    if let Some((project_id, site_name, from_host)) = resolved {
        if let Err(response) = fill_site_headers(&mut req, &project_id, &site_name, from_host) {
            return response;
        }
        req.extensions_mut().insert(RequestSite {
            project_id,
            site_name,
            from_host,
        });
    }

    next.run(req).await
}

/// Fill in absent `X-Project-Id` / `X-Site-Id` from the resolved site.
///
/// When `strict` (a subdomain named the site), a header that is present and
/// names a different site is refused instead — the URL is the answer to
/// "which site is this", so disagreeing with it is a bug in the caller, not a
/// preference. A non-strict default leaves what the client sent alone.
fn fill_site_headers(
    req: &mut Request,
    project_id: &str,
    site_name: &str,
    strict: bool,
) -> Result<(), Response> {
    for (name, expected) in [(PROJECT_HEADER, project_id), (SITE_HEADER, site_name)] {
        // An empty header is what the scope helpers already treat as absent,
        // so fill it in rather than refuse it.
        let sent = req
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty());
        match sent {
            Some(sent) if sent == expected => {}
            Some(sent) if strict => {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!(
                        "{name} is {sent}, but this host serves {project_id}/{site_name} — \
                         omit the header or send the matching value"
                    ),
                ));
            }
            Some(_) => {}
            None => {
                // `expected` came from a slug path, so it is always legal.
                if let Ok(value) = HeaderValue::from_str(expected) {
                    req.headers_mut().insert(name, value);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_site_reference() {
        assert_eq!(
            parse_site_ref("loco/studio/studio"),
            Some(("loco/studio".to_string(), "studio".to_string()))
        );
        assert_eq!(
            parse_site_ref("  ben/blog/www  "),
            Some(("ben/blog".to_string(), "www".to_string()))
        );
    }

    #[test]
    fn refuses_a_reference_that_is_not_account_project_site() {
        for bad in [
            "",
            "studio",
            "loco/studio",
            "a/b/c/d",
            "loco//studio",
            "/a/b",
        ] {
            assert_eq!(parse_site_ref(bad), None, "expected {bad:?} to be refused");
        }
    }

    #[test]
    fn reads_a_site_out_of_a_subdomain() {
        let expected = Some(("ben/blog".to_string(), "www".to_string()));
        // Port, case, and a trailing root dot are all noise.
        for host in [
            "www.blog.ben.localhost:3000",
            "www.blog.ben.localhost",
            "WWW.Blog.Ben.localhost:3000",
            "www.blog.ben.localhost.:3000",
            // Any suffix at all, of any depth — the listen host is not
            // configured, so it is simply whatever is left over.
            "www.blog.ben.example.com",
        ] {
            assert_eq!(site_ref_from_host(host), expected, "host {host:?}");
        }
    }

    #[test]
    fn a_host_with_no_room_for_a_site_names_none() {
        for host in [
            // Too few labels: these are apexes, not sites with an empty
            // suffix.
            "localhost:3000",
            "blog.ben.localhost",
            "example.com",
            // Addresses, v4 and v6.
            "127.0.0.1:3000",
            "[::1]:3000",
            // Empty labels are not names.
            "..blog.ben.localhost",
        ] {
            assert_eq!(site_ref_from_host(host), None, "host {host:?}");
        }
    }
}
