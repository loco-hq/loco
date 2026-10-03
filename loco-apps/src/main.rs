use loco_apps::server::{
    absolute_path, build_app_with_options, default_data_root, parse_loco_root, resolve_port,
    resolve_sqlite_path, AppOptions,
};

#[tokio::main]
async fn main() {
    let configured_root = parse_loco_root(std::env::var("LOCO_ROOT").ok().as_deref());
    let root = match &configured_root {
        Some(path) => absolute_path(path),
        None => default_data_root().to_path_buf(),
    };
    // Join the database onto the root only when LOCO_ROOT is set. Unset, the
    // path stays relative to the working directory, which is where the dev
    // database already lives.
    let sqlite_root = if configured_root.is_some() {
        Some(root.as_path())
    } else {
        None
    };
    let sqlite_path =
        resolve_sqlite_path(sqlite_root, std::env::var("LOCO_DB_PATH").ok().as_deref());
    println!("Root: {}", root.display());
    println!("SQLite database: {}", absolute_path(&sqlite_path).display());

    // A fresh LOCO_ROOT has no parent for a nested relative database path.
    // Seed creates the root itself; this covers `LOCO_DB_PATH=data/app.db`.
    // Unset, nothing is created: `./loco.db` opens in the working directory.
    let sqlite_adapter = std::env::var("LOCO_ADAPTER").unwrap_or_else(|_| "sqlite".to_string());
    if configured_root.is_some() && sqlite_adapter == "sqlite" {
        if let Some(parent) = sqlite_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).unwrap_or_else(|err| {
                panic!(
                    "failed to create the directory for the SQLite database {}: {err}",
                    parent.display()
                );
            });
        }
    }

    let port =
        resolve_port(std::env::var("PORT").ok().as_deref()).unwrap_or_else(|err| panic!("{err}"));
    let app = build_app_with_options(
        &root,
        AppOptions {
            sqlite_path: Some(sqlite_path),
            ..AppOptions::default()
        },
    );

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("Listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
