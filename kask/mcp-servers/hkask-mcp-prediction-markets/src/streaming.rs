//! Polymarket market-channel WebSocket subscriber (T11b).
//!
//! Public channel (no auth): wss://ws-subscriptions-clob.polymarket.com/ws/market.
//! Subscribe with CLOB asset (token) IDs; receive book/price_change/
//! last_trade_price/market_resolved events. `market_resolved` carries
//! `winning_outcome` — the automatic sense arm that feeds resolutions into
//! the calibration store without polling.
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

/// Parse one WS text frame into a MarketEvent. Unparseable frames and
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

/// Connect, subscribe, and drive events into a handler until the stream
/// ends or errors. Errors propagate (typed) — a dead stream is surfaced,
/// never silently dropped.
pub async fn subscribe_market<F, Fut>(
    asset_ids: &[String],
    mut on_event: F,
) -> Result<(), McpToolError>
where
    F: FnMut(MarketEvent) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use futures::StreamExt as _;

    let (stream, _response) = async_tungstenite::tokio::connect_async(MARKET_WS_URL)
        .await
        .map_err(|e| McpToolError::unavailable(format!("market WS connect failed: {e}")))?;

    // split() yields a sink whose Send impl is unambiguous (mirrors the
    // repl crate's remote_kernels.rs pattern) — direct `stream.send()` hits
    // a SinkExt resolution quirk under feature unification in this workspace.
    let (mut write, mut read) = stream.split();
    write
        .send(async_tungstenite::tungstenite::Message::Text(
            subscription_frame(asset_ids).into(),
        ))
        .await
        .map_err(|e| McpToolError::unavailable(format!("market WS subscribe failed: {e}")))?;

    while let Some(message) = read.next().await {
        let message = message
            .map_err(|e| McpToolError::unavailable(format!("market WS read failed: {e}")))?;
        if let async_tungstenite::tungstenite::Message::Text(text) = message
            && let Some(event) = parse_frame(&text)
        {
            on_event(event).await;
        }
    }
    Ok(())
}
