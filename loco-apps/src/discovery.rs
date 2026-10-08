//! Hand-written discovery document.
//!
//! `GET /.well-known/loco.json` is the interface for an agent that has no
//! copy of this repo. [`ROUTES`] is the source of truth: the JSON `routes`
//! and `prefixes` arrays are built from it, the Hurl suite requests every
//! entry, and the unit tests refuse a guide step that names a path the
//! table does not list.
//!
//! Signup below describes the server as it answers today. A handle that
//! fails `LocalAuthAdapter::is_valid_handle` is 401 `invalid credentials`,
//! the same text as a failed login. A body missing `username` or `name` is
//! 422 text/plain from the JSON extractor. If #147 / PR #149 merges first,
//! change `rejected_handle` to the 400 rule sentence and `missing_field` to
//! the 400 JSON missing-field error, then rebase.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

pub const DISCOVERY_PATH: &str = "/.well-known/loco.json";
pub const LLMS_PATH: &str = "/llms.txt";

pub const RESERVED_PREFIXES: [&str; 5] = ["/data", "/schema", "/config", "/auth", "/actions"];

const AUTH_NONE: &str = "none";
const AUTH_BEARER: &str = "bearer";
const AUTH_SITE: &str = "site";

struct Route {
    method: &'static str,
    path: &'static str,
    auth: &'static str,
    summary: &'static str,
}

