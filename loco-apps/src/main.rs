use loco_apps::config::{absolute_path, Config};
use loco_apps::server::{build_app, Extensions};

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap_or_else(|err| panic!("{err}"));
    println!("Root: {}", config.root.display());
    // Printed for every adapter, including memory. With LOCO_ROOT unset the
    // path stays relative, so this absolute form is only the log line.
    println!(
        "SQLite database: {}",
        absolute_path(&config.database_path).display()
    );

    let port = config.port;
    let app = build_app(&config, Extensions::default());

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("Listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
