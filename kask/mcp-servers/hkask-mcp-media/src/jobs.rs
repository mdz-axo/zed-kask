//! In-memory generation job store — tracks async media generation jobs.
//!
//! The store is ephemeral (not persisted to SQLite). Persistent lineage is
//! already handled by `gallery_record_generation` / `gallery_lineage`. The
//! job store is for real-time queue visibility: which jobs are queued, running,
//! completed, or failed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::JobRecord;

/// Thread-safe in-memory job store.
pub type JobStore = Arc<Mutex<HashMap<String, JobRecord>>>;

/// Create a new empty job store.
pub fn new_job_store() -> JobStore {
    Arc::new(Mutex::new(HashMap::new()))
}
