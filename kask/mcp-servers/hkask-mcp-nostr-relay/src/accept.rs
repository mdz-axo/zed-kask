//! The relay's own HTTP accept layer.
//!
//! The SDK's built-in listener accepts a websocket upgrade from any web page
//! (measured: a foreign `Origin` got `101`), and has no option to refuse one.
//! So this server never calls `LocalRelay::run()`. It binds its own loopback
//! listener, decides each request here, and hands accepted upgrades to
//! `LocalRelay::take_connection`.
//!
//! What this layer does not do: identify the caller. Any local process that
//! sends no `Origin` header is accepted. That limit is stated in the NIP-11
//! document and in `nostr_relay_status`.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;

use base64::Engine as _;
use hyper::body::Incoming;
use hyper::header::{
    ACCEPT, CONNECTION, CONTENT_TYPE, HOST, HeaderValue, ORIGIN, SEC_WEBSOCKET_ACCEPT, UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::Service;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use nostr_sdk::prelude::LocalRelay;
use sha1::{Digest, Sha1};
use tokio::net::TcpListener;

/// RFC 6455 §1.3: the fixed GUID appended to the client's key.
const WEBSOCKET_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// What to do with one HTTP request, decided from its headers alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The request carries an `Origin` header: it came from a web page.
    RefuseOrigin,
    /// The `Host` header is not this listener's loopback address.
    RefuseHost,
    /// A websocket upgrade with no `Sec-WebSocket-Key`.
    BadUpgrade,
    /// A websocket upgrade to hand to the relay, with its accept key.
    Upgrade { accept_key: String },
    /// A NIP-11 relay information request.
    RelayInfo,
    /// Anything else.
    NotFound,
}

fn header<'a>(headers: &'a HeaderMap, name: &hyper::header::HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The accept key a server returns for a client's `Sec-WebSocket-Key`.
pub fn accept_key(request_key: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(request_key);
    hasher.update(WEBSOCKET_GUID);
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

/// Decide one request. Pure, so every rule is tested without a socket.
///
/// Order matters: `Origin` is refused before anything else is read, so a web
/// page gets the same answer whatever else it sends.
pub fn decide(headers: &HeaderMap, port: u16) -> Decision {
    // Any `Origin` at all, including `null` and this relay's own address.
    // Browsers attach it to every cross-site websocket and fetch; native
    // Nostr clients do not send it.
    if headers.contains_key(ORIGIN) {
        return Decision::RefuseOrigin;
    }

    // A page that reaches this port through DNS rebinding can send a plain
    // GET with no `Origin`, but its `Host` is the attacker's name.
    let host_ok = header(headers, &HOST).is_some_and(|host| {
        host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
    });
    if !host_ok {
        return Decision::RefuseHost;
    }

    let wants_upgrade = header(headers, &CONNECTION)
        .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
        && header(headers, &UPGRADE).is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if wants_upgrade {
        return match headers.get("sec-websocket-key") {
            Some(key) => Decision::Upgrade {
                accept_key: accept_key(key.as_bytes()),
            },
            None => Decision::BadUpgrade,
        };
    }

    if header(headers, &ACCEPT).is_some_and(|value| value.contains("application/nostr+json")) {
        return Decision::RelayInfo;
    }

    Decision::NotFound
}

fn text(status: StatusCode, body: &str) -> Response<String> {
    let mut response = Response::new(body.to_string());
    *response.status_mut() = status;
    response
}

struct Front {
    relay: LocalRelay,
    remote: SocketAddr,
    port: u16,
    relay_info: String,
}

impl Service<Request<Incoming>> for Front {
    type Response = Response<String>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        // No response from this layer carries Access-Control-Allow-Origin.
        let response = match decide(request.headers(), self.port) {
            Decision::RefuseOrigin => text(StatusCode::FORBIDDEN, "origin not allowed"),
            Decision::RefuseHost => text(StatusCode::MISDIRECTED_REQUEST, "unexpected host"),
            Decision::BadUpgrade => text(StatusCode::BAD_REQUEST, "missing Sec-WebSocket-Key"),
            Decision::NotFound => text(StatusCode::NOT_FOUND, ""),
            Decision::RelayInfo => {
                let mut response = Response::new(self.relay_info.clone());
                response.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/nostr+json"),
                );
                response
            }
            Decision::Upgrade { accept_key } => match HeaderValue::from_str(&accept_key) {
                Ok(accept) => {
                    let relay = self.relay.clone();
                    let remote = self.remote;
                    tokio::spawn(async move {
                        match hyper::upgrade::on(request).await {
                            Ok(upgraded) => {
                                if let Err(error) =
                                    relay.take_connection(TokioIo::new(upgraded), remote).await
                                {
                                    tracing::debug!(%remote, %error, "relay connection ended with an error");
                                }
                            }
                            Err(error) => {
                                tracing::debug!(%remote, %error, "websocket upgrade failed");
                            }
                        }
                    });
                    let mut response = Response::new(String::new());
                    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
                    let headers = response.headers_mut();
                    headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
                    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
                    headers.insert(SEC_WEBSOCKET_ACCEPT, accept);
                    response
                }
                Err(_) => text(StatusCode::BAD_REQUEST, "invalid Sec-WebSocket-Key"),
            },
        };
        Box::pin(async move { Ok(response) })
    }
}

