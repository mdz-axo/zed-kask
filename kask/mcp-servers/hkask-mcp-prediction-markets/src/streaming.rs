//! Polymarket market-channel WebSocket subscriber (T11b).
//!
//! Public channel (no auth): wss://ws-subscriptions-clob.polymarket.com/ws/market.
//! Subscribe with CLOB asset (token) IDs; receive book/price_change/
//! last_trade_price/market_resolved events. `market_resolved` carries
//! `winning_outcome` — the notification arm only: the wire carries no
//! pre-resolution probability, so the stream never writes calibration
//! observations (fabricating one would corrupt the Brier loop); the caller
//! pairs a notification with market_record_resolution.
//!
//! Runs on the server's tokio runtime (MCP servers are tokio processes —
//! the `.rules` background_spawn/GPUI trap does not apply here).

use hkask_mcp_server::server::McpToolError;
use serde::Deserialize;

pub const MARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// The event types we act on; everything else is logged and skipped.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum MarketEvent {
    /// A market resolved: notify + count. The market/winning_asset_id fields
    /// are parsed for forward use (price-snapshot join) but only the outcome
    /// is consumed today.
    MarketResolved {
        #[expect(dead_code, reason = "reserved for the price-snapshot join")]
        market: String,
        winning_outcome: String,
        #[expect(dead_code, reason = "reserved for the price-snapshot join")]
        winning_asset_id: String,
    },
    /// Trade execution: candidate for realized-variance tracking (parsed,
    /// not yet consumed — realized_variance wiring).
    LastTradePrice {
        #[expect(dead_code, reason = "reserved for realized-variance wiring")]
        market: String,
        #[expect(dead_code, reason = "reserved for realized-variance wiring")]
        price: String,
        #[expect(dead_code, reason = "reserved for realized-variance wiring")]
        timestamp: String,
    },
    /// Everything else (book, price_change, tick_size_change, new_market,
    /// best_bid_ask) — captured generically so the stream never dies on an
    /// unhandled variant.
    #[serde(other)]
    Other,
}

