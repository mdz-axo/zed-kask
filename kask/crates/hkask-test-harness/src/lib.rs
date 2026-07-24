#![forbid(unsafe_code)]
//! hKask Test Harness — Shared fixtures for test infrastructure
//!
//! Public API:
//! - `TestDb` — isolated temp SQLite database with full schema
//! - `TestKeystore` — temp directory with test master key
//! - `TestWebId` — factory for valid test WebIDs
//! - `MockRegulationLedger` — Regulation runtime with controllable state
//! - `temp_dir()` — guarded temp directory, auto-cleans on drop
//! - `test_event()` / `test_h_mem()` — factories for well-formed test entities
//! - `strategies` — proptest strategy functions for core types
//! - `test_runner` — cargo test invocation and REQ-tagged failure parsing
//! - `ProbContractRunner` — (p, δ, k)-satisfaction for non-deterministic contracts
//!
//! # Principle grounding
//! - P5 (Essentialism): each public item does one thing well
//! - P8 (Semantic Grounding): every test using these fixtures carries REQ tags
//! - P12 (Authenticated Host Mandate): all test identities use TestWebId (authenticated)

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]

pub mod fuzz;
pub mod prob_contract;
pub mod qa_script;
mod schema;
pub mod self_heal;
pub mod strategies;
pub mod test_runner;

pub use prob_contract::{ProbContractResult, ProbContractRunner};
pub use schema::SCHEMA_SQL;
pub use test_runner::ExpectProposal;

use chrono::Utc;
use hkask_storage::HMem;
use hkask_types::event::{CyclePhase, RegulationRecord, Span};
use hkask_types::id::WebID;
use rand::Rng;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tempfile::TempDir;

// ── TestDb ────────────────────────────────────────────────────────────────────

/// Isolated temp SQLite database with full hKask schema initialized.
///
/// Creates an in-memory database and runs the complete schema DDL.
/// The database is destroyed when `TestDb` is dropped.
///
/// # Example
/// ```ignore
/// let db = TestDb::new();
/// db.conn().execute("INSERT INTO hmems ...", [])?;
/// ```
pub struct TestDb {
    conn: Arc<Mutex<Connection>>,
}

impl Default for TestDb {
    /// post: returns TestDb with in-memory SQLite connection and full schema initialized
    fn default() -> Self {
        Self::new()
    }
}

impl TestDb {
    /// Create a new in-memory test database with full schema.
    ///
    /// post: returns TestDb with in-memory SQLite connection and full schema initialized
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory SQLite should always open");
        conn.execute_batch(SCHEMA_SQL)
            .expect("schema initialization should succeed");
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Borrow the underlying SQLite connection (locks the mutex).
    ///
    /// post: returns `MutexGuard<Connection>` for direct SQL access
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Execute a batch of SQL statements (for seeding test data).
    ///
    /// pre:  sql is valid SQL
    /// post: batch executed on the connection
    pub fn execute_batch(&self, sql: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute_batch(sql)
    }
}

// ── TestKeystore ──────────────────────────────────────────────────────────────

/// Temp directory with a test master key file.
///
/// Creates a temporary directory containing a generated master key.
/// The directory and all contents are deleted when `TestKeystore` is dropped.
pub struct TestKeystore {
    dir: TempDir,
    key_path: PathBuf,
    master_key: [u8; 32],
}

impl Default for TestKeystore {
    /// post: returns TestKeystore with temp dir, key file written, 32-byte master key
    fn default() -> Self {
        Self::new()
    }
}

impl TestKeystore {
    /// Create a new test keystore with a randomly generated master key.
    ///
    /// post: returns TestKeystore with temp dir, key file written, 32-byte master key
    pub fn new() -> Self {
        let dir = TempDir::new().expect("temp dir creation should succeed");
        let key_path = dir.path().join("master.key");
        let master_key: [u8; 32] = rand::rng().random();
        std::fs::write(&key_path, master_key).expect("key file write should succeed");
        Self {
            dir,
            key_path,
            master_key,
        }
    }

