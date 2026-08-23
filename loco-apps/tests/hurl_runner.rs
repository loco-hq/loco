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
    ADAPTER_ENV_ONCE.call_once(|| {
        unsafe {
            std::env::set_var("LOCO_ADAPTER", "memory");
            std::env::set_var("LOCO_AUTH_AUTO_CREATE", "1");
        }
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