const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: DISCOVERY_PATH,
        auth: AUTH_NONE,
        summary: "This document. Served on every host. A bundle cannot shadow it.",
    },
    Route {
        method: "GET",
        path: LLMS_PATH,
        auth: AUTH_NONE,
        summary: "Plain text that points here. Served on every host.",
    },
    Route {
        method: "POST",
        path: "/auth/login",
        auth: AUTH_NONE,
        summary: "Log in. Body {username, password}. 200 data.token. Does not create an account.",
    },
    Route {
        method: "POST",
        path: "/auth/logout",
        auth: AUTH_BEARER,
        summary: "End the session named by the bearer token.",
    },
    Route {
        method: "GET",
        path: "/auth/me",
        auth: AUTH_BEARER,
        summary: "The signed-in user.",
    },
    Route {
        method: "POST",
        path: "/auth/users",
        auth: AUTH_NONE,
        summary: "Sign up. Body {username, name, password}. 201 and no token. Log in next.",
    },
    Route {
        method: "PUT",
        path: "/auth/users/{id}",
        auth: AUTH_BEARER,
        summary: "Change the signed-in user's name. {id} is data.id from signup, not the handle.",
    },
    Route {
        method: "DELETE",
        path: "/auth/users/{id}",
        auth: AUTH_BEARER,
        summary: "Delete the signed-in user. {id} is data.id from signup, not the handle.",
    },
    Route {
        method: "POST",
        path: "/auth/api-keys",
        auth: AUTH_BEARER,
        summary: "Create an API key. Body {label}. data.key is returned once.",
    },
    Route {
        method: "GET",
        path: "/auth/api-keys/list",
        auth: AUTH_BEARER,
        summary: "List API keys. The secret is not included.",
    },
    Route {
        method: "DELETE",
        path: "/auth/api-keys/{id}",
        auth: AUTH_BEARER,
        summary: "Revoke an API key. {id} is data.id from create, not the secret.",
    },
    Route {
        method: "POST",
        path: "/config/project",
        auth: AUTH_BEARER,
        summary: "Create a project. Bootstraps version 0.0.1-dev, dataset dev, and site dev.",
    },
    Route {
        method: "GET",
        path: "/config/project/list",
        auth: AUTH_BEARER,
        summary: "Projects the caller can see.",
    },
    Route {
        method: "GET",
        path: "/config/project/{account}/{project}",
        auth: AUTH_BEARER,
        summary: "One project. {account} is the handle. {project} is the project name.",
    },
    Route {
        method: "POST",
        path: "/config/dataset/{account}/{project}",
        auth: AUTH_BEARER,
        summary: "Create a dataset. The bootstrap dataset dev is enough for a first app.",
    },
    Route {
        method: "GET",
        path: "/config/dataset/{account}/{project}/list",
        auth: AUTH_BEARER,
        summary: "Datasets of the project.",
    },
    Route {
        method: "GET",
        path: "/config/dataset/{account}/{project}/{name}",
        auth: AUTH_BEARER,
        summary: "One dataset.",
    },
    Route {
        method: "POST",
        path: "/config/site/{account}/{project}",
        auth: AUTH_BEARER,
        summary: "Create a site. Body {name, label, version, dataset}.",
    },
    Route {
        method: "GET",
        path: "/config/site/{account}/{project}/list",
        auth: AUTH_BEARER,
        summary: "Sites of the project.",
    },
    Route {
        method: "GET",
        path: "/config/site/{account}/{project}/{name}",
        auth: AUTH_BEARER,
        summary: "One site, including the version and dataset it pins.",
    },
    Route {
        method: "PUT",
        path: "/config/site/{account}/{project}/{name}",
        auth: AUTH_BEARER,
        summary: "Re-pin a site. Body {version, dataset}.",
    },
    Route {
        method: "POST",
        path: "/config/version/{account}/{project}",
        auth: AUTH_BEARER,
        summary: "Create a version. Body {version, from} copies schema and the bundle, not records.",
    },
    Route {
        method: "GET",
        path: "/config/version/{account}/{project}/list",
        auth: AUTH_BEARER,
        summary: "Versions of the project.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/manifest",
        auth: AUTH_BEARER,
        summary: "The version's dependencies and public_permission_sets.",
    },
    Route {
        method: "PUT",
        path: "/schema/{account}/{project}/{version}/manifest",
        auth: AUTH_BEARER,
        summary: "Update the manifest. {\"public_permission_sets\":[\"parts_public\"]} assigns that set to public.",
    },
    Route {
        method: "POST",
        path: "/schema/{account}/{project}/{version}/collection",
        auth: AUTH_BEARER,
        summary: "Create a collection. Body {name, label, label_plural}. Draft version only.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/collection/list",
        auth: AUTH_BEARER,
        summary: "Collections in the version.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/collection/{name}",
        auth: AUTH_BEARER,
        summary: "One collection.",
    },
    Route {
        method: "PUT",
        path: "/schema/{account}/{project}/{version}/collection/{name}",
        auth: AUTH_BEARER,
        summary: "Update a collection's label. Draft version only.",
    },
    Route {
        method: "DELETE",
        path: "/schema/{account}/{project}/{version}/collection/{name}",
        auth: AUTH_BEARER,
        summary: "Delete a collection document. Records in the dataset are separate.",
    },
    Route {
        method: "POST",
        path: "/schema/{account}/{project}/{version}/field",
        auth: AUTH_BEARER,
        summary: "Create a field. Body {collection, name, type, label, required}. Draft only.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/field/{collection}/list",
        auth: AUTH_BEARER,
        summary: "Fields of a collection, in display order.",
    },
    Route {
        method: "PUT",
        path: "/schema/{account}/{project}/{version}/field/{collection}/{name}",
        auth: AUTH_BEARER,
        summary: "Update a field. Draft version only.",
    },
    Route {
        method: "DELETE",
        path: "/schema/{account}/{project}/{version}/field/{collection}/{name}",
        auth: AUTH_BEARER,
        summary: "Delete a field. Draft version only.",
    },
    Route {
        method: "POST",
        path: "/schema/{account}/{project}/{version}/permission_set",
        auth: AUTH_BEARER,
        summary: "Create a permission set. Body {name, label, collections}. Draft only.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/permission_set/list",
        auth: AUTH_BEARER,
        summary: "Permission sets in the version.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/permission_set/{name}",
        auth: AUTH_BEARER,
        summary: "One permission set.",
    },
    Route {
        method: "PUT",
        path: "/schema/{account}/{project}/{version}/permission_set/{name}",
        auth: AUTH_BEARER,
        summary: "Replace a permission set. Sending collections replaces that list.",
    },
    Route {
        method: "PUT",
        path: "/schema/{account}/{project}/{version}/bundle",
        auth: AUTH_BEARER,
        summary: "Upload a zip as the version's frontend. Draft only. 200 and metadata.",
    },
    Route {
        method: "GET",
        path: "/schema/{account}/{project}/{version}/bundle",
        auth: AUTH_BEARER,
        summary: "Metadata for the uploaded frontend (hash, uploaded_at, size, files), not the files.",
    },
    Route {
        method: "DELETE",
        path: "/schema/{account}/{project}/{version}/bundle",
        auth: AUTH_BEARER,
        summary: "Remove the frontend from a draft. 200.",
    },
    Route {
        method: "POST",
        path: "/data/{collection}/add",
        auth: AUTH_SITE,
        summary: "Create a record. The body is the fields object itself. 201. data.id is the record id.",
    },
    Route {
        method: "GET",
        path: "/data/{collection}/list",
        auth: AUTH_SITE,
        summary: "Every record. data is an array. Read data[0].fields.sku, not data[0].sku.",
    },
    Route {
        method: "GET",
        path: "/data/{collection}/get/{id}",
        auth: AUTH_SITE,
        summary: "One record. {id} is data.id from add.",
    },
    Route {
        method: "PUT",
        path: "/data/{collection}/update/{id}",
        auth: AUTH_SITE,
        summary: "Patch a record. The body is the fields you are changing. 200.",
    },
    Route {
        method: "DELETE",
        path: "/data/{collection}/delete/{id}",
        auth: AUTH_SITE,
        summary: "Delete one record.",
    },
    Route {
        method: "GET",
        path: "/data/{collection}/fields",
        auth: AUTH_SITE,
        summary: "Field metadata for the site's pinned version: name, type, label, required, options.",
    },
    Route {
        method: "POST",
        path: "/data/query",
        auth: AUTH_SITE,
        summary: "Batched reads. Records for a query named parts are at data.parts.records.",
    },
    Route {
        method: "GET",
        path: "/actions",
        auth: AUTH_SITE,
        summary: "List actions. This path is a route. It is not the prefix catalog.",
    },
    Route {
        method: "GET",
        path: "/actions/{name}",
        auth: AUTH_SITE,
        summary: "One action and its params.",
    },
    Route {
        method: "POST",
        path: "/actions/{name}",
        auth: AUTH_SITE,
        summary: "Run an action. Body {input}. A new app has no handler, so this is 501. Use /data.",
    },
];

