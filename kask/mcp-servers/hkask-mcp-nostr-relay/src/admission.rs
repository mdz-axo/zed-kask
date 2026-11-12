//! The one admission rule for every path into the event store.
//!
//! Events reach the store two ways: a client publishes over the websocket,
//! or `nostr_relay_sync` fetches from a public relay. The SDK applies its
//! write policy and its kind blacklist only to the first path
//! (`LocalRelay::add_event` applies neither), so both paths call
//! [`Gate::admit`] here and nothing is stored that it refuses.

use std::collections::{BTreeSet, VecDeque};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use nostr_sdk::prelude::*;
use serde::Serialize;

/// Kinds that are never stored, whoever signed them.
///
/// The store is unencrypted on disk and readable by any local process, so it
/// holds public material only: gift wraps (operator ruling 2026-11-12), and
/// the other direct-message kinds with them.
pub const REJECTED_KINDS: [Kind; 4] = [
    Kind::EncryptedDirectMessage,
    Kind::Seal,
    Kind::PrivateDirectMessage,
    Kind::GiftWrap,
];

/// The path an event arrived on, recorded with each decision.
pub const PATH_WEBSOCKET: &str = "websocket";
pub const PATH_SYNC: &str = "sync";

/// How many decisions the log keeps.
pub const DECISION_LOG_CAPACITY: usize = 200;

/// Why an event was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The event's kind is in [`REJECTED_KINDS`].
    KindNotAccepted(u16),
    /// The event's author is not on the operator's allowlist.
    AuthorNotAllowed,
}

impl Refusal {
    /// The text sent to a publishing client and written to the decision log.
    pub fn message(&self) -> String {
        match self {
            Refusal::KindNotAccepted(kind) => {
                format!("kind {kind} is not accepted by this relay")
            }
            Refusal::AuthorNotAllowed => "author is not on this relay's allowlist".to_string(),
        }
    }
}

/// One recorded admission decision.
#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    /// Unix seconds when the decision was made.
    pub at: u64,
    /// `websocket` or `sync`.
    pub path: &'static str,
    pub event_id: String,
    pub author: String,
    pub kind: u16,
    /// `admitted`, or the refusal text.
    pub outcome: String,
}

/// The allowlist, the kind rule, and a bounded record of what they decided.
#[derive(Debug)]
pub struct Gate {
    authors: BTreeSet<PublicKey>,
    decisions: Mutex<VecDeque<Decision>>,
    admitted: AtomicU64,
    refused: AtomicU64,
}

impl Gate {
    pub fn new(authors: BTreeSet<PublicKey>) -> Self {
        Self {
            authors,
            decisions: Mutex::new(VecDeque::with_capacity(DECISION_LOG_CAPACITY)),
            admitted: AtomicU64::new(0),
            refused: AtomicU64::new(0),
        }
    }

    /// The authors whose events are admitted.
    pub fn authors(&self) -> &BTreeSet<PublicKey> {
        &self.authors
    }

    /// The rule itself, with no side effect. The kind rule is checked first,
    /// so a rejected kind is refused even from an allowlisted author.
    pub fn check(&self, event: &Event) -> Result<(), Refusal> {
        if REJECTED_KINDS.contains(&event.kind) {
            return Err(Refusal::KindNotAccepted(event.kind.as_u16()));
        }
        if !self.authors.contains(&event.pubkey) {
            return Err(Refusal::AuthorNotAllowed);
        }
        Ok(())
    }

    /// Apply the rule and record the decision.
    pub fn admit(&self, path: &'static str, event: &Event) -> Result<(), Refusal> {
        let result = self.check(event);
        let outcome = match &result {
            Ok(()) => {
                self.admitted.fetch_add(1, Ordering::Relaxed);
                "admitted".to_string()
            }
            Err(refusal) => {
                self.refused.fetch_add(1, Ordering::Relaxed);
                refusal.message()
            }
        };
        let decision = Decision {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default(),
            path,
            event_id: event.id.to_hex(),
            author: event.pubkey.to_hex(),
            kind: event.kind.as_u16(),
            outcome,
        };
        let mut decisions = self
            .decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if decisions.len() == DECISION_LOG_CAPACITY {
            decisions.pop_front();
        }
        decisions.push_back(decision);
        result
    }

