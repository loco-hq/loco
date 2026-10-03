//! `HTTP_PROXY` is process-wide, and libc `getenv` does not take the env lock.
//! This binary has one test so no other test in the process calls `getenv`
//! (for example `getaddrinfo` on `localhost`) while these variables are set.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use loco_apps::actions::http_client;

/// The client is built with [`http_client`], so a proxy hit means
/// `.no_proxy()` was dropped. The variables stay set for the whole request:
/// reqwest reads them when it sends, not when the client is built.
#[tokio::test]
async fn http_client_ignores_http_proxy() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_for_proxy = Arc::clone(&hits);
    let proxy = spawn_server(move |_port, _path| {
        hits_for_proxy.fetch_add(1, Ordering::SeqCst);
        response(502, "Bad Gateway", &[], "proxied")
    });
    let dest = spawn_server(|_port, _path| response(200, "OK", &[], "direct"));
    let _guard = ProxyEnv::set(&format!("http://127.0.0.1:{proxy}"));
    let client = http_client();
    let response = client
        .get(format!("http://127.0.0.1:{dest}/direct"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "direct");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

struct ProxyEnv;

impl ProxyEnv {
    fn set(url: &str) -> Self {
        // SAFETY: this binary's only test sets these, and the guard removes
        // both on drop, including on panic. Nothing else in the process calls
        // `getenv` while they are set.
        unsafe {
            std::env::set_var("HTTP_PROXY", url);
            std::env::set_var("http_proxy", url);
        }
        Self
    }
}

impl Drop for ProxyEnv {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("http_proxy");
        }
    }
}

fn response(status: u16, reason: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    out
}

fn spawn_server(handler: impl Fn(u16, &str) -> String + Send + Sync + 'static) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = answer(stream, &|path| handler(port, path));
        }
    });
    port
}

fn answer(mut stream: TcpStream, handler: &impl Fn(&str) -> String) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
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
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
            Err(err) => return Err(err),
        }
    }
    let req = String::from_utf8_lossy(&buf);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    stream.write_all(handler(path).as_bytes())?;
    stream.flush()
}