/// Bind the loopback listener. Port 0 asks the OS for a free port.
pub async fn bind(port: u16) -> std::io::Result<(TcpListener, SocketAddr)> {
    let listener =
        TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await?;
    let address = listener.local_addr()?;
    Ok((listener, address))
}

/// Serve connections until the listener fails. Each connection is one task.
pub async fn serve(listener: TcpListener, relay: LocalRelay, port: u16, relay_info: String) {
    loop {
        let (socket, remote) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::error!(%error, "relay listener stopped accepting connections");
                return;
            }
        };
        let front = Front {
            relay: relay.clone(),
            remote,
            port,
            relay_info: relay_info.clone(),
        };
        tokio::spawn(async move {
            if let Err(error) = http1::Builder::new()
                .serve_connection(TokioIo::new(socket), front)
                .with_upgrades()
                .await
            {
                tracing::debug!(%remote, %error, "relay HTTP connection ended with an error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 7447;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                hyper::header::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        map
    }

    fn upgrade(extra: &[(&str, &str)]) -> HeaderMap {
        let mut pairs = vec![
            ("host", "127.0.0.1:7447"),
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ];
        pairs.extend_from_slice(extra);
        headers(&pairs)
    }

    #[test]
    fn the_accept_key_matches_the_rfc_6455_example() {
        // RFC 6455 §1.3's worked example.
        assert_eq!(
            accept_key(b"dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn an_upgrade_without_origin_is_handed_to_the_relay() {
        assert_eq!(
            decide(&upgrade(&[]), PORT),
            Decision::Upgrade {
                accept_key: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".to_string()
            }
        );
    }

    #[test]
    fn any_origin_is_refused_whatever_its_value() {
        for origin in ["https://evil.example", "null", "http://127.0.0.1:7447", ""] {
            assert_eq!(
                decide(&upgrade(&[("origin", origin)]), PORT),
                Decision::RefuseOrigin,
                "origin {origin:?} must be refused"
            );
        }
    }

    #[test]
    fn origin_is_refused_on_a_relay_info_request_too() {
        let request = headers(&[
            ("host", "127.0.0.1:7447"),
            ("accept", "application/nostr+json"),
            ("origin", "https://evil.example"),
        ]);
        assert_eq!(decide(&request, PORT), Decision::RefuseOrigin);
    }

    #[test]
    fn a_host_that_is_not_this_listener_is_refused() {
        for host in [
            "attacker.example:7447",
            "127.0.0.1:9999",
            "127.0.0.1",
            "192.168.1.5:7447",
        ] {
            let mut request = upgrade(&[]);
            request.insert(HOST, HeaderValue::from_str(host).expect("host"));
            assert_eq!(
                decide(&request, PORT),
                Decision::RefuseHost,
                "host {host:?}"
            );
        }
        let mut no_host = upgrade(&[]);
        no_host.remove(HOST);
        assert_eq!(decide(&no_host, PORT), Decision::RefuseHost);
    }

    #[test]
    fn localhost_is_an_accepted_host_name() {
        let mut request = upgrade(&[]);
        request.insert(HOST, HeaderValue::from_static("localhost:7447"));
        assert!(matches!(decide(&request, PORT), Decision::Upgrade { .. }));
    }

    #[test]
    fn an_upgrade_without_a_key_is_a_bad_request() {
        let mut request = upgrade(&[]);
        request.remove("sec-websocket-key");
        assert_eq!(decide(&request, PORT), Decision::BadUpgrade);
    }

    #[test]
    fn a_relay_info_request_is_recognised_and_other_requests_are_not_found() {
        let info = headers(&[
            ("host", "127.0.0.1:7447"),
            ("accept", "application/nostr+json"),
        ]);
        assert_eq!(decide(&info, PORT), Decision::RelayInfo);
        let page = headers(&[("host", "127.0.0.1:7447"), ("accept", "text/html")]);
        assert_eq!(decide(&page, PORT), Decision::NotFound);
    }
}