const LLMS_BODY: &str = "\
Loco HTTP API.\n\
\n\
Read /.well-known/loco.json and follow it. That JSON document is the\n\
interface: how to sign up, log in, create a project, declare a collection,\n\
write records, upload a bundle, and pin a site. Do not guess paths.\n\
";

/// The example page an agent saves as `index.html` and uploads. The Hurl
/// suite zips this same file, so the bytes a visitor is served are the
/// bytes this document tells the agent to write.
const INDEX_HTML: &str = include_str!("../tests/suites/discovery/index.html");

/// `Some` when `path` is a reserved API prefix or a path under one.
/// `/database` and `/action` are not reserved.
pub fn reserved_prefix(path: &str) -> Option<&'static str> {
    RESERVED_PREFIXES.iter().copied().find(|prefix| {
        path == *prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn is_prefix_root(path: &str) -> bool {
    let trimmed = path.trim_end_matches('/');
    RESERVED_PREFIXES.contains(&trimmed)
}

fn in_prefix(route: &Route, prefix: &str) -> bool {
    route.path == prefix || route.path.starts_with(&format!("{prefix}/"))
}

fn prefix_catalog(prefix: &str) -> String {
    let listed = ROUTES
        .iter()
        .filter(|route| in_prefix(route, prefix))
        .map(|route| format!("{} {}", route.method, route.path))
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
    json!({
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

fn route_json(route: &Route) -> Value {
    json!({
        "method": route.method,
        "path": route.path,
        "auth": route.auth,
        "summary": route.summary,
    })
}

fn prefixes() -> Value {
    Value::Array(
        RESERVED_PREFIXES
            .iter()
            .map(|prefix| {
                let routes = ROUTES
                    .iter()
                    .filter(|route| in_prefix(route, prefix))
                    .map(route_json)
                    .collect::<Vec<_>>();
                json!({
                    "prefix": prefix,
                    "summary": prefix_summary(prefix),
                    "routes": routes,
                })
            })
            .collect(),
    )
}

fn prefix_summary(prefix: &str) -> &'static str {
    match prefix {
        "/auth" => "Sign up, log in, the signed-in user, and API keys. No site headers.",
        "/config" => "Projects, datasets, sites, and versions. Bearer token. No site headers.",
        "/schema" => "Draft metadata: manifest, collections, fields, permission sets, and the bundle. Bearer token. Path includes account, project, and version.",
        "/data" => "Records. Site headers, or the site's own host. Public may call a verb the pinned version grants.",
        "/actions" => "Declared actions. GET /actions is this list. A new app's records go through /data, because a declared action with no handler is 501.",
        _ => "",
    }
}

fn json_call(method: &str, path: &str, auth: &str, body: Value, success: &str) -> Value {
    json!({
        "method": method,
        "path": path,
        "auth": auth,
        "headers": {"Content-Type": "application/json"},
        "json": body,
        "success": success,
    })
}

fn get_call(path: &str, auth: &str, success: &str) -> Value {
    json!({
        "method": "GET",
        "path": path,
        "auth": auth,
        "success": success,
    })
}

fn document_value() -> Value {
    json!({
        "name": "loco",
        "version": env!("CARGO_PKG_VERSION"),
        "summary": "Loco is an HTTP API. You declare a collection, store records in it, upload a static page, and open that page on a site URL. This JSON is the whole interface. Follow guide from n=1. Do not invent paths.",
        "read_this": "Read guide in order. A path containing {account} or another {name} is a pattern: substitute your handle, project, version, collection, and ids. The do text of each step gives the concrete URLs for the parts example. There is no OpenAPI document, no /docs page, and no loco command on this server. docs is empty. Fieldsets, secrets, variables, integrations, and declared actions exist and are not required to publish a list-and-edit page. When a JSON error includes see, GET that path and read it again. Send Content-Type: application/json on every JSON body. CORS allows every origin, method, and header. Responses are JSON.",
        "docs": [],
        "discovery": DISCOVERY_PATH,
        "envelope": {
            "shape": "A success is {\"ok\": true, \"data\": ...}. An error is {\"ok\": false, \"error\": \"...\"}. A rejected record or action input is 400, error \"validation failed\", and diagnostics is a list of {severity, kind, path, message}. Read message. A required string that is missing, null, or \"\" fails. An integer is a JSON number, including 0, not a string.",
            "auth_header": "Authorization: Bearer <token>",
            "auth_header_note": "One space after Bearer. No Authorization header means the caller is public. A header that is present but malformed, unknown, expired, or revoked is 401 and is never treated as public. Missing: \"missing auth: use Authorization: Bearer <token> header\". Malformed: \"malformed Authorization header: use Bearer <token>\". Bad or expired: \"invalid or expired session\". 403 means \"you do not have access to this resource\".",
            "cors": "*"
        },
        "auth_values": {
            "none": "No token. Signup, login, this document, and /llms.txt.",
            "bearer": "Authorization: Bearer <token>. Auth routes other than signup and login, plus /config and /schema.",
            "site": "X-Project-Id: {account}/{project} and X-Site-Id: {site}, unless Host is the site URL, which fills those headers in. A header that names a different site than the URL is 400. A bearer token is required for a member when public has no grant. Token-less public may call a /data verb only when the pinned version's manifest assigns a permission set that grants it. /actions is never public."
        },
        "signup": signup(),
        "login": {
            "method": "POST",
            "path": "/auth/login",
            "content_type": "application/json",
            "body": {"username": "lego_reseller", "password": "the password from signup"},
            "success": "200. data.token is the session. data.user.username is the handle. data.user.id is the same UUID as signup. Use the token as Authorization: Bearer <token>.",
            "failure": "401 {\"ok\": false, \"error\": \"invalid credentials\"} for a wrong password or an unknown handle. Login does not create an account. An empty password on login is that same 401, not \"password is required\".",
            "session": "A session lasts 7 days. There is no refresh. Log in again. POST /auth/logout with the bearer token ends it.",
            "api_key": "Optional. POST /auth/api-keys with {\"label\": \"agent\"} returns data.key once and data.id. GET /auth/api-keys/list does not show the secret. DELETE /auth/api-keys/{id} revokes it, using data.id. A key does not expire until it is revoked. Send it as the same Bearer token. Org accounts cannot log in and are not needed for a first app."
        },
        "names": {
            "handle": "The username you sign up with. Non-empty, not the reserved word public, no '/', only a-z, 0-9, and _, starting with a letter or _. There is no length cap. lego_reseller is legal. lego-reseller, Lego, and 1lego are not. The display name may contain spaces and capitals. The project account is the handle, not the user UUID.",
            "project_dataset_site": "1-63 characters of a-z, 0-9, and _, starting with a letter or _. A bad name is 400 and the error quotes this rule. inventory, dev, and www are legal.",
            "version": "1-63 characters of a-z, 0-9, '.', '_', and '-', not starting with '.'. No '@'. A name that ends in -dev is a draft and accepts /schema writes. Any other name is published and refuses /schema writes. 0.0.1-dev and 0.0.1 are the names this guide uses.",
            "collection": "One or more of a-z, 0-9, '_', '.', and '-'. '$' is reserved and is rejected. parts is legal.",
            "field_and_permission_set": "Use lowercase letters and underscores: sku, qty, parts_public."
        },
        "site": {
            "url": "http://{site}.{project}.{account}.{listen-host}/",
            "example": "http://dev.inventory.lego_reseller.localhost:3000/",
            "note": "Replace the port with the port of this server. A name ending in .localhost resolves to this machine. 127.0.0.1 is not a site host: curling an IP serves the API only, unless you also send Host: dev.inventory.lego_reseller.localhost. The host with no site labels is the API only. After you publish, http://127.0.0.1:{port}/ is still JSON. Open the three-label host. Hosted files are public. Do not put a bearer token in the page."
        },
        "guide": guide(),
        "prefixes": prefixes(),
        "routes": ROUTES.iter().map(route_json).collect::<Vec<_>>(),
    })
}

fn signup() -> Value {
    json!({
        "method": "POST",
        "path": "/auth/users",
        "content_type": "application/json",
        "body": {
            "username": "lego_reseller",
            "name": "Lego Reseller",
            "password": "any-non-empty"
        },
        "handle": "username is the handle. It must be non-empty, not the reserved word public, contain no '/', and use only a-z, 0-9, and _, starting with a letter or _. lego_reseller is legal. lego-reseller, Lego, and 1lego are not. name may contain spaces and capitals. There is no length cap on a handle. There is no password complexity rule. A hyphen in the password is fine.",
        "rejected_handle": "401 {\"ok\": false, \"error\": \"invalid credentials\"}. On signup this means the handle was rejected, including the reserved word public. It does not mean the password was wrong. Change the handle and retry. The same error text on POST /auth/login means the handle and password did not match.",
        "missing_field": "A body missing username or name is 422 text/plain, not the JSON envelope, and the text names the missing field. Send both. A missing, empty, or whitespace-only password is 400 {\"ok\": false, \"error\": \"password is required\"}.",
        "taken": "409 {\"ok\": false, \"error\": \"user already exists\"}. Pick a different handle.",
        "success": "201. data.username is the handle. data.id is a UUID. Use that UUID only for PUT and DELETE /auth/users/{id}. There is no token in this response. Call POST /auth/login next. The project account is data.username, not data.id."
    })
}

fn guide() -> Value {
    json!([
        {
            "n": 1,
            "title": "Read this document",
            "do": "GET this document and follow the steps below in order. Do not guess a path. If you are unsure, look at routes. A 404 whose error is \"no such endpoint: {path}\" means that path is not mounted; read see and try the route this document names instead.",
            "requests": [
                get_call(DISCOVERY_PATH, AUTH_NONE, "200 JSON. name is loco. version is a string. docs is an empty array."),
                get_call(LLMS_PATH, AUTH_NONE, "200 text/plain. The body contains /.well-known/loco.json.")
            ]
        },
        {
            "n": 2,
            "title": "Create an account",
            "do": "POST /auth/users. username is the handle (see signup.handle). name is a display name. password is any non-empty string. A 401 invalid credentials on this call means the handle was rejected: change username and retry. It does not mean the password was wrong. A 422 whose body is plain text and names username or name means that field was missing. A 400 password is required means the password was missing or blank. A 409 user already exists means the handle is taken. Success is 201 and does not include a token. Remember data.username (the account) and data.id (only for later user update or delete).",
            "requests": [
                json_call(
                    "POST",
                    "/auth/users",
                    AUTH_NONE,
                    json!({"username": "lego_reseller", "name": "Lego Reseller", "password": "any-non-empty"}),
                    "201. data.username is lego_reseller. data.id is a UUID. No token."
                )
            ]
        },
        {
            "n": 3,
            "title": "Log in",
            "do": "POST /auth/login with the same username and password. Save data.token. From here, send Authorization: Bearer <token> on /config and /schema calls. A session lasts 7 days. There is no refresh. When you are finished, POST /auth/logout with the token. An API key is optional and is not required for this app: POST /auth/api-keys returns data.key once, and that string is sent the same way as the session token. Do not log out between these steps. Login of an unknown user does not create an account.",
            "requests": [
                json_call(
                    "POST",
                    "/auth/login",
                    AUTH_NONE,
                    json!({"username": "lego_reseller", "password": "any-non-empty"}),
                    "200. data.token is the secret to send. data.user.username is the handle."
                )
            ]
        },
        {
            "n": 4,
            "title": "Create a project",
            "do": "POST /config/project with the bearer token. Omit account: the project is created under your handle. name is a slug such as inventory (see names.project_dataset_site). The response is the project. The id you will use is {handle}/{name}, for example lego_reseller/inventory. The server also creates version 0.0.1-dev, dataset dev, and site dev. Site dev pins that version and that dataset. 0.0.1-dev ends in -dev, so it is a draft and accepts /schema writes. Confirm the pin with GET /config/site/lego_reseller/inventory/dev: data.version is 0.0.1-dev and data.dataset is dev. You are the developer of every project under your handle.",
            "requests": [
                json_call(
                    "POST",
                    "/config/project",
                    AUTH_BEARER,
                    json!({"name": "inventory", "label": "Inventory", "description": "A parts list"}),
                    "201. The project id is lego_reseller/inventory."
                ),
                get_call(
                    "/config/site/{account}/{project}/{name}",
                    AUTH_BEARER,
                    "200 for /config/site/lego_reseller/inventory/dev. data.version is 0.0.1-dev. data.dataset is dev."
                )
            ]
        },
        {
            "n": 5,
            "title": "Declare the collection and its fields",
            "do": "Write schema on the draft: /schema/lego_reseller/inventory/0.0.1-dev/.... Create the collection first, then its fields. A collection name is one or more of a-z, 0-9, '_', '.', and '-'. parts is legal. A field type is one of string, integer, float, boolean. Anything else is 400 and the error says unknown field type. required true means a create must send a value: missing, null, or \"\" for a string is an error, and the message looks like field 'qty' is required. An update checks a required field only when the patch names it. options, a list of {value, label}, is only for a string field; omit it to allow any string. You do not manage fieldsets. The server creates the default one.",
            "requests": [
                json_call(
                    "POST",
                    "/schema/{account}/{project}/{version}/collection",
                    AUTH_BEARER,
                    json!({"name": "parts", "label": "Part", "label_plural": "Parts"}),
                    "201. data.name is parts."
                ),
                json_call(
                    "POST",
                    "/schema/{account}/{project}/{version}/field",
                    AUTH_BEARER,
                    json!({"collection": "parts", "name": "sku", "type": "string", "label": "SKU", "required": true}),
                    "201. data.name is sku. data.type is string."
                ),
                json_call(
                    "POST",
                    "/schema/{account}/{project}/{version}/field",
                    AUTH_BEARER,
                    json!({"collection": "parts", "name": "qty", "type": "integer", "label": "Quantity", "required": true}),
                    "201. data.name is qty. data.type is integer."
                )
            ]
        },
        {
            "n": 6,
            "title": "Let visitors read and edit",
            "do": "A hosted page sends no token. Public can list, add, and update only if you grant those verbs and assign the set on the manifest. Create a permission set whose collections entry names the bare collection parts (this project's own collection, not account/project@version). Set read, create, and update to true. A verb you omit is false. Leave delete false. delete true means anyone who can open the site can delete every row. Then PUT the manifest with public_permission_sets set to that set's name. Policy lives on the version, so every site that pins this version shares it. PUT of the permission set later replaces the collections list; send the whole list you want to keep.",
            "requests": [
                json_call(
                    "POST",
                    "/schema/{account}/{project}/{version}/permission_set",
                    AUTH_BEARER,
                    json!({
                        "name": "parts_public",
                        "label": "Public parts",
                        "collections": [
                            {"collection": "parts", "read": true, "create": true, "update": true, "delete": false}
                        ]
                    }),
                    "201. data.name is parts_public. data.collections[0].delete is false."
                ),
                json_call(
                    "PUT",
                    "/schema/{account}/{project}/{version}/permission_set/{name}",
                    AUTH_BEARER,
                    json!({
                        "label": "Public parts",
                        "collections": [
                            {"collection": "parts", "read": true, "create": true, "update": true, "delete": false}
                        ]
                    }),
                    "200 when you need to replace the grants. The path name is parts_public."
                ),
                json_call(
                    "PUT",
                    "/schema/{account}/{project}/{version}/manifest",
                    AUTH_BEARER,
                    json!({"public_permission_sets": ["parts_public"]}),
                    "200. data.public_permission_sets[0] is parts_public."
                )
            ]
        },
        {
            "n": 7,
            "title": "Write a record",
            "do": "Records are site-scoped. Send the bearer token plus X-Project-Id: lego_reseller/inventory and X-Site-Id: dev. The add body is the fields themselves: {\"sku\": \"3001\", \"qty\": 2}. qty is a number. The response record is {id, dataset_id, created_at, created_by, updated_at, updated_by, owner, fields}. Read data.fields.sku. data.id is the record id for get, update, and delete. List returns every row: data is an array, and each row has the same shape. Update is a partial patch: {\"qty\": 3} changes qty and leaves sku. Query is optional. POST /data/query with {\"queries\": {\"parts\": {\"collection\": \"parts\", \"limit\": 50}}} returns data.parts.records (the same record shape) and data.parts.cursor (null when there are no further rows). Limit defaults to 50 and cannot exceed 500. GET /data/parts/fields returns labels and options for a hosted page that does not know its version. An unknown field, a string where an integer belongs, or a string outside options is 400 validation failed plus diagnostics.",
            "requests": [
                json_call(
                    "POST",
                    "/data/{collection}/add",
                    AUTH_SITE,
                    json!({"sku": "3001", "qty": 2}),
                    "201 with X-Project-Id: lego_reseller/inventory and X-Site-Id: dev. data.fields.sku is 3001. data.fields.qty is 2. Save data.id."
                ),
                get_call(
                    "/data/{collection}/list",
                    AUTH_SITE,
                    "200. data is an array. data[0].fields.sku is the SKU. Same site headers."
                ),
                json_call(
                    "PUT",
                    "/data/{collection}/update/{id}",
                    AUTH_SITE,
                    json!({"qty": 3}),
                    "200. data.fields.qty is 3 and data.fields.sku is still 3001. {id} is the record id."
                ),
                json_call(
                    "POST",
                    "/data/query",
                    AUTH_SITE,
                    json!({"queries": {"parts": {"collection": "parts", "limit": 50}}}),
                    "200. data.parts.records is an array. data.parts.records[0].fields.sku is the SKU. data.parts.cursor is null when the page is the last one."
                ),
                get_call(
                    "/data/{collection}/fields",
                    AUTH_SITE,
                    "200. data is the field list for the pinned version, including label, type, required, and options."
                )
            ]
        },
        {
            "n": 8,
            "title": "Upload the page",
            "do": "Save index_html as a file named index.html. From the directory that contains that file, zip it so the archive entry is index.html and not dist/index.html. On a Mac or Linux shell: zip bundle.zip index.html. PUT the raw zip bytes to the draft bundle URL. Content-Type is application/zip. Do not send JSON and do not base64-encode the zip. Success is 200, not 201, and data is {hash, uploaded_at, size, files}. GET of the same path returns that metadata, not the files. A missing bundle is 404 and the error is no bundle for {project} at version {version}. The PUT is refused at 32 MiB of body, 64 MiB unpacked, 16 MiB in one file, or 2000 files, and when an entry contains .., is absolute, is a symlink, or when index.html is not at the zip root. Only a draft accepts the PUT. The page fetches /data/parts/list, POST /data/parts/add, and PUT /data/parts/update/{id} on the same origin, with no token and no site headers. It works only when a visitor loads it from the site URL in the next step, and only because step 6 granted public read, create, and update.",
            "index_html": INDEX_HTML,
            "requests": [
                {
                    "method": "PUT",
                    "path": "/schema/{account}/{project}/{version}/bundle",
                    "auth": AUTH_BEARER,
                    "headers": {"Content-Type": "application/zip"},
                    "body": "Raw zip bytes of index.html at the zip root.",
                    "success": "200 on /schema/lego_reseller/inventory/0.0.1-dev/bundle. data.files is at least 1. data.hash is a string."
                },
                get_call(
                    "/schema/{account}/{project}/{version}/bundle",
                    AUTH_BEARER,
                    "200. data.hash, data.uploaded_at, data.size, and data.files. Not the HTML."
                )
            ]
        },
        {
            "n": 9,
            "title": "Open the site",
            "do": "Site dev already pins 0.0.1-dev and dataset dev, so there is nothing to re-pin. Open http://dev.inventory.lego_reseller.localhost:{port}/ where {port} is this server's port. The page heading is discovery page. The list is loaded by the browser with no token. 127.0.0.1 and the bare hostname are not that site. If you curl an IP, send Host: dev.inventory.lego_reseller.localhost and you will get the HTML. A curl of the site host to GET /data/parts/list, with only that Host and no token, is 200 and data is the array of rows. GET /.well-known/loco.json on that same host is still this JSON, not the HTML.",
            "requests": []
        },
        {
            "n": 10,
            "title": "Publish",
            "do": "POST /config/version/lego_reseller/inventory with {\"version\": \"0.0.1\", \"from\": \"0.0.1-dev\"}. That copies the schema and the bundle into 0.0.1. It does not copy records, datasets, sites, or secret values. The public grants are part of the copied manifest, so they come along. 0.0.1 does not end in -dev, so it is published: further /schema writes to it are refused, and its hashed assets are cached as immutable while index.html revalidates. Then create site www with {\"name\": \"www\", \"label\": \"Public\", \"version\": \"0.0.1\", \"dataset\": \"dev\"}. Reusing dataset dev keeps the rows you already wrote. Open http://www.inventory.lego_reseller.localhost:{port}/. To change the published site, edit 0.0.1-dev (or a new draft), copy to a new version name such as 0.0.2, and PUT /config/site/lego_reseller/inventory/www with {\"version\": \"0.0.2\", \"dataset\": \"dev\"}. Copying onto a version that already exists is 409 and the error starts with already exists. Publish the next snapshot under a new name.",
            "requests": [
                json_call(
                    "POST",
                    "/config/version/{account}/{project}",
                    AUTH_BEARER,
                    json!({"version": "0.0.1", "from": "0.0.1-dev"}),
                    "201. data.version is 0.0.1. The bundle and the public grants were copied. The records were not."
                ),
                json_call(
                    "POST",
                    "/config/site/{account}/{project}",
                    AUTH_BEARER,
                    json!({"name": "www", "label": "Public", "version": "0.0.1", "dataset": "dev"}),
                    "201. Open http://www.inventory.lego_reseller.localhost:{port}/."
                )
            ]
        },
        {
            "n": 11,
            "title": "When a path is wrong",
            "do": "GET /auth, GET /schema, GET /data, or GET /config is 404. The error names that prefix's routes and does not say no such endpoint. see is /.well-known/loco.json. GET /actions is not that catalog: it is the action list, and without site headers it is 400 missing project: use X-Project-Id header. An unknown path under a prefix, such as GET /schema/foo, is 404 and the error is \"no such endpoint: /schema/foo\", with see set to this document. The same pointer is on a 404 from a host that names no site, including GET / and GET /help. A missing file inside a site's bundle does not set see. This server registers no action handlers. POST /actions/{name} on an app you just created is 501. Keep the list-and-edit UI on /data. Do not look for a CLI.",
            "requests": [
                get_call(
                    "/actions",
                    AUTH_SITE,
                    "400 missing project on a host that is not a site, or the action list when the site headers are present. Not a prefix catalog."
                )
            ]
        }
    ])
}

pub async fn document() -> Response {
    let bytes =
        serde_json::to_vec_pretty(&document_value()).expect("discovery document serializes");
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
    fn prefix_roots_name_routes_and_other_misses_keep_the_pointer() {
        for prefix in RESERVED_PREFIXES {
            let body = api_miss_body(prefix);
            let error = body["error"].as_str().unwrap();
            assert!(!error.contains("no such endpoint"), "{prefix}: {error}");
            assert_eq!(body["see"], DISCOVERY_PATH);
            assert!(is_prefix_root(&format!("{prefix}/")));
            let mut count = 0;
            for route in ROUTES {
                if in_prefix(route, prefix) {
                    count += 1;
                    let listed = format!("{} {}", route.method, route.path);
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
        let doc = document_value();
        assert_eq!(doc["name"], "loco");
        assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(doc["discovery"], DISCOVERY_PATH);
        assert_eq!(doc["docs"].as_array().unwrap().len(), 0);
        let handle = doc["signup"]["handle"].as_str().unwrap();
        assert!(handle.contains("a-z"), "{handle}");
        assert!(handle.contains("lego_reseller"), "{handle}");
        let rejected = doc["signup"]["rejected_handle"].as_str().unwrap();
        assert!(rejected.contains("invalid credentials"), "{rejected}");
        assert!(rejected.contains("401"), "{rejected}");
        let missing = doc["signup"]["missing_field"].as_str().unwrap();
        assert!(missing.contains("422"), "{missing}");
        assert!(missing.contains("password is required"), "{missing}");
        let routes = doc["routes"].as_array().unwrap();
        assert_eq!(routes.len(), ROUTES.len());
        for (got, expect) in routes.iter().zip(ROUTES.iter()) {
            assert_eq!(got["method"], expect.method);
            assert_eq!(got["path"], expect.path);
            assert_eq!(got["auth"], expect.auth);
            assert_eq!(got["summary"], expect.summary);
        }
    }

    #[test]
    fn guide_requests_are_listed_routes() {
        let doc = document_value();
        let guide = doc["guide"].as_array().unwrap();
        assert!(!guide.is_empty());
        let mut saw_page = false;
        for step in guide {
            if step.get("index_html").and_then(Value::as_str).is_some() {
                saw_page = true;
            }
            for request in step["requests"].as_array().unwrap() {
                let method = request["method"].as_str().unwrap();
                let path = request["path"].as_str().unwrap();
                assert!(
                    ROUTES
                        .iter()
                        .any(|route| route.method == method && route.path == path),
                    "guide request {method} {path} is not in ROUTES"
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
        let count_line = format!("jsonpath \"$.routes\" count == {}", ROUTES.len());
        assert!(
            HURL.lines().any(|line| line == count_line),
            "discovery.hurl is missing {count_line}"
        );
        for route in ROUTES {
            let line = format!(
                "{} http://127.0.0.1:{{{{port}}}}{}",
                route.method,
                probe_path(route.path)
            );
            assert!(
                HURL.lines().any(|candidate| candidate == line),
                "discovery.hurl is missing {line}"
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

        for route in ROUTES {
            let url = format!("http://127.0.0.1:{port}{}", probe_path(route.path));
            let method = reqwest::Method::from_bytes(route.method.as_bytes()).unwrap();
            let response = client
                .request(method, &url)
                .send()
                .await
                .unwrap_or_else(|err| panic!("{} {}: {err}", route.method, url));
            let status = response.status();
            let body = response.text().await.unwrap();
            if route.path == DISCOVERY_PATH || route.path == LLMS_PATH {
                assert_eq!(
                    status, 200,
                    "{} {} -> {status} {body}",
                    route.method, route.path
                );
            } else {
                assert!(
                    !body.contains("no such endpoint"),
                    "{} {} fell through ({status}): {body}",
                    route.method,
                    route.path
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
