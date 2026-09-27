//! B2 of the 1.10.0 launch review: a pinned fetch must not go through a proxy.
//!
//! A forward proxy is handed the hostname and resolves it itself, so the
//! address the app vetted is not the one the request reaches. This sets a proxy
//! the way a user's environment would, then checks that `pinned_client`
//! connects straight to the pinned address, and that the builder the app used
//! before — the same, without `no_proxy()` — goes through the proxy instead,
//! which is what shows the test can tell the difference.
//!
//! A test binary of its own, with a single test: the proxy is set through the
//! process environment, which every thread shares.
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

fn recording_server(body: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, seen)
}

#[tokio::test]
async fn a_pinned_fetch_ignores_a_configured_proxy() {
    let (target, target_seen) = recording_server("DIRECT");
    let (proxy, proxy_seen) = recording_server("PROXY");
    for var in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        std::env::set_var(var, format!("http://127.0.0.1:{proxy}"));
    }
    for var in ["NO_PROXY", "no_proxy"] {
        std::env::remove_var(var);
    }
    let pinned: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    let url = format!("http://example.test:{target}/pinned");

    // Control: the old construction, without no_proxy(), goes via the proxy.
    let old = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve("example.test", std::net::SocketAddr::new(pinned, 0))
        .build()
        .unwrap();
    let body = old.get(&url).send().await.unwrap().text().await.unwrap();
    assert_eq!(
        body, "PROXY",
        "the control did not use the proxy, so this test cannot see a bypass"
    );
    assert!(!proxy_seen.lock().unwrap().is_empty());

    // pinned_client connects to the pinned address and the proxy sees nothing new.
    let before = proxy_seen.lock().unwrap().len();
    let client =
        scale_lib::pinned_client("example.test", pinned, std::time::Duration::from_secs(10))
            .unwrap();
    let body = client.get(&url).send().await.unwrap().text().await.unwrap();
    assert_eq!(body, "DIRECT");
    assert_eq!(
        proxy_seen.lock().unwrap().len(),
        before,
        "the proxy was used"
    );
    assert!(target_seen
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains("GET /pinned")));
}
