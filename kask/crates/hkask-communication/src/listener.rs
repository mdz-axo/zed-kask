//! 7R7 Communication Listener — polls Matrix rooms and emits Regulation observation spans.
//!
//! The 7R7 bot is a passive listener. It polls Matrix rooms on a configurable
//! interval, receives messages, and emits Regulation spans for observability. It does
//! NOT classify, escalate, moderate, or judge content. Those decisions belong
//! to the agent layer (Curator + skills + templates + LLM calls).
//!
//! Architecture:
//!   Matrix rooms → 7R7 poll → Regulation span emission → agent layer (Curator)
//!
//! The communication server is a dumb pipe. Regulation observes. Agents decide.

use crate::matrix::{MatrixMessage, MatrixTransport, UserId};
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span};
use hkask_types::{EventID, WebID};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, watch};

fn is_self_authored(message: &MatrixMessage, authenticated_user: Option<&UserId>) -> bool {
    authenticated_user == Some(&message.sender)
}

// ── 7R7 Listener ───────────────────────────────────────────────────────────

/// 7R7 communication listener — polls Matrix for messages, emits Regulation spans.
///
/// This is a passive observer. It does not classify, escalate, or moderate.
/// Content decisions are made by the agent layer (Curator + skills + templates).
pub struct SevenR7Listener {
    /// Matrix transport for polling (Mutex-wrapped for shared &mut access).
    matrix: Arc<Mutex<MatrixTransport>>,
    /// Polling interval in seconds.
    poll_interval_secs: u64,
    /// Regulation event sink for persisting observed messages as RegulationRecords.
    /// When set, the listener joins the Regulation observability fabric;
    /// the curation loop can then sense Matrix activity.
    event_sink: Option<Arc<dyn RegulationSink>>,
    /// Whether the listener is active.
    active: RwLock<bool>,
    /// Cancellation channel — dropping the sender (via stop) signals the loop to exit.
    cancel_tx: RwLock<Option<watch::Sender<bool>>>,
}

impl SevenR7Listener {
    /// Create a new 7R7 listener.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  matrix is a valid MatrixTransport (authenticated)
    /// pre:  poll_interval_secs > 0
    /// post: returns SevenR7Listener with active=false
    pub fn new(matrix: Arc<Mutex<MatrixTransport>>, poll_interval_secs: u64) -> Self {
        Self {
            matrix,
            poll_interval_secs,
            event_sink: None,
            active: RwLock::new(false),
            cancel_tx: RwLock::new(None),
        }
    }

    /// Attach a Regulation event sink for persisting observed messages.
    ///
    /// Without this, the listener only emits tracing events.
    /// With it, observed messages flow into the RegulationRecord store
    /// where the curation loop can sense them.
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Start the polling loop.
    ///
    /// Spawns a background task that polls Matrix rooms on the configured
    /// interval and emits Regulation observation spans for each message received.
    /// The agent layer (Curator) subscribes to these spans and decides what
    /// action to take.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// pre:  matrix transport is authenticated
    /// post: background polling task spawned
    /// post: idempotent — calling start() on already-active listener is no-op
    pub async fn start(&self) {
        let was_active = *self.active.read().await;
        if was_active {
            return;
        }
        *self.active.write().await = true;

        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        *self.cancel_tx.write().await = Some(cancel_tx);

        let matrix = Arc::clone(&self.matrix);
        let interval = self.poll_interval_secs;
        let event_sink = self.event_sink.clone();

        tokio::spawn(async move {
            let mut timer = tokio::time::interval(std::time::Duration::from_secs(interval));
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        // List known rooms
                        let rooms = {
                            let transport = matrix.lock().await;
                            match transport.list_rooms().await {
                                Ok(r) => r,
                                Err(e) => {
                                    tracing::warn!(
                                        target: "reg.communication.listener",
                                        error = %e,
                                        "7R7 failed to list rooms"
                                    );
                                    continue;
                                }
                            }
                        };

                        for room in &rooms {
                            let room_id = room.room_id.as_str();
                            let transport = matrix.lock().await;
                            let authenticated_user = transport.authenticated_user_id();
                            match transport.get_messages(&room.room_id, 10).await {
                                Ok(messages) => {
                                    for msg in &messages {
                                        if is_self_authored(msg, authenticated_user.as_ref()) {
                                            tracing::debug!(
                                                target: "reg.communication.message.ignored",
                                                room_id = %room_id,
                                                event_id = %msg.event_id,
                                                "7R7 ignored its own Matrix message"
                                            );
                                            continue;
                                        }

                                        tracing::info!(
                                            target: "reg.communication.message.observed",
                                            room_id = %room_id,
                                            sender = %msg.sender.as_str(),
                                            body_len = %msg.body.len(),
                                            "7R7 observed message"
                                        );
                                        // Persist RegulationRecord so the curation loop can sense it.
                                        if let Some(ref sink) = event_sink {
                                            let span = Span::new(
                                                hkask_types::event::SpanNamespace::new("reg.communication.message").expect("canonical namespace: reg.communication.message"),
                                                "observed",
                                            );
                                        let mut event = RegulationRecord::new(
                                            WebID::from_persona(b"7r7-listener"),
                                            span,
                                            CyclePhase::Act,
                                            serde_json::json!({
                                                "source_event_id": msg.event_id,
                                                "room_id": room_id,
                                                "sender": msg.sender.as_str(),
                                                "body": msg.body,
                                                "timestamp": msg.timestamp,
                                            }),
                                            0,
                                        );
                                        event.id = EventID::from_uuid(uuid::Uuid::new_v5(
                                            &uuid::Uuid::NAMESPACE_URL,
                                            msg.event_id.as_bytes(),
                                        ));
                                        match sink.persist_if_absent(&msg.event_id, &event) {
                                            Ok(true) => {}
                                            Ok(false) => {
                                                tracing::debug!(
                                                    target: "reg.communication.message.ignored",
                                                    room_id = %room_id,
                                                    event_id = %msg.event_id,
                                                    "7R7 ignored replayed Matrix message"
                                                );
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    target: "reg.communication.listener",
                                                    error = %e,
                                                    "7R7 failed to persist RegulationRecord"
                                                );
                                            }
                                        }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        target: "reg.communication.listener",
                                        room_id = %room_id,
                                        error = %e,
                                        "7R7 failed to poll room"
                                    );
                                }
                            }
                        }
                    }
                    _ = cancel_rx.changed() => {
                        tracing::info!(target: "reg.communication.listener", "7R7 listener stopped");
                        break;
                    }
                }
            }
        });

        tracing::info!(
            target: "reg.communication.listener.started",
            interval_secs = %interval,
            "7R7 listener started"
        );
    }

    /// Stop the polling loop.
    ///
    /// expect: "Agents communicate through user-owned channels"
    /// post: active flag set to false
    /// post: idempotent — calling stop() on already-stopped listener is no-op
    pub async fn stop(&self) {
        *self.active.write().await = false;
        // Dropping the sender triggers the receiver in the select! loop.
        *self.cancel_tx.write().await = None;
        tracing::info!(target: "reg.communication.listener.stopped", "7R7 listener stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_authored_messages_are_ignored() {
        let message = MatrixMessage {
            event_id: "$self:localhost".to_string(),
            sender: UserId::new("@curator:localhost"),
            body: "reply".to_string(),
            structured: None,
            timestamp: 0,
        };

        assert!(is_self_authored(
            &message,
            Some(&UserId::new("@curator:localhost"))
        ));
        assert!(!is_self_authored(
            &message,
            Some(&UserId::new("@human:localhost"))
        ));
    }
}
