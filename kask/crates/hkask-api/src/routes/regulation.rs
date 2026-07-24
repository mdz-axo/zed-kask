//! Regulation observability routes — including SSE event stream

use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use hkask_types::event::{RegulationRecord, SpanNamespace};
use hkask_types::{BackpressureSignal, DepletionSignal, LedgerObserver};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;

/// Create Regulation router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with Regulation routes registered
pub fn regulation_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(regulation_health))
        .route(
            "/api/regulation/alerts",
            axum::routing::get(regulation_alerts),
        )
        .routes(routes!(regulation_variety))
        .routes(routes!(regulation_subscribe))
}

/// Broadcast channel capacity for SSE events.
const SSE_CHANNEL_CAPACITY: usize = 256;

// ── SSE Event Envelope ──

/// Union type for all Regulation events that can be streamed over SSE.
/// Wraps RegulationRecord, DepletionSignal, and BackpressureSignal so a single
/// broadcast channel can carry all observer callbacks.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload")]
enum RegulationSseEvent {
    #[serde(rename = "event")]
    RegulationRecord(RegulationRecord),
    #[serde(rename = "depletion")]
    Depletion(DepletionSignal),
    #[serde(rename = "backpressure")]
    Backpressure(BackpressureSignal),
}

// ── SSE Observer Bridge ──

/// Bridge between RegulationLedger's observer pattern and tokio broadcast channel.
///
/// Implements `LedgerObserver` so the Regulation can deliver events via its standard
/// callback interface. Each callback forwards the event into a broadcast channel
/// whose receiver is consumed by the SSE response stream.
struct SseObserver {
    sender: broadcast::Sender<RegulationSseEvent>,
    interest_mask: Vec<SpanNamespace>,
}

impl SseObserver {
    fn new(interest_mask: Vec<SpanNamespace>) -> (Self, broadcast::Receiver<RegulationSseEvent>) {
        let (sender, receiver) = broadcast::channel(SSE_CHANNEL_CAPACITY);
        let observer = Self {
            sender,
            interest_mask,
        };
        (observer, receiver)
    }
}
#[async_trait]
impl LedgerObserver for SseObserver {
    fn interest_mask(&self) -> Vec<SpanNamespace> {
        self.interest_mask.clone()
    }

    async fn on_event(&self, event: &RegulationRecord) {
        let interested =
            self.interest_mask.is_empty() || self.interest_mask.contains(&event.span.namespace);
        if interested {
            let _ = self
                .sender
                .send(RegulationSseEvent::RegulationRecord(event.clone()));
        }
    }

    async fn on_depletion(&self, signal: &DepletionSignal) {
        let _ = self
            .sender
            .send(RegulationSseEvent::Depletion(signal.clone()));
    }

    async fn on_backpressure(&self, signal: &BackpressureSignal) {
        let _ = self
            .sender
            .send(RegulationSseEvent::Backpressure(signal.clone()));
    }
}

// ── Regulation Health ──

