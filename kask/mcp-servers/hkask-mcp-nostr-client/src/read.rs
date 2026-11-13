//! Reading events from the local relay and the seed relays.
//!
//! One [`Session`] connects to every relay this server reads from, and each
//! [`Session::read`] asks all of them the same question. Three rules:
//!
//! - **Every relay is named.** Each relay's read ends in exactly one
//!   [`ReadOutcome`]. A relay that went silent is `read_timed_out`, not
//!   complete: the relay client ends a timed-out read the same way as a
//!   finished one, so the timer here is this module's own.
//! - **A relay's answer is not trusted to be the answer asked for.** The
//!   relay client verifies each event's id and signature; this module
//!   verifies again and also drops events that do not match the request,
//!   and counts what it dropped.
//! - **Every event names the relays that returned it.** The same event from
//!   the local relay and a public one is one event with two sources: that is
//!   the redundancy the local relay exists for.

use std::collections::BTreeMap;
use std::time::Duration;

use hkask_types::nostr_wire::{WireEvent, truncate_chars};
use nostr_sdk::prelude::*;
use serde_json::{Value, json};

use crate::config::Config;
use crate::relays::{
    AddressPolicy, LocalRelay, MAX_SEED_RELAYS, PROBE_TIMEOUT, ProbeOutcome, classify_failure,
    read_local_relay, validate_relay_address,
};

/// How long one relay has to finish answering one read.
pub const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// The most characters of a relay's own refusal text a tool repeats.
pub const MAX_REASON_CHARS: usize = 300;

/// The kinds this server does not read: direct messages and their wrappers.
/// The local relay refuses to archive the same four (operator rulings
/// 2026-11-12).
pub const WITHHELD_KINDS: [u16; 4] = [4, 13, 14, 1059];

/// Which relay a read went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The relay server on this machine.
    Local,
    /// A relay from `kask.nostr.seed_relays`.
    Seed,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Seed => "seed",
        }
    }
}

/// How one relay's read ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The relay sent its stored events and said it had no more.
    Complete,
    /// The relay did not finish within [`READ_TIMEOUT`]. What arrived is
    /// kept; more may exist.
    TimedOut,
    /// The connection ended before the relay said it had no more.
    Disconnected,
    /// The relay sent more events than were asked for; reading stopped.
    OverLimit,
    /// The relay refused or closed the request, in its own words.
    Closed { reason: String },
    /// No connection was established.
    NotConnected(ProbeOutcome),
}

impl ReadOutcome {
    /// The state name a tool reports.
    pub fn state(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::TimedOut => "read_timed_out",
            Self::Disconnected => "disconnected",
            Self::OverLimit => "over_limit",
            Self::Closed { .. } => "closed",
            Self::NotConnected(outcome) => outcome.state(),
        }
    }

    fn reason(&self) -> Option<&str> {
        match self {
            Self::Closed { reason } => Some(reason),
            Self::NotConnected(outcome) => outcome.reason(),
            Self::Complete | Self::TimedOut | Self::Disconnected | Self::OverLimit => None,
        }
    }
}

struct Slot {
    role: Role,
    url: RelayUrl,
    link: Result<Relay, ProbeOutcome>,
}

/// Connections to every relay this server reads from, for one tool call.
pub struct Session {
    slots: Vec<Slot>,
    refused: Vec<Value>,
    local: LocalRelay,
    seeds_not_read: usize,
}

/// What one read of every relay found.
pub struct Reading {
    /// The events, newest first, each with the relays that returned it.
    pub events: Vec<(Event, Vec<String>)>,
    /// One report per relay asked.
    pub relays: Vec<Value>,
    /// Relays that gave a usable answer: finished, or cut short with at
    /// least one event.
    pub served: usize,
    /// Whether every relay asked finished its answer.
    pub complete: bool,
    /// The most events any one relay returned. At the number asked for, that
    /// relay may hold more.
    pub largest_answer: usize,
}

enum Ended {
    Stream,
    OverLimit,
    Error(String),
}

impl Session {
    /// Connect to the local relay, if it is announced, and to the first
    /// [`MAX_SEED_RELAYS`] seed relays `policy` admits.
    pub async fn open(config: &Config, policy: AddressPolicy) -> Self {
        let local = read_local_relay(config.local_relay_announcement.as_deref());
        let mut targets: Vec<(Role, RelayUrl)> = Vec::new();
        if let LocalRelay::Announced(url) = &local {
            targets.push((Role::Local, url.clone()));
        }
        let mut refused = Vec::new();
        for (index, entry) in config.seed_relays.iter().take(MAX_SEED_RELAYS).enumerate() {
            match validate_relay_address(entry, policy) {
                Ok(url) => {
                    if !targets.iter().any(|(_, seen)| seen == &url) {
                        targets.push((Role::Seed, url));
                    }
                }
                // By position and reason, never by its text.
                Err(error) => refused.push(json!({
                    "position": index + 1,
                    "role": Role::Seed.name(),
                    "state": "refused",
                    "reason": error.to_string(),
                })),
            }
        }
        let slots = futures::future::join_all(targets.into_iter().map(|(role, url)| async move {
            let relay = Relay::new(url.clone());
            let link = match relay.try_connect().timeout(PROBE_TIMEOUT).await {
                Ok(()) => Ok(relay),
                Err(error) => {
                    relay.shutdown();
                    Err(classify_failure(&error))
                }
            };
            Slot { role, url, link }
        }))
        .await;
        Self {
            slots,
            refused,
            local,
            seeds_not_read: config.seed_relays.len().saturating_sub(MAX_SEED_RELAYS),
        }
    }

