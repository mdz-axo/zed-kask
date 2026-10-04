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
pub fn parse_frame(frame: &str) -> Option<MarketEvent> {
    let trimmed = frame.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// The subscription request sent on connect.
pub fn subscription_frame(asset_ids: &[String]) -> String {
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
pub async fn drive_market_events<S, F, Fut>(
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
pub async fn subscribe_market<F, Fut>(asset_ids: &[String], on_event: F) -> Result<(), McpToolError>
where
    F: FnMut(MarketEvent) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let (stream, _response) = async_tungstenite::tokio::connect_async(MARKET_WS_URL)
        .await
        .map_err(|e| McpToolError::unavailable(format!("market WS connect failed: {e}")))?;

    // split() yields a sink whose Send impl is unambiguous (mirrors the
    // repl crate's remote_kernels.rs pattern) — direct `stream.send()` hits
    // a SinkExt resolution quirk under feature unification in this workspace.
    let (mut write, read) = stream.split();
    write
        .send(async_tungstenite::tungstenite::Message::Text(
            subscription_frame(asset_ids).into(),
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
}
