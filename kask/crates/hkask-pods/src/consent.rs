//! Consent Manager — User consent tracking for sovereignty boundaries
//!
//! Manages explicit user consent for data access:
//! - Grant consent for specific data categories
//! - Revoke consent
//! - Audit consent history
//! - Check consent status
//!
//! Consent records are persisted via `ConsentStore` (SQLite-backed),
//! so they survive restarts — enforcing user sovereignty (Principle 1.3).

use crate::ports::ConsentPort;
use crate::sovereignty::SovereigntyConsent;
use hkask_types::DataCategory;
use hkask_types::WebID;
use hkask_types::consent_port::StoredConsentRecord as PortStoredConsentRecord;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::{debug, warn};

/// Consent manager errors
#[derive(Debug, Error)]
pub enum ConsentError {
    #[error("Consent store error: {0}")]
    Store(#[from] hkask_types::InfrastructureError),

    #[error("Consent not found for WebID: {0}")]
    ConsentNotFound(String),
}

/// Consent record (in-memory cache entry)
#[derive(Debug, Clone)]
pub(crate) struct ConsentRecord {
    pub(crate) webid: String,
    pub(crate) granted_categories: HashSet<String>,
    pub(crate) granted_at: i64,
    pub(crate) revoked_at: Option<i64>,
    pub(crate) active: bool,
}

impl ConsentRecord {
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — consent record starts empty and active
    /// \[P1\] Constraining: User Sovereignty — record is bound to user WebID
    /// pre:  `webid` is a non-empty string.
    /// post: Returns a new `ConsentRecord` with empty granted categories,
    ///       `active = true`, `revoked_at = None`, and `granted_at` set to
    ///       the current UTC timestamp.
    pub fn new(webid: &str) -> Self {
        Self {
            webid: webid.to_string(),
            granted_categories: HashSet::new(),
            granted_at: chrono::Utc::now().timestamp(),
            revoked_at: None,
            active: true,
        }
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — explicit grant adds a data category
    /// pre:  `category` is a non-empty string.
    /// post: `category` is added to `granted_categories`; `active` is set
    ///       to `true`; `revoked_at` is cleared to `None`.
    pub fn grant(&mut self, category: &str) {
        self.granted_categories.insert(category.to_string());
        self.active = true;
        self.revoked_at = None;
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — revocation terminates consent
    /// pre:  (none — revoke is always valid).
    /// post: `revoked_at` is set to the current UTC timestamp;
    ///       `active` is set to `false`.
    pub fn revoke(&mut self) {
        self.revoked_at = Some(chrono::Utc::now().timestamp());
        self.active = false;
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — active iff not revoked
    /// pre:  (none).
    /// post: Returns `true` iff `active == true` AND `revoked_at` is `None`.
    pub fn is_active(&self) -> bool {
        self.active && self.revoked_at.is_none()
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — category check enforces scoped grant
    /// pre:  `category` is a non-empty string.
    /// post: Returns `true` iff the record is active AND `category` is
    ///       present in `granted_categories`.
    pub fn has_category(&self, category: &str) -> bool {
        self.active && self.granted_categories.contains(category)
    }
}

impl From<PortStoredConsentRecord> for ConsentRecord {
    fn from(stored: PortStoredConsentRecord) -> Self {
        Self {
            webid: stored.webid,
            granted_categories: stored.granted_categories,
            granted_at: stored.granted_at,
            revoked_at: stored.revoked_at,
            active: stored.active,
        }
    }
}

impl ConsentRecord {
    /// Convert to a `StoredConsentRecord` for persistence.
    /// Uses a stable id derived from the webid to enable upserts
    /// rather than generating a new UUID per call.
    fn to_stored(&self) -> PortStoredConsentRecord {
        PortStoredConsentRecord {
            id: format!("cr_{}", self.webid),
            webid: self.webid.clone(),
            granted_categories: self.granted_categories.clone(),
            granted_at: self.granted_at,
            revoked_at: self.revoked_at,
            active: self.active,
        }
    }
}

/// Consent manager with persistent storage
///
/// Uses a `ConsentStore` for persistence and an in-memory cache for
/// fast reads. Writes go to both the store and the cache; reads
/// check the cache first (loaded eagerly from the store on startup).
pub struct ConsentManager {
    store: Arc<dyn ConsentPort>,
    cache: Arc<RwLock<Vec<ConsentRecord>>>,
    /// Optional Regulation event sink for observability of consent denials.
    /// When set, a `reg.consent.denied` ν-event is emitted every time
    /// `has_consent` returns false, closing the observability loop
    /// on the Prohibition gate (Magna Carta P2).
    event_sink: Option<Arc<dyn RegulationSink>>,
}

impl ConsentManager {
    /// Create a new consent manager backed by the given store.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — manager caches active consent records
    /// pre:  `store` is a valid `Arc<dyn ConsentPort>`.
    /// post: Returns a `ConsentManager` with an empty in-memory cache;
    ///       eagerly loads active records from the store into the cache;
    ///       logs a warning if the load fails (cache remains empty).
    pub fn new(store: Arc<dyn ConsentPort>) -> Self {
        let manager = Self {
            store,
            cache: Arc::new(RwLock::new(Vec::new())),
            event_sink: None,
        };
        if let Err(e) = manager.load_from_store() {
            tracing::warn!("Failed to load consent records from store: {}", e);
        }
        manager
    }

    /// Set a Regulation event sink for consent denial observability.
    ///
    /// When set, every `has_consent` denial produces a `reg.consent.denied`
    /// ν-event. This provides observability without opening a feedback path
    /// (the denial remains terminal — this is a Prohibition, not a Guardrail).
    /// # REQ: OPEN_QUESTIONS §2.2 — consent denial Regulation instrumentation.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — Regulation instrumentation for denials (observability only, no feedback)
    /// pre:  `sink` is a valid `Arc<dyn RegulationSink>`.
    /// post: Returns `self` with `event_sink` set to `Some(sink)`.
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Load all active consent records from the store into the in-memory cache
    fn load_from_store(&self) -> Result<(), ConsentError> {
        let stored = self.store.list_active()?;
        let records: Vec<ConsentRecord> = stored.into_iter().map(ConsentRecord::from).collect();
        let mut cache = self
            .cache
            .write()
            .map_err(|_| ConsentError::Store(hkask_types::InfrastructureError::LockPoisoned))?;
        *cache = records;
        Ok(())
    }

    /// Persist a consent record to the store
    fn persist(&self, record: &ConsentRecord) -> Result<(), ConsentError> {
        let stored = record.to_stored();
        self.store.store(&stored)?;
        Ok(())
    }

    /// Grant consent for a data category.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — persist a scoped grant
    /// pre:  `webid` is a non-empty string; `category` is a valid
    ///       `DataCategory` variant.
    /// post: If a record exists for `webid`, the category is granted and
    ///       persisted; otherwise a new record is created, granted, and
    ///       persisted. Returns `Ok(())` on success.
    pub fn grant_consent(&self, webid: &str, category: &DataCategory) -> Result<(), ConsentError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|_| ConsentError::Store(hkask_types::InfrastructureError::LockPoisoned))?;

        // Find or create consent record
        let record = cache.iter_mut().find(|r| r.webid == webid);

        if let Some(record) = record {
            record.grant(category.as_str());
            self.persist(record)?;
        } else {
            let mut new_record = ConsentRecord::new(webid);
            new_record.grant(category.as_str());
            self.persist(&new_record)?;
            cache.push(new_record);
        }

        tracing::info!(
            target: "reg.sovereignty",
            operation = "consent_granted",
            webid = %webid,
            category = ?category,
            "REG"
        );

        debug!(
            "Granted consent for WebID: {} category: {}",
            webid,
            category.as_str()
        );
        Ok(())
    }

    /// Revoke all consent for a WebID.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — revoke all consent for a WebID
    /// pre:  `webid` is a non-empty string.
    /// post: If a record exists for `webid`, it is revoked and persisted;
    ///       returns `Ok(())`. If no record exists, returns
    ///       `Err(ConsentError::ConsentNotFound)`.
    pub fn revoke_consent(&self, webid: &str) -> Result<(), ConsentError> {
        let mut cache = self
            .cache
            .write()
            .map_err(|_| ConsentError::Store(hkask_types::InfrastructureError::LockPoisoned))?;

        if let Some(record) = cache.iter_mut().find(|r| r.webid == webid) {
            record.revoke();
            self.persist(record)?;
            tracing::info!(
                target: "reg.sovereignty",
                operation = "consent_revoked",
                webid = %webid,
                "REG"
            );
            debug!("Revoked consent for WebID: {}", webid);
            Ok(())
        } else {
            Err(ConsentError::ConsentNotFound(webid.to_string()))
        }
    }

    /// Check if consent is granted for a data category.
    ///
    /// Emits a `reg.consent.denied` ν-event when consent is denied,
    /// providing observability without opening a feedback path.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — terminal deny unless active grant exists
    /// \[P1\] Constraining: User Sovereignty — check is per-user/data-category
    /// pre:  `webid` is a non-empty string; `category` is a valid
    ///       `DataCategory` variant.
    /// post: Returns `Ok(true)` if an active record for `webid` has the
    ///       category granted; `Ok(false)` otherwise (including when no
    ///       record exists). Emits a denial ν-event on `false`.
    #[must_use = "result must be used"]
    pub fn has_consent(&self, webid: &str, category: &DataCategory) -> Result<bool, ConsentError> {
        let cache = self
            .cache
            .read()
            .map_err(|_| ConsentError::Store(hkask_types::InfrastructureError::LockPoisoned))?;

        let granted = cache
            .iter()
            .find(|r| r.webid == webid)
            .map(|r| r.has_category(category.as_str()))
            .unwrap_or(false);

        if !granted {
            tracing::info!(
                target: "reg.sovereignty",
                operation = "consent_checked",
                webid = %webid,
                category = ?category,
                result = "denied",
                "REG"
            );
            self.emit_consent_denied(webid, category);
        }

        Ok(granted)
    }

    /// Emit a `reg.consent.denied` ν-event for observability.
    ///
    /// This is a Prohibition-gate observation, not a regulatory loop signal.
    /// The denial is terminal — the event records the fact for audit.
    fn emit_consent_denied(&self, webid: &str, category: &DataCategory) {
        if let Some(ref sink) = self.event_sink {
            let event = RegulationRecord::new(
                WebID::from_persona(b"consent"),
                Span::new(
                    SpanNamespace::new("reg.consent").expect("canonical namespace: reg.consent"),
                    "denied",
                ),
                CyclePhase::Compare,
                serde_json::json!({
                    "webid": webid,
                    "category": category.as_str(),
                }),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                warn!(
                    target: "reg.consent",
                    error = %e,
                    webid = %webid,
                    category = %category.as_str(),
                    "Failed to persist consent denial event"
                );
            }
        }
    }

    /// Get all granted categories for a WebID.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — list granted categories for disclosure
    /// pre:  `webid` is a non-empty string.
    /// post: Returns `Ok(Vec<String>)` containing all granted category
    ///       names for an active record; returns `Ok(vec![])` if no active
    ///       record exists for `webid`.
    #[must_use = "result must be used"]
    pub fn get_granted_categories(&self, webid: &str) -> Result<Vec<String>, ConsentError> {
        let cache = self
            .cache
            .read()
            .map_err(|_| ConsentError::Store(hkask_types::InfrastructureError::LockPoisoned))?;

        Ok(cache
            .iter()
            .find(|r| r.webid == webid && r.is_active())
            .map(|r| r.granted_categories.iter().cloned().collect())
            .unwrap_or_default())
    }
}

impl SovereigntyConsent for ConsentManager {
    fn has_consent(&self, webid: &str, category: &DataCategory) -> bool {
        // Translate storage errors into "deny by default" — sovereignty must
        // fail closed, never open. The Magna Carta's "Maximum" default
        // fail-closed default deny is enforced by this conservative translation.
        ConsentManager::has_consent(self, webid, category).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    #[test]
    fn consent_record_new_has_correct_defaults() {
        let record = ConsentRecord::new("user:alice");
        assert_eq!(record.webid, "user:alice");
        assert!(record.granted_categories.is_empty());
        assert!(record.is_active());
        assert!(record.revoked_at.is_none());
        assert!(record.granted_at > 0);
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    #[test]
    fn consent_record_grant_adds_category_and_activates() {
        let mut record = ConsentRecord::new("user:alice");
        // First revoke to set inactive state, then grant to verify reactivation
        record.revoke();
        assert!(!record.is_active());

        record.grant("episodic_memory");
        assert!(record.is_active());
        assert!(record.revoked_at.is_none());
        assert!(record.has_category("episodic_memory"));
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    #[test]
    fn consent_record_revoke_sets_inactive() {
        let mut record = ConsentRecord::new("user:alice");
        record.grant("episodic_memory");
        assert!(record.is_active());

        record.revoke();
        assert!(!record.is_active());
        assert!(record.revoked_at.is_some());
        // After revoke, previously granted categories should not be accessible
        assert!(!record.has_category("episodic_memory"));
    }

    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    #[test]
    fn consent_record_has_category_only_when_active_and_granted() {
        let mut record = ConsentRecord::new("user:alice");
        // Not granted yet
        assert!(!record.has_category("episodic_memory"));

        record.grant("episodic_memory");
        assert!(record.has_category("episodic_memory"));
        // Different category not granted
        assert!(!record.has_category("semantic_memory"));

        record.revoke();
        // After revoke, even granted categories are denied
        assert!(!record.has_category("episodic_memory"));
    }
}