/// Parse one WS text frame into a MarketEvent. Unparsable frames and
/// heartbeats (`{}`) return None — never an error (a stream must not die
/// on a heartbeat or an unknown new event type).
pub(crate) fn parse_frame(frame: &str) -> Option<MarketEvent> {
    let trimmed = frame.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// The subscription request sent on connect.
pub(crate) fn subscription_frame(asset_ids: &[String]) -> String {
    serde_json::json!({
        "assets_ids": asset_ids,
        "type": "market"
    })
    .to_string()
}

/// Drive parsed events from a websocket message stream into the handler
/// until the stream ends or the handler signals its bound (returns
/// `true`). Extracted from `subscribe_market` so the bound path is
/// testable against a mock stream (mcp-tool-review PM-08 follow-up: the
/// stop signal was enforced live but unpinned — a mock-stream regression
/// test now pins that the loop stops exactly at the handler's bound).
pub(crate) async fn drive_market_events<S, F, Fut>(
    mut read: S,
    mut on_event: F,
) -> Result<(), McpToolError>
where
    S: futures::Stream<
            Item = Result<
                async_tungstenite::tungstenite::Message,
                async_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
    F: FnMut(MarketEvent) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    use futures::StreamExt as _;

    while let Some(message) = read.next().await {
        let message = message
            .map_err(|e| McpToolError::unavailable(format!("market WS read failed: {e}")))?;
        if let async_tungstenite::tungstenite::Message::Text(text) = message
            && let Some(event) = parse_frame(&text)
        {
            if on_event(event).await {
                // The handler signalled its bound — stop cleanly.
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Connect, subscribe, and drive events into a handler until the stream
/// ends, the handler signals its bound, or the call errors. The handler's
/// future returns `true` to stop (its bound — e.g. max events — is
/// reached): a clean, surfaced stop, not a dropped stream. Errors
/// propagate (typed) — a dead stream is surfaced, never silently dropped.
/// Maximum CLOB asset IDs subscribed in one call (S076-F01).
pub const MAX_SUBSCRIBED_ASSETS: usize = 100;
/// Bounded WebSocket message size (256 KiB).
const MAX_WS_MESSAGE_BYTES: usize = 256 * 1024;
/// Bounded WebSocket frame size (64 KiB).
const MAX_WS_FRAME_BYTES: usize = 64 * 1024;

fn bounded_ws_config() -> async_tungstenite::tungstenite::protocol::WebSocketConfig {
    let mut cfg = async_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    cfg.max_message_size = Some(MAX_WS_MESSAGE_BYTES);
    cfg.max_frame_size = Some(MAX_WS_FRAME_BYTES);
    cfg
}

fn ws_to_http_url(ws_url: &str) -> Result<String, McpToolError> {
    if let Some(rest) = ws_url.strip_prefix("wss://") {
        Ok(format!("https://{rest}"))
    } else if let Some(rest) = ws_url.strip_prefix("ws://") {
        Ok(format!("http://{rest}"))
    } else {
        Err(McpToolError::invalid_argument(format!(
            "WebSocket URL must start with wss:// or ws://, got {ws_url:?}"
        )))
    }
}

pub async fn subscribe_market<F, Fut>(asset_ids: &[String], on_event: F) -> Result<(), McpToolError>
where
    F: FnMut(MarketEvent) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    subscribe_market_at(MARKET_WS_URL, asset_ids, on_event).await
}

pub(crate) async fn subscribe_market_at<F, Fut>(
    ws_url: &str,
    asset_ids: &[String],
    on_event: F,
) -> Result<(), McpToolError>
where
    F: FnMut(MarketEvent) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let cleaned_ids: Vec<String> = asset_ids
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if cleaned_ids.is_empty() {
        return Err(McpToolError::invalid_argument(
            "asset_ids is empty — pass at least one CLOB asset id",
        ));
    }
    if cleaned_ids.len() > MAX_SUBSCRIBED_ASSETS {
        return Err(McpToolError::invalid_argument(format!(
            "asset_ids count ({}) exceeds maximum of {MAX_SUBSCRIBED_ASSETS}",
            cleaned_ids.len()
        )));
    }

    // S076-F01: Establish the verified TLS + HTTP/1.1 WebSocket upgrade via
    // `reqwest` (which configures rustls with platform/native root certificates
    // and its CryptoProvider, avoiding `tokio-rustls-manual-roots`'s empty
    // RootCertStore and workspace ring+aws-lc-rs provider panic), then frame
    // messages via `async_tungstenite` with a bounded `WebSocketConfig`.
    let http_url = ws_to_http_url(ws_url)?;
    let ws_key = async_tungstenite::tungstenite::handshake::client::generate_key();
    let expected_accept =
        async_tungstenite::tungstenite::handshake::derive_accept_key(ws_key.as_bytes());

    let client = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| McpToolError::internal(format!("market WS client init failed: {e}")))?;

    let response = client
        .get(&http_url)
        .header(reqwest::header::CONNECTION, "Upgrade")
        .header(reqwest::header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", &ws_key)
        .send()
        .await
        .map_err(|e| {
            McpToolError::unavailable(format!("market WS connect failed: {}", e.without_url()))
        })?;

    if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
        return Err(McpToolError::unavailable(format!(
            "market WS handshake rejected with HTTP {}",
            response.status()
        )));
    }
    let accept = response
        .headers()
        .get("sec-websocket-accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if accept != expected_accept {
        return Err(McpToolError::unavailable(
            "market WS handshake returned invalid Sec-WebSocket-Accept",
        ));
    }

    let upgraded = response
        .upgrade()
        .await
        .map_err(|e| McpToolError::unavailable(format!("market WS upgrade failed: {e}")))?;

    let stream = async_tungstenite::WebSocketStream::from_raw_socket(
        async_tungstenite::tokio::TokioAdapter::new(upgraded),
        async_tungstenite::tungstenite::protocol::Role::Client,
        Some(bounded_ws_config()),
    )
    .await;

    let (mut write, read) = stream.split();
    write
        .send(async_tungstenite::tungstenite::Message::Text(
            subscription_frame(&cleaned_ids).into(),
        ))
        .await
        .map_err(|e| McpToolError::unavailable(format!("market WS subscribe failed: {e}")))?;

    drive_market_events(read, on_event).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_tungstenite::tungstenite::Message;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn resolved_frame() -> String {
        serde_json::json!({
            "event_type": "market_resolved",
            "market": "m",
            "winning_outcome": "Yes",
            "winning_asset_id": "a",
        })
        .to_string()
    }

    /// PM-08 pin: the handler's bound (max events) stops the drive loop
    /// cleanly — the stream is not drained past the bound, and the stop is
    /// a clean Ok, not an error.
    #[tokio::test]
    async fn handler_bound_stops_the_drive_loop() {
        let frames: Vec<Result<Message, async_tungstenite::tungstenite::Error>> = (0..10)
            .map(|_| Ok(Message::Text(resolved_frame().into())))
            .collect();
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let result = drive_market_events(futures::stream::iter(frames), move |_| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                counter.load(Ordering::SeqCst) >= 3
            }
        })
        .await;
        assert!(result.is_ok(), "a bound stop is a clean Ok: {result:?}");
        assert_eq!(seen.load(Ordering::SeqCst), 3, "stops exactly at the bound");
    }

    /// Heartbeats (`{}`) and unparsable frames are skipped without killing
    /// the stream; a parsed unknown event type (`Other`) still reaches the
    /// handler; stream end is a clean Ok.
    #[tokio::test]
    async fn unknown_frames_are_skipped_and_stream_end_is_clean() {
        let frames = vec![
            Ok(Message::Text("{}".into())),
            Ok(Message::Text("not json".into())),
            Ok(Message::Text(
                serde_json::json!({"event_type": "best_bid_ask"})
                    .to_string()
                    .into(),
            )),
        ];
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let result = drive_market_events(futures::stream::iter(frames), move |_| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                false
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(
            seen.load(Ordering::SeqCst),
            1,
            "only the parsed event reaches the handler"
        );
    }

    /// S076-F01: oversized asset_ids lists are rejected before opening a socket,
    /// and connecting to a dead/non-TLS loopback socket returns a typed
    /// McpToolError::Unavailable without panicking on rustls CryptoProvider.
    #[tokio::test]
    async fn subscribe_market_caps_asset_ids_and_never_panics_on_tls_init() {
        let too_many: Vec<String> = (0..101).map(|i| format!("asset-{i}")).collect();
        let err = subscribe_market(&too_many, |_| async { true })
            .await
            .expect_err("101 asset_ids must be rejected");
        assert_eq!(err.kind, hkask_types::McpErrorKind::InvalidArgument);

        // Bind an ephemeral loopback listener that closes immediately: exercises
        // wss:// TLS connector setup in-process to verify no CryptoProvider panic.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        let tls_res = subscribe_market_at(
            &format!("wss://localhost:{port}/ws/market"),
            &["tok-1".to_string()],
            |_| async { true },
        )
        .await;
        assert!(
            tls_res.is_err(),
            "closed socket must fail cleanly without panic"
        );

        // Also verify full HTTP/1.1 upgrade + bounded WebSocket framing round-trip over loopback.
        let ws_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ws");
        let ws_port = ws_listener.local_addr().expect("addr").port();
        let server = tokio::spawn(async move {
            let (stream, _) = ws_listener.accept().await.expect("accept");
            let mut ws = async_tungstenite::tokio::accept_async(stream)
                .await
                .expect("accept ws");
            let sub = futures::StreamExt::next(&mut ws)
                .await
                .expect("sub msg")
                .expect("ok");
            assert!(sub.into_text().expect("text").contains("tok-live"));
            ws.send(Message::Text(resolved_frame().into()))
                .await
                .expect("send event");
        });
        let got = Arc::new(AtomicUsize::new(0));
        let got_clone = Arc::clone(&got);
        subscribe_market_at(
            &format!("ws://127.0.0.1:{ws_port}/ws/market"),
            &["tok-live".to_string()],
            move |_| {
                let got_clone = Arc::clone(&got_clone);
                async move {
                    got_clone.fetch_add(1, Ordering::SeqCst);
                    true
                }
            },
        )
        .await
        .expect("loopback upgrade + subscribe succeeds");
        server.await.expect("server clean");
        assert_eq!(got.load(Ordering::SeqCst), 1);
    }
}