/// Regulation health endpoint
#[utoipa::path(
    get,
    path = "/api/regulation/health",
    tag = "regulation",
    responses(
        (status = 200, description = "Regulation health status", body = LedgerHealthResponse),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn regulation_health(
    State(state): State<ApiState>,
) -> axum::Json<LedgerHealthResponse> {
    let health = state.agent_service.ledger().health().await;

    axum::Json(LedgerHealthResponse {
        overall_deficit: health.overall_deficit,
        critical_count: health.critical_count,
        warning_count: health.warning_count,
        healthy: health.healthy,
    })
}

/// Regulation alerts endpoint
async fn regulation_alerts(State(_state): State<ApiState>) -> axum::Json<Vec<String>> {
    axum::Json(vec![])
}

/// Regulation variety endpoint
#[utoipa::path(
    get,
    path = "/api/regulation/variety",
    tag = "regulation",
    responses(
        (status = 200, description = "Regulation variety counters", body = RegulationVarietyResponse),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn regulation_variety(
    State(state): State<ApiState>,
) -> axum::Json<RegulationVarietyResponse> {
    let variety_data = state.agent_service.ledger().variety().await;

    let domains: Vec<String> = variety_data
        .keys()
        .map(|ns| ns.as_str().to_string())
        .collect();

    let counters: std::collections::HashMap<String, VarietyCounterResponse> = variety_data
        .iter()
        .map(|(ns, variety)| {
            (
                ns.as_str().to_string(),
                VarietyCounterResponse {
                    variety: *variety,
                    total: *variety,
                    entropy: 0.0, // Real entropy requires per-domain tracker access
                },
            )
        })
        .collect();

    let total_deficit: u64 = counters.values().map(|c| c.variety).sum();

    axum::Json(RegulationVarietyResponse {
        domains,
        total_deficit,
        counters,
    })
}

// ── Regulation Subscribe (SSE) ──

/// Query parameters for Regulation SSE subscription
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub(crate) struct RegulationSubscribeParams {
    /// Span namespaces to subscribe to (e.g., ["reg.tool", "reg.inference"])
    #[serde(default)]
    spans: Vec<String>,
}

/// Subscribe to Regulation events as an SSE stream.
///
/// The endpoint upgrades the HTTP response to a long-lived SSE connection.
/// Events matching the requested span namespaces are forwarded in real time.
/// Lag notifications are emitted when the client falls behind.
#[utoipa::path(
    get,
    path = "/api/regulation/subscribe",
    tag = "regulation",
    params(RegulationSubscribeParams),
    responses(
        (status = 200, description = "SSE event stream", content_type = "text/event-stream"),
        (status = 400, description = "Invalid request"),
    ),
)]
pub(crate) async fn regulation_subscribe(
    State(state): State<ApiState>,
    Query(params): Query<RegulationSubscribeParams>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Validate spans — filter to only canonical Regulation namespaces
    let valid_spans: Vec<SpanNamespace> = params
        .spans
        .iter()
        .filter_map(|s| SpanNamespace::parse(s))
        .collect();

    let (observer, mut receiver) = SseObserver::new(valid_spans);
    let regulation_runtime = &state.agent_service.ledger().runtime;
    regulation_runtime
        .read()
        .await
        .subscribe_async(Arc::new(observer))
        .await;

    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(regulation_event) => {
                    let data = serde_json::to_string(&regulation_event).unwrap_or_default();
                    let event_type = match regulation_event {
                        RegulationSseEvent::RegulationRecord(_) => "regulation-event",
                        RegulationSseEvent::Depletion(_) => "regulation-depletion",
                        RegulationSseEvent::Backpressure(_) => "regulation-backpressure",
                    };
                    yield Ok(Event::default().data(data).event(event_type));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let data = format!(r#"{{"type":"lagged","count":{n}}}"#);
                    yield Ok(Event::default().data(data).event("regulation-warning"));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
}

// ── Response Types ──

/// Regulation health response — P9 (Homeostatic Self-Regulation).
///
/// `healthy: false` means one or more variety domains are in deficit (below
/// their configured threshold). Check `critical_count` and `warning_count` for
/// severity. `overall_deficit` is the sum of all per-domain deficits.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LedgerHealthResponse {
    /// Sum of all per-domain variety deficits
    pub overall_deficit: u64,
    /// Count of domains in critical deficit (escalation-triggered)
    pub critical_count: usize,
    /// Count of domains in warning deficit
    pub warning_count: usize,
    /// Whether all variety domains are within healthy thresholds
    pub healthy: bool,
}

/// Regulation variety counter for a single domain.
///
/// `variety` is the tracked behavioral diversity for this domain.
/// `entropy` is the Shannon entropy of the domain's observation distribution
/// (0.0 when not yet computed).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VarietyCounterResponse {
    /// Tracked behavioral diversity count
    pub variety: u64,
    /// Total observations in this domain
    pub total: u64,
    /// Shannon entropy (0.0 = not computed yet)
    pub entropy: f64,
}

/// Regulation variety response — per-domain variety counters for the Cybernetic Nervous System (Pattern B).
///
/// `domains` lists all canonical Regulation span namespaces registered (e.g., regulation.tool,
/// regulation.inference, regulation.memory). `total_deficit` is the aggregate variety gap across
/// all domains.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RegulationVarietyResponse {
    /// All registered Regulation span namespace domains
    pub domains: Vec<String>,
    /// Aggregate variety deficit across all domains
    pub total_deficit: u64,
    /// Per-domain variety counters keyed by namespace
    pub counters: std::collections::HashMap<String, VarietyCounterResponse>,
}
