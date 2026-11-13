//! Shared test support for the research crate's inline test modules.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A loopback HTTP fixture that answers every connection with the same
/// response bytes and counts connections. Serves one response per
/// connection, then closes — enough for GET-based redirect regressions.
pub(crate) async fn http_fixture(response: Arc<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    http_fixture_sequence(vec![(*response).clone()]).await
}

/// A loopback HTTP fixture that answers every connection with `response`
/// and records each request's body. The header-only fixtures above cannot
/// show what a POST carried; a provider's request shape is exactly what
/// the Firecrawl extract pin has to see.
pub(crate) async fn http_fixture_capturing_bodies(
    response: Vec<u8>,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = bodies.clone();
    let response = Arc::new(response);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let recorded = recorded.clone();
            let response = response.clone();
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 2048];
                // Read the head, then exactly Content-Length body bytes.
                let (head_end, content_length) = loop {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                    if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buffer[..position]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (position + 4, length);
                    }
                };
                while buffer.len() < head_end + content_length {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                }
                let body = String::from_utf8_lossy(&buffer[head_end..]).into_owned();
                if let Ok(mut bodies) = recorded.lock() {
                    bodies.push(body);
                }
                let _ = socket.write_all(&response).await;
            });
        }
    });
    (format!("http://{address}"), bodies)
}

/// A loopback HTTP fixture that answers each connection with the next
/// response from a fixed sequence, counting connections; once the sequence
/// is exhausted the final response repeats. The retry paths need the same
/// URL to answer differently per attempt (429 then 200), which the
/// single-response fixture cannot serve.
pub(crate) async fn http_fixture_sequence(responses: Vec<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let hits = Arc::new(AtomicUsize::new(0));
    let server_hits = hits.clone();
    let responses = Arc::new(responses);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // The hit count doubles as the response index: the retry paths
            // connect sequentially, so attempt N answers with
            // responses[N].
            let index = server_hits.fetch_add(1, Ordering::SeqCst);
            let responses = responses.clone();
            tokio::spawn(async move {
                // Drain the request head before answering.
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 512];
                loop {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buffer.extend_from_slice(&chunk[..n]);
                            if buffer.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let last = responses.len().saturating_sub(1);
                let _ = socket.write_all(&responses[index.min(last)]).await;
            });
        }
    });
    (format!("http://{address}"), hits)
}
