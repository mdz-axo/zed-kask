//! Shared test fixtures for the training server's inline test modules (the
//! smoke tests and the status contract tests) — the corpus
//! `corpus_test_server` precedent: one owner of the construction
//! choreography, the per-module host and port error at the call sites.

use crate::TrainingServer;
use crate::adapter::AdapterStore;
use crate::dataset::DatasetPipeline;
use crate::providers::{TrainingHarnessId, TrainingHost, TrainingHostId};
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::ports::{InferenceError, InferencePort, InferenceResult};
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, WebID};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A no-op inference port — every `generate` call returns the constructed
/// error. The error value is the caller's (each module's variant is its
/// test's semantic); the trait stub is the fixture's. Recording ports keep
/// their own `generate` stubs (a real method inside a recording type).
pub(crate) struct NoopPort(pub InferenceError);

impl InferencePort for NoopPort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async { Err(self.0.clone()) })
    }
}

/// Build a `TrainingServer` over an in-memory SQLite adapter store — the
/// shared construction of the inline test fixtures. The host, the inference
/// port, and the pipeline cache label are the caller's; the null store,
/// passphrase, and job table (the inline tests never read them) and the
/// Runpod/Axolotl ids are the fixture's.
pub(crate) fn training_server_with(
    host: Box<dyn TrainingHost>,
    inference_port: Arc<dyn InferencePort>,
    pipeline_cache: &str,
) -> TrainingServer {
    let pool = SqliteDriver::in_memory_pool().expect("in-memory sqlite pool must construct");
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new(pool));
    let adapter_store = Arc::new(
        AdapterStore::from_driver(driver)
            .expect("AdapterStore::from_driver must succeed against a fresh in-memory pool"),
    );
    let pipeline = Mutex::new(DatasetPipeline::new(
        std::env::temp_dir().join(pipeline_cache),
    ));
    TrainingServer::new(
        WebID::new(),
        // store: no MemoryStore — the inline tests never read it.
        None,
        // db_passphrase: no encrypted DB — the inline tests never read it.
        None,
        host,
        TrainingHostId::Runpod,
        TrainingHarnessId::Axolotl,
        pipeline,
        adapter_store,
        // job_store: no persistent job table — the inline tests never read it.
        None,
        inference_port,
    )
}
