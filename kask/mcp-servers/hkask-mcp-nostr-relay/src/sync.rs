//! Fill the archive from public relays.
//!
//! `LocalRelay::sync_with` cannot do this: it fetches into a client that has
//! no handle on the relay's store, so nothing is saved. A client that does
//! share the store saves everything it receives and skips the admission
//! rule. So sync here is three explicit steps: fetch into a client with no
//! handle on the store, admit each event through the one [`Gate`], and store
//! the admitted ones with `LocalRelay::add_event`, which verifies the event's
//! id and signature first.

use std::collections::BTreeMap;
use std::time::Duration;

use nostr_sdk::prelude::*;
use serde::Serialize;

use crate::admission::{Gate, PATH_SYNC};

/// How long to wait for the relays to connect.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for the stored events.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// The most events one sync asks for.
pub const MAX_EVENTS_PER_SYNC: usize = 5_000;

/// Why a sync could not run at all.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the author allowlist is empty, so there is nothing to sync")]
    NoAuthors,
    #[error("no sync relay is configured")]
    NoRelays,
    #[error("no sync relay could be reached: {0}")]
    NoRelayReachable(String),
    #[error("fetching events failed: {0}")]
    Fetch(String),
}

/// What one sync did.
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    /// Each relay asked, and `connected` or the reason it was not used.
    pub relays: BTreeMap<String, String>,
    /// Events the relays returned.
    pub fetched: usize,
    /// Events the admission rule accepted.
    pub admitted: usize,
    /// Events the admission rule refused.
    pub refused: usize,
    /// Admitted events that were new to the archive.
    pub stored: usize,
    /// Admitted events the archive did not store (already present, replaced
    /// by a newer version, deleted, or expired).
    pub not_stored: usize,
    /// Events that failed id or signature verification.
    pub invalid: usize,
    /// True when the fetch hit [`MAX_EVENTS_PER_SYNC`]; older events may
    /// remain on the relays.
    pub truncated: bool,
}

/// Fetch the allowlisted authors' events from `relays` and store the
/// admitted ones through `relay`.
pub async fn run(
    relay: &LocalRelay,
    gate: &Gate,
    relays: &[RelayUrl],
) -> Result<SyncReport, SyncError> {
    if gate.authors().is_empty() {
        return Err(SyncError::NoAuthors);
    }
    if relays.is_empty() {
        return Err(SyncError::NoRelays);
    }

    let mut report = SyncReport::default();

    // No `.database(...)`: this client must not write to the archive.
    let client = Client::default();
    for url in relays {
        if let Err(error) = client.add_relay(url.clone()).await {
            report
                .relays
                .insert(url.to_string(), format!("not added: {error}"));
        }
    }
    let connected = client.try_connect().timeout(CONNECT_TIMEOUT).await;
    for url in connected.success.keys() {
        report
            .relays
            .insert(url.to_string(), "connected".to_string());
    }
    for (url, reason) in &connected.failed {
        report
            .relays
            .insert(url.to_string(), format!("unreachable: {reason}"));
    }
    if connected.success.is_empty() {
        client.shutdown().await;
        let detail = report
            .relays
            .iter()
            .map(|(url, state)| format!("{url} {state}"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(SyncError::NoRelayReachable(detail));
    }

    // The filter limits what is transferred. It is not the control: a relay
    // can answer with anything, so every event goes through the gate below.
    let filter = Filter::new()
        .authors(gate.authors().iter().copied())
        .limit(MAX_EVENTS_PER_SYNC);
    let fetched = client.fetch_events(filter).timeout(FETCH_TIMEOUT).await;
    client.shutdown().await;
    let events = fetched.map_err(|error| SyncError::Fetch(error.to_string()))?;

    report.fetched = events.len();
    report.truncated = events.len() >= MAX_EVENTS_PER_SYNC;
    for event in events {
        if gate.admit(PATH_SYNC, &event).is_err() {
            report.refused += 1;
            continue;
        }
        report.admitted += 1;
        match relay.add_event(event).await {
            Ok(status) if status.is_success() => report.stored += 1,
            Ok(_) => report.not_stored += 1,
            Err(_) => report.invalid += 1,
        }
    }
    Ok(report)
}