    /// The most recent decisions, newest first.
    pub fn recent(&self, limit: usize) -> Vec<Decision> {
        let decisions = self
            .decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        decisions.iter().rev().take(limit).cloned().collect()
    }

    /// Decisions since this process started: `(admitted, refused)`.
    pub fn totals(&self) -> (u64, u64) {
        (
            self.admitted.load(Ordering::Relaxed),
            self.refused.load(Ordering::Relaxed),
        )
    }
}

/// The websocket path's hook into [`Gate`].
#[derive(Debug)]
pub struct WebsocketPolicy(pub Arc<Gate>);

impl WritePolicy for WebsocketPolicy {
    fn admit_event<'a>(
        &'a self,
        event: &'a Event,
        _addr: &'a SocketAddr,
    ) -> Pin<Box<dyn Future<Output = WritePolicyResult> + Send + 'a>> {
        Box::pin(async move {
            match self.0.admit(PATH_WEBSOCKET, event) {
                Ok(()) => WritePolicyResult::Accept,
                Err(refusal) => {
                    WritePolicyResult::reject(MachineReadablePrefix::Blocked, refusal.message())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(keys: &Keys, kind: Kind) -> Event {
        EventBuilder::new(kind, "text")
            .finalize(keys)
            .expect("event signs")
    }

    #[test]
    fn an_allowlisted_author_is_admitted_and_another_is_refused() {
        let allowed = Keys::generate();
        let stranger = Keys::generate();
        let gate = Gate::new(BTreeSet::from([allowed.public_key()]));
        assert_eq!(gate.check(&note(&allowed, Kind::TextNote)), Ok(()));
        assert_eq!(
            gate.check(&note(&stranger, Kind::TextNote)),
            Err(Refusal::AuthorNotAllowed)
        );
    }

    #[test]
    fn every_rejected_kind_is_refused_even_from_an_allowlisted_author() {
        let allowed = Keys::generate();
        let gate = Gate::new(BTreeSet::from([allowed.public_key()]));
        for kind in REJECTED_KINDS {
            assert_eq!(
                gate.check(&note(&allowed, kind)),
                Err(Refusal::KindNotAccepted(kind.as_u16())),
                "kind {} must be refused",
                kind.as_u16()
            );
        }
        assert!(REJECTED_KINDS.contains(&Kind::GiftWrap));
    }

    #[test]
    fn an_empty_allowlist_admits_nothing() {
        let gate = Gate::new(BTreeSet::new());
        assert_eq!(
            gate.check(&note(&Keys::generate(), Kind::TextNote)),
            Err(Refusal::AuthorNotAllowed)
        );
    }

    #[test]
    fn decisions_are_recorded_newest_first_and_bounded() {
        let allowed = Keys::generate();
        let stranger = Keys::generate();
        let gate = Gate::new(BTreeSet::from([allowed.public_key()]));
        for _ in 0..DECISION_LOG_CAPACITY {
            let _ = gate.admit(PATH_SYNC, &note(&stranger, Kind::TextNote));
        }
        let last = note(&allowed, Kind::TextNote);
        assert_eq!(gate.admit(PATH_WEBSOCKET, &last), Ok(()));

        let recent = gate.recent(DECISION_LOG_CAPACITY + 50);
        assert_eq!(recent.len(), DECISION_LOG_CAPACITY, "the log is bounded");
        assert_eq!(recent[0].event_id, last.id.to_hex());
        assert_eq!(recent[0].outcome, "admitted");
        assert_eq!(recent[0].path, PATH_WEBSOCKET);
        assert_eq!(recent[1].outcome, Refusal::AuthorNotAllowed.message());
        assert_eq!(gate.totals(), (1, DECISION_LOG_CAPACITY as u64));
    }
}