    /// Path to the keystore directory.
    ///
    /// post: returns &Path to the temp directory
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Path to the master key file.
    ///
    /// post: returns &Path to the master.key file
    pub fn key_path(&self) -> &std::path::Path {
        &self.key_path
    }

    /// The generated master key bytes.
    ///
    /// post: returns &[u8; 32] reference to the master key
    pub fn master_key(&self) -> &[u8; 32] {
        &self.master_key
    }
}

// ── TestWebId ─────────────────────────────────────────────────────────────────

/// Factory for valid test WebIDs with known identities.
///
/// Provides deterministic WebIDs for common test personas (alice, bob, carol)
/// plus a random generator. All WebIDs are valid and carry authenticated identity.
///
/// # Example
/// ```ignore
/// let alice = TestWebId::alice();
/// let bob = TestWebId::bob();
/// let random = TestWebId::random();
/// ```
pub struct TestWebId;

impl TestWebId {
    /// Deterministic WebID for test user "alice".
    ///
    /// post: returns deterministic WebID from persona b"alice"
    pub fn alice() -> WebID {
        WebID::from_persona(b"alice")
    }

    /// Deterministic WebID for test user "bob".
    ///
    /// post: returns deterministic WebID from persona b"bob"
    pub fn bob() -> WebID {
        WebID::from_persona(b"bob")
    }

    /// Deterministic WebID for test user "carol".
    ///
    /// post: returns deterministic WebID from persona b"carol"
    pub fn carol() -> WebID {
        WebID::from_persona(b"carol")
    }

    /// Generate a new random WebID.
    ///
    /// post: returns new random WebID
    pub fn random() -> WebID {
        WebID::new()
    }

    /// Generate a WebID from arbitrary persona bytes.
    ///
    /// pre:  bytes is non-empty
    /// post: returns deterministic WebID from persona bytes
    pub fn from_persona(bytes: &[u8]) -> WebID {
        WebID::from_persona(bytes)
    }
}

// ── MockRegulationLedger ────────────────────────────────────────────────────────────

/// Regulation state for mock runtime — controllable in tests.
#[derive(Debug, Clone)]
pub struct MockRegState {
    pub homeostatic: bool,
    pub throttled_tools: Vec<String>,
    pub recent_signals: Vec<MockAlgedonicSignal>,
    pub variety_counters: HashMap<String, u64>,
}

impl MockRegState {
    /// Create a homeostatic (healthy) Regulation state.
    ///
    /// post: returns MockRegState with homeostatic=true, no throttled tools, empty signals
    pub fn homeostatic() -> Self {
        Self {
            homeostatic: true,
            throttled_tools: Vec::new(),
            recent_signals: Vec::new(),
            variety_counters: HashMap::new(),
        }
    }

    /// Create a perturbed Regulation state with a specific tool throttled.
    ///
    /// pre:  throttled_tool is non-empty
    /// post: returns MockRegState with homeostatic=false, tool throttled
    pub fn perturbed(throttled_tool: &str) -> Self {
        let mut state = Self::homeostatic();
        state.homeostatic = false;
        state.throttled_tools.push(throttled_tool.to_string());
        state
    }
}

/// Simplified algedonic signal for mock Regulation.
#[derive(Debug, Clone)]
pub struct MockAlgedonicSignal {
    pub valence: SignalValence,
    pub message: String,
    pub timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalValence {
    Positive,
    Negative,
    Neutral,
}

impl MockAlgedonicSignal {
    /// Check if signal has negative valence.
    ///
    /// post: returns true iff valence == Negative
    pub fn is_negative_valence(&self) -> bool {
        self.valence == SignalValence::Negative
    }

