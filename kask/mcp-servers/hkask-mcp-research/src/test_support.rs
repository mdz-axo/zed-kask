//! Shared test support for the research crate's inline test modules.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A loopback HTTP fixture that answers every connection with the same
/// response bytes and counts connections. Serves one response per
/// connection, then closes — enough for GET-based redirect regressions.
pub(crate) async fn http_fixture(response: Arc<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let hits = Arc::new(AtomicUsize::new(0));
    let server_hits = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            server_hits.fetch_add(1, Ordering::SeqCst);
            let response = response.clone();
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
                let _ = socket.write_all(&response).await;
            });
        }
    });
    (format!("http://{address}"), hits)
}
