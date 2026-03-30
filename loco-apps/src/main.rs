include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));
pub mod auth;
mod server;

#[tokio::main]
async fn main() {
    let app = server::build_app();
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
