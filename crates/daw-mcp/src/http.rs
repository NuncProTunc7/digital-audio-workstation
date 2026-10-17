//! Web mode (`npt-mcp --http`): the same bridge over MCP's Streamable HTTP,
//! for AI apps on this computer that take a web address instead of a
//! program to launch.
//!
//! Safety: it listens on 127.0.0.1 only, every request needs the secret key
//! (from `NPT_MCP_KEY`, as `Authorization: Bearer <key>` or `?key=<key>`),
//! and requests from web pages other than local ones are refused, so a
//! website open in the browser can't drive the app.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tower_service::Service;

use crate::Bridge;

/// Where the endpoint lives.
pub const PATH: &str = "/mcp";
/// Environment variable holding the secret key.
pub const KEY_VAR: &str = "NPT_MCP_KEY";
/// Shortest key accepted, so a guessable one isn't used by mistake.
const MIN_KEY_CHARS: usize = 16;

type Body = BoxBody<Bytes, Infallible>;

fn plain(status: StatusCode, text: &str) -> Response<Body> {
    let mut r = Response::new(Full::new(Bytes::from(text.to_owned())).boxed());
    *r.status_mut() = status;
    r
}

/// Compares without stopping at the first difference, so timing doesn't
/// leak the key.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Whether the request carries the key, in a header or the address.
fn has_key<B>(req: &Request<B>, key: &str) -> bool {
    let header = req
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|k| same(k.trim(), key));
    let query = req.uri().query().is_some_and(|q| {
        q.split('&')
            .filter_map(|pair| pair.strip_prefix("key="))
            .any(|k| same(k, key))
    });
    header || query
}

/// A web page's address as sent by browsers: only local pages may connect.
fn origin_allowed<B>(req: &Request<B>) -> bool {
    let Some(origin) = req
        .headers()
        .get(hyper::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    else {
        return true; // Not a browser.
    };
    let host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Reads the key, binds 127.0.0.1:`port` (0 = any free port), prints the
/// address on the first line of stdout, and serves until killed.
pub async fn serve(core: bool, port: u16) -> Result<(), String> {
    let key = std::env::var(KEY_VAR).unwrap_or_default();
    if key.trim().chars().count() < MIN_KEY_CHARS {
        return Err(format!(
            "web mode needs a secret key of at least {MIN_KEY_CHARS} characters in {KEY_VAR}"
        ));
    }
    let key: Arc<str> = key.trim().into();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("could not listen on port {port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    // The app reads this line to learn the port.
    println!("npt-mcp: listening on http://127.0.0.1:{port}{PATH}");

    let mut config = StreamableHttpServerConfig::default();
    config.allowed_origins = vec![
        "http://localhost:*".into(),
        "http://127.0.0.1:*".into(),
        "https://localhost:*".into(),
    ];
    let mcp = StreamableHttpService::new(
        move || Ok(Bridge::new(core)),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mcp = mcp.clone();
        let key = Arc::clone(&key);
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let mut mcp = mcp.clone();
                let key = Arc::clone(&key);
                async move {
                    if req.uri().path() != PATH {
                        return Ok::<_, Infallible>(plain(StatusCode::NOT_FOUND, "not found"));
                    }
                    if !origin_allowed(&req) {
                        return Ok(plain(
                            StatusCode::FORBIDDEN,
                            "web pages other than local ones can't connect",
                        ));
                    }
                    if !has_key(&req, &key) {
                        return Ok(plain(
                            StatusCode::UNAUTHORIZED,
                            "the secret key is missing or wrong",
                        ));
                    }
                    mcp.call(req).await
                }
            });
            let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(uri: &str, headers: &[(&str, &str)]) -> Request<()> {
        let mut b = Request::builder().uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(()).expect("request")
    }

    #[test]
    fn the_key_can_come_in_a_header_or_the_address() {
        let key = "abcdefghijklmnop";
        assert!(has_key(
            &req("/mcp", &[("Authorization", "Bearer abcdefghijklmnop")]),
            key
        ));
        assert!(has_key(&req("/mcp?x=1&key=abcdefghijklmnop", &[]), key));
        assert!(!has_key(&req("/mcp?key=abc", &[]), key));
        assert!(!has_key(&req("/mcp", &[]), key));
    }

    #[test]
    fn only_local_pages_pass() {
        assert!(origin_allowed(&req("/mcp", &[])));
        assert!(origin_allowed(&req(
            "/mcp",
            &[("Origin", "http://localhost:3000")]
        )));
        assert!(origin_allowed(&req(
            "/mcp",
            &[("Origin", "http://127.0.0.1:8080")]
        )));
        assert!(!origin_allowed(&req(
            "/mcp",
            &[("Origin", "https://example.com")]
        )));
        assert!(!origin_allowed(&req(
            "/mcp",
            &[("Origin", "http://localhost.evil.com")]
        )));
        assert!(!origin_allowed(&req("/mcp", &[("Origin", "null")])));
    }
}
