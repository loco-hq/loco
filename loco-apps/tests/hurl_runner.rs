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
fn run_suite(suite_dir: &Path) {
    run_suite_with(suite_dir, AppOptions::default());
}

/// As `run_suite`, with app options pinned instead of read from the
/// environment. Returns the server root so a caller can assert on what the
/// suite did (or did not) write to disk.
fn run_suite_with(suite_dir: &Path, options: AppOptions) -> tempfile::TempDir {
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

    // 2. Use in-memory adapter (no SQLite needed for tests). Set once across all suites.
    ADAPTER_ENV_ONCE.call_once(|| unsafe {
        std::env::set_var("LOCO_ADAPTER", "memory");
        std::env::set_var("LOCO_AUTH_AUTO_CREATE", "1");
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
    let output = Command::new("hurl")
        .arg("--test")
        .arg("--jobs")
        .arg("1")
        .arg("--variable")
        .arg(format!("port={port}"))
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
fn suite_data_crud() {
    run_suite(&suites_dir().join("data_crud"));
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
fn suite_project_lifecycle() {
    run_suite(&suites_dir().join("project_lifecycle"));
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
fn suite_version_lifecycle() {
    run_suite(&suites_dir().join("version_lifecycle"));
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
        },
    );
    for dir in ["accounts", "identities"] {
        assert!(
            !tmp.path().join("auth").join(dir).join("acme.json").exists(),
            "login of an unknown handle wrote auth/{dir}/acme.json"
        );
    }
}