    /// Check if signal has positive valence.
    ///
    /// post: returns true iff valence == Positive
    pub fn is_positive_valence(&self) -> bool {
        self.valence == SignalValence::Positive
    }
}

/// Simplified Regulation runtime mock for integration tests.
///
/// Provides controllable state, event injection, time advancement,
/// and signal observation — sufficient for testing Regulation-dependent code
/// without a full running Regulation daemon.
#[derive(Clone)]
pub struct MockRegulationLedger {
    state: Arc<RwLock<MockRegState>>,
}

impl MockRegulationLedger {
    /// Create a new mock Regulation runtime with homeostatic state.
    ///
    /// post: returns MockRegulationLedger with homeostatic state
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(MockRegState::homeostatic())),
        }
    }

    /// Create a mock Regulation with a specific initial state.
    ///
    /// pre:  state is a valid MockRegState
    /// post: returns MockRegulationLedger with the given state
    pub fn with_state(state: MockRegState) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
        }
    }

    /// Inject an event into the Regulation (simulates a perturbation).
    ///
    /// pre:  event is a valid RegulationRecord
    /// post: homeostatic set to false, negative signal appended
    pub fn inject(&self, event: RegulationRecord) {
        let mut state = self.state.write().unwrap();
        state.homeostatic = false;
        let signal = MockAlgedonicSignal {
            valence: SignalValence::Negative,
            message: format!("event: {:?}", event.span),
            timestamp: Utc::now(),
        };
        state.recent_signals.push(signal);
    }

    /// Advance mock time by a duration (simulates feedback processing).
    /// After sufficient time, the Regulation may return toward homeostasis.
    ///
    /// post: if duration >= 5s, homeostatic restored, throttled tools cleared, positive signal appended
    pub fn advance_time(&self, duration: std::time::Duration) {
        let mut state = self.state.write().unwrap();
        // After 5+ seconds, system trends toward homeostasis
        if duration >= std::time::Duration::from_secs(5) {
            state.homeostatic = true;
            state.throttled_tools.clear();
            let signal = MockAlgedonicSignal {
                valence: SignalValence::Positive,
                message: "homeostasis restored".to_string(),
                timestamp: Utc::now(),
            };
            state.recent_signals.push(signal);
        }
    }

    /// Get recent algedonic signals.
    ///
    /// post: returns clone of recent_signals vector
    pub fn recent_signals(&self) -> Vec<MockAlgedonicSignal> {
        self.state.read().unwrap().recent_signals.clone()
    }

    /// Check if a specific tool is throttled.
    ///
    /// pre:  tool_name is non-empty
    /// post: returns Throttled if tool in throttled_tools, Active otherwise
    pub fn tool_state(&self, tool_name: &str) -> MockToolState {
        let state = self.state.read().unwrap();
        if state.throttled_tools.iter().any(|t| t == tool_name) {
            MockToolState::Throttled
        } else {
            MockToolState::Active
        }
    }

    /// Check if the Regulation is in homeostatic state.
    ///
    /// post: returns true iff homeostatic flag is true
    pub fn is_homeostatic(&self) -> bool {
        self.state.read().unwrap().homeostatic
    }

    /// Record variety for a domain (simulates tool dispatch).
    ///
    /// pre:  domain is non-empty
    /// post: variety counter for domain incremented by 1
    pub fn record_variety(&self, domain: &str) {
        let mut state = self.state.write().unwrap();
        *state
            .variety_counters
            .entry(domain.to_string())
            .or_insert(0) += 1;
    }

    /// Get variety count for a domain.
    ///
    /// pre:  domain is non-empty
    /// post: returns variety count for domain, 0 if never recorded
    pub fn variety_for_domain(&self, domain: &str) -> u64 {
        self.state
            .read()
            .unwrap()
            .variety_counters
            .get(domain)
            .copied()
            .unwrap_or(0)
    }
}

impl Default for MockRegulationLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool state as reported by mock Regulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockToolState {
    Active,
    Throttled,
}

// ── temp_dir ──────────────────────────────────────────────────────────────────

/// Create a guarded temp directory that auto-cleans on drop.
///
/// # Example
/// ```ignore
/// let dir = temp_dir();
/// let path = dir.path().join("test.txt");
/// std::fs::write(&path, b"data")?;
/// // dir and contents deleted when `dir` goes out of scope
/// ```
///
/// post: returns TempDir that auto-cleans on drop
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("temp dir creation should succeed")
}

// ── test_event ────────────────────────────────────────────────────────────────