    /// Whether there is any relay to ask at all.
    pub fn has_targets(&self) -> bool {
        !self.slots.is_empty()
    }

    /// The local relay as this session found it, for tool output.
    pub fn local_relay_state(&self) -> &'static str {
        match &self.local {
            LocalRelay::NotAnnounced => "not_announced",
            LocalRelay::Announced(_) => "announced",
            LocalRelay::Unusable(_) => "unusable",
        }
    }

    /// Seed relays beyond the bound, which were not looked at.
    pub fn seeds_not_read(&self) -> usize {
        self.seeds_not_read
    }

    /// Ask every relay for the events matching `filters`. At most `cap`
    /// events are taken from any one relay.
    pub async fn read(&self, filters: &[Filter], cap: usize) -> Reading {
        let answers =
            futures::future::join_all(self.slots.iter().map(|slot| read_slot(slot, filters, cap)))
                .await;

        let mut merged: BTreeMap<EventId, (Event, Vec<String>)> = BTreeMap::new();
        let mut relays = Vec::new();
        let mut served = 0;
        let mut complete = true;
        let mut largest_answer = 0;
        for (slot, (events, dropped, outcome)) in self.slots.iter().zip(answers) {
            let address = slot.url.to_string();
            largest_answer = largest_answer.max(events.len());
            let usable = match outcome {
                ReadOutcome::Complete | ReadOutcome::OverLimit => true,
                ReadOutcome::TimedOut | ReadOutcome::Disconnected => !events.is_empty(),
                ReadOutcome::Closed { .. } | ReadOutcome::NotConnected(_) => false,
            };
            if usable {
                served += 1;
            }
            if outcome != ReadOutcome::Complete {
                complete = false;
            }
            let mut report = json!({
                "address": address,
                "role": slot.role.name(),
                "state": outcome.state(),
                "events": events.len(),
                "dropped": dropped,
            });
            if let Some(reason) = outcome.reason() {
                // A refusal is the relay's own text: bounded like any other
                // text this server did not write.
                report["reason"] = json!(truncate_chars(reason, MAX_REASON_CHARS).0);
            }
            relays.push(report);
            for event in events {
                merged
                    .entry(event.id)
                    .or_insert_with(|| (event, Vec::new()))
                    .1
                    .push(address.clone());
            }
        }
        relays.extend(self.refused.iter().cloned());

        let mut events: Vec<(Event, Vec<String>)> = merged.into_values().collect();
        events.sort_by(|(a, _), (b, _)| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        Reading {
            events,
            relays,
            served,
            complete,
            largest_answer,
        }
    }
}

/// Every connection is closed when the session ends, on any path out of a
/// tool.
impl Drop for Session {
    fn drop(&mut self) {
        for slot in &self.slots {
            if let Ok(relay) = &slot.link {
                relay.shutdown();
            }
        }
    }
}

/// One relay's answer: the events kept, how many were dropped, and how the
/// read ended.
async fn read_slot(
    slot: &Slot,
    filters: &[Filter],
    cap: usize,
) -> (Vec<Event>, usize, ReadOutcome) {
    let relay = match &slot.link {
        Ok(relay) => relay,
        Err(outcome) => return (Vec::new(), 0, ReadOutcome::NotConnected(outcome.clone())),
    };
    let mut stream = match relay.stream_events(filters.to_vec()).await {
        Ok(stream) => stream,
        Err(error) => {
            return (
                Vec::new(),
                0,
                ReadOutcome::Closed {
                    reason: error.to_string(),
                },
            );
        }
    };

    let mut events: Vec<Event> = Vec::new();
    let mut dropped = 0usize;
    let ended = tokio::time::timeout(READ_TIMEOUT, async {
        while let Some(item) = stream.next().await {
            let event = match item {
                Ok(event) => event,
                Err(error) => return Ended::Error(error.to_string()),
            };
            let asked_for = filters
                .iter()
                .any(|filter| filter.match_event(&event, MatchEventOptions::new()));
            if !asked_for
                || WITHHELD_KINDS.contains(&event.kind.as_u16())
                || event.verify().is_err()
            {
                dropped += 1;
                continue;
            }
            if events.iter().any(|seen| seen.id == event.id) {
                continue;
            }
            if events.len() == cap {
                return Ended::OverLimit;
            }
            events.push(event);
        }
        Ended::Stream
    })
    .await;
    drop(stream);

    let outcome = match ended {
        Err(_elapsed) => ReadOutcome::TimedOut,
        Ok(Ended::OverLimit) => ReadOutcome::OverLimit,
        Ok(Ended::Error(reason)) => ReadOutcome::Closed { reason },
        // The stream also ends when the connection drops. Only a relay that
        // is still connected said it had no more.
        Ok(Ended::Stream) if relay.status().is_connected() => ReadOutcome::Complete,
        Ok(Ended::Stream) => ReadOutcome::Disconnected,
    };
    (events, dropped, outcome)
}

/// One event in the shape every Nostr tool reports, with its sources.
pub fn wire_event(event: &Event, served_by: &[String]) -> WireEvent {
    let mut wire = WireEvent::bounded(
        event.id.to_hex(),
        event.pubkey.to_hex(),
        event.created_at.as_secs(),
        event.kind.as_u16(),
        &event.content,
        event.tags.iter().map(|tag| tag.as_slice()),
    );
    wire.served_by = served_by.to_vec();
    wire
}
