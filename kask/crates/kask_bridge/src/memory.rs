//! `MemoryPort` adapter — bridges zed's thread completion to hKask memory (D6).
//!
//! The initial implementation is a logging no-op: it records the turn record
//! via `tracing` and returns `Ok(())`. The full hKask memory stack (SQLCipher,
//! episodic/semantic storage, consolidation, WebID mapping) is deferred until
//! the storage layer is available in-process and the zed-account → WebID
//! mapping is defined.
//!
//! The port is injected via a global hook (`agent::set_memory_port`) so the
//! `agent` crate doesn't depend on `kask_bridge`.

use hkask_types::{MemoryError, MemoryPort, TurnRecord};
use std::future::Future;
use std::pin::Pin;

/// Logging no-op `MemoryPort` implementation.
///
/// Logs the turn record at `info` level and returns `Ok(())`.
/// This is the D6 placeholder — the full hKask memory wiring is deferred.
pub struct LoggingMemoryPort;

impl LoggingMemoryPort {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoggingMemoryPort {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPort for LoggingMemoryPort {
    fn ingest_turn<'a>(
        &'a self,
        record: TurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                target: "reg.memory",
                thread_id = %record.thread_id,
                model = %record.model,
                prompt_len = record.user_prompt.len(),
                response_len = record.agent_response.len(),
                title = ?record.thread_title,
                "Turn ingested into memory (logging no-op — full hKask memory wiring deferred)"
            );
            Ok(())
        })
    }
}

/// Adapter that implements the `agent` crate's `ThreadMemoryPort` trait
/// by delegating to an `hkask_types::MemoryPort`.
///
/// This is the bridge between the `agent` crate's local trait (which can't
/// depend on `hkask-types`) and the hKask `MemoryPort` trait.
pub struct BridgeMemoryPort {
    inner: std::sync::Arc<dyn MemoryPort>,
}

impl BridgeMemoryPort {
    pub fn new(inner: std::sync::Arc<dyn MemoryPort>) -> Self {
        Self { inner }
    }
}

impl agent::ThreadMemoryPort for BridgeMemoryPort {
    fn ingest_turn(
        &self,
        record: agent::ThreadTurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            inner
                .ingest_turn(TurnRecord {
                    thread_id: record.thread_id,
                    user_prompt: record.user_prompt,
                    agent_response: record.agent_response,
                    model: record.model,
                    thread_title: record.thread_title,
                })
                .await
                .map_err(|e| e.to_string())
        })
    }
}