/// Create a well-formed test RegulationRecord with required fields.
///
/// Uses a random observer WebID unless `observer` is provided.
///
/// # Example
/// ```ignore
/// let span = Span::new(SpanNamespace::new("reg.tool"), "invoked");
/// let event = test_event(span, CyclePhase::Observation, None);
/// assert!(event.observer_webid.as_uuid().is_set());
/// ```
///
/// pre:  span is a valid Span, phase is a valid Phase
/// post: returns RegulationRecord with random observer if observer is None, depth=0, test observation
pub fn test_event(span: Span, phase: CyclePhase, observer: Option<WebID>) -> RegulationRecord {
    RegulationRecord::new(
        observer.unwrap_or_else(TestWebId::random),
        span,
        phase,
        serde_json::json!({"test": true}),
        0,
    )
}

// ── test_h_mem ───────────────────────────────────────────────────────────────

/// Create a well-formed test HMem with required fields.
///
/// Uses a random owner WebID unless `owner` is provided.
///
/// # Example
/// ```ignore
/// let h_mem = test_h_mem("entity:test", "attribute:name", json!("value"), None);
/// assert_eq!(h_mem.entity, "entity:test");
/// ```
///
/// pre:  entity and attribute are non-empty, value is valid JSON
/// post: returns HMem with random owner if owner is None, specified owner otherwise
pub fn test_h_mem(entity: &str, attribute: &str, value: Value, owner: Option<WebID>) -> HMem {
    HMem::new(
        entity,
        attribute,
        value,
        owner.unwrap_or_else(TestWebId::random),
    )
}

// ── Internal helpers (not public) ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::event::SpanNamespace;

    #[test]
    fn test_db_creates_valid_database() {
        let db = TestDb::new();
        // Verify schema was initialized by querying a known table
        let result: Result<String, _> = db.conn().query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='hmems'",
            [],
            |row| row.get(0),
        );
        assert_eq!(result.unwrap(), "hmems");
    }

    #[test]
    fn test_keystore_creates_key_file() {
        let ks = TestKeystore::new();
        assert!(ks.key_path().exists());
        assert_eq!(ks.master_key().len(), 32);
        let stored = std::fs::read(ks.key_path()).unwrap();
        assert_eq!(stored.len(), 32);
    }

    #[test]
    fn test_webid_deterministic() {
        let a1 = TestWebId::alice();
        let a2 = TestWebId::alice();
        assert_eq!(a1, a2, "same persona must produce same WebID");

        let b = TestWebId::bob();
        assert_ne!(a1, b, "different personas must produce different WebIDs");
    }

    #[test]
    fn mock_reg_detects_perturbation() {
        let ledger = MockRegulationLedger::new();
        assert!(ledger.is_homeostatic());

        let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
        let event = test_event(span, CyclePhase::Sense, None);
        ledger.inject(event);

        assert!(!ledger.is_homeostatic());
        let signals = ledger.recent_signals();
        assert!(signals.iter().any(|s| s.is_negative_valence()));
    }

    #[test]
    fn mock_reg_restores_homeostasis() {
        let ledger = MockRegulationLedger::new();
        let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
        ledger.inject(test_event(span, CyclePhase::Sense, None));
        assert!(!ledger.is_homeostatic());

        ledger.advance_time(std::time::Duration::from_secs(10));
        assert!(ledger.is_homeostatic());
        let signals = ledger.recent_signals();
        assert!(signals.iter().any(|s| s.is_positive_valence()));
    }

    #[test]
    fn temp_dir_is_usable() {
        let dir = temp_dir();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello").unwrap();
        assert!(file_path.exists());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello");
    }

    #[test]
    fn test_event_is_valid() {
        let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
        let event = test_event(span, CyclePhase::Sense, None);
        assert!(!event.id.as_uuid().is_nil());
        assert!(!event.observer_webid.as_uuid().is_nil());
        assert_eq!(event.recursion_depth, 0);
    }

    #[test]
    fn test_hmem_is_valid() {
        let h_mem = test_h_mem("entity:test", "attr:name", serde_json::json!("value"), None);
        assert_eq!(h_mem.entity, "entity:test");
        assert_eq!(h_mem.attribute, "attr:name");
        assert_eq!(h_mem.value, serde_json::json!("value"));
        assert!(!h_mem.id.as_uuid().is_nil());
    }
}
