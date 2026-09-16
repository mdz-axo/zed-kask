//! In-memory generation job controller — tracks and bounds async media jobs.
//!
//! The controller is ephemeral (not persisted to SQLite). Persistent lineage is
//! already handled by `gallery_record_generation` / `gallery_lineage`. A
//! missing record is therefore surfaced as possible server restart data loss.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tokio_util::sync::CancellationToken;

use crate::types::JobRecord;

pub const MAX_CONCURRENT_HEAVY_OPERATIONS: usize = 4;
pub const MAX_TERMINAL_RECORDS: usize = 256;

/// One fail-fast capacity authority shared by direct heavy RPCs and background jobs.
#[derive(Clone)]
pub struct HeavyOperationAdmission {
    slots: Arc<Semaphore>,
}

#[derive(Debug)]
pub struct HeavyOperationPermit {
    _permit: OwnedSemaphorePermit,
}

impl HeavyOperationAdmission {
    fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(MAX_CONCURRENT_HEAVY_OPERATIONS)),
        }
    }

    fn try_admit(&self) -> Result<HeavyOperationPermit, JobStoreError> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|error| match error {
                tokio::sync::TryAcquireError::NoPermits => JobStoreError::Overloaded {
                    limit: MAX_CONCURRENT_HEAVY_OPERATIONS,
                },
                tokio::sync::TryAcquireError::Closed => JobStoreError::AdmissionClosed,
            })?;
        Ok(HeavyOperationPermit { _permit: permit })
    }
}

/// Shared controller for records, cancellation, admission, and retention.
pub type JobStore = Arc<JobController>;

pub struct JobController {
    records: Mutex<HashMap<String, JobRecord>>,
    active: Mutex<HashMap<String, ActiveJob>>,
    admission: HeavyOperationAdmission,
    #[cfg(test)]
    publication_gate: Mutex<Option<TestPublicationGate>>,
}

struct ActiveJob {
    token: CancellationToken,
    completion: watch::Receiver<bool>,
}

#[cfg(test)]
struct TestPublicationGate {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

#[derive(Debug, thiserror::Error)]
pub enum JobStoreError {
    #[error("media job capacity exhausted: at most {limit} jobs may be active")]
    Overloaded { limit: usize },
    #[error("job store {part} lock poisoned: {detail}")]
    LockPoisoned { part: &'static str, detail: String },
    #[error("job not found: {0}")]
    NotFound(String),
    #[error("job {job_id} is already {status} — cannot cancel")]
    AlreadyTerminal { job_id: String, status: String },
    #[error("job already exists: {0}")]
    Duplicate(String),
    #[error("media job admission controller is closed")]
    AdmissionClosed,
    #[error("active job has no cancellation control: {0}")]
    MissingCancellationControl(String),
    #[error("job {0} ended without signalling completion")]
    CompletionSignalClosed(String),
    #[error("job {job_id} cancellation cleanup failed: {detail}")]
    CancellationCleanupFailed { job_id: String, detail: String },
}

pub enum JobOutcome {
    Completed(serde_json::Value),
    Failed(String),
    Cancelled,
    CancellationFailed(String),
}

/// Owns one concurrency permit until the admitted task reaches cleanup.
pub struct JobLease {
    store: JobStore,
    pub job_id: String,
    token: CancellationToken,
    completion: watch::Sender<bool>,
    permit: Option<HeavyOperationPermit>,
    finished: bool,
}

impl JobController {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            admission: HeavyOperationAdmission::new(),
            #[cfg(test)]
            publication_gate: Mutex::new(None),
        }
    }

    /// Return cloned records without exposing the controller's mutable representation.
    pub fn list(&self) -> Result<Vec<JobRecord>, JobStoreError> {
        self.records
            .lock()
            .map(|records| records.values().cloned().collect())
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })
    }

    /// Return one cloned record without holding the controller lock across callers.
    pub fn get(&self, job_id: &str) -> Result<Option<JobRecord>, JobStoreError> {
        self.records
            .lock()
            .map(|records| records.get(job_id).cloned())
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })
    }

    #[cfg(test)]
    pub(crate) fn insert_record_for_test(&self, record: JobRecord) -> Result<(), JobStoreError> {
        let mut records = self
            .records
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })?;
        if records.contains_key(&record.id) {
            return Err(JobStoreError::Duplicate(record.id));
        }
        records.insert(record.id.clone(), record);
        Ok(())
    }

    /// Admit a direct heavy operation only when a shared slot is immediately available.
    pub fn admit_direct(&self) -> Result<HeavyOperationPermit, JobStoreError> {
        self.admission.try_admit()
    }

    /// Admit a job only when it can immediately own one of the shared bounded slots.
    pub fn admit(self: &Arc<Self>, record: JobRecord) -> Result<JobLease, JobStoreError> {
        let permit = self.admission.try_admit()?;
        let job_id = record.id.clone();
        let token = CancellationToken::new();
        let (completion, completion_rx) = watch::channel(false);
        let mut active = self
            .active
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "active",
                detail: error.to_string(),
            })?;
        let mut records = self
            .records
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })?;
        if records.contains_key(&job_id) {
            return Err(JobStoreError::Duplicate(job_id));
        }
        active.insert(
            job_id.clone(),
            ActiveJob {
                token: token.clone(),
                completion: completion_rx,
            },
        );
        records.insert(job_id.clone(), record);
        drop(active);
        drop(records);

        Ok(JobLease {
            store: self.clone(),
            job_id,
            token,
            completion,
            permit: Some(permit),
            finished: false,
        })
    }

    pub fn mark_running(&self, job_id: &str) -> Result<bool, JobStoreError> {
        let mut records = self
            .records
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })?;
        let job = records
            .get_mut(job_id)
            .ok_or_else(|| JobStoreError::NotFound(job_id.to_string()))?;
        if job.status == "queued" {
            job.status = "running".to_string();
            return Ok(true);
        }
        Ok(false)
    }

    /// Request cancellation and wait until task teardown has released its active slot.
    pub async fn cancel(&self, job_id: &str) -> Result<(), JobStoreError> {
        let mut completion = {
            let active = self
                .active
                .lock()
                .map_err(|error| JobStoreError::LockPoisoned {
                    part: "active",
                    detail: error.to_string(),
                })?;
            let mut records = self
                .records
                .lock()
                .map_err(|error| JobStoreError::LockPoisoned {
                    part: "records",
                    detail: error.to_string(),
                })?;
            let job = records
                .get_mut(job_id)
                .ok_or_else(|| JobStoreError::NotFound(job_id.to_string()))?;
            match job.status.as_str() {
                "queued" | "running" => {
                    let control = active.get(job_id).ok_or_else(|| {
                        JobStoreError::MissingCancellationControl(job_id.to_string())
                    })?;
                    job.status = "cancelling".to_string();
                    job.completed_at = None;
                    job.result = None;
                    job.error = None;
                    control.token.cancel();
                    control.completion.clone()
                }
                "cancelling" => active
                    .get(job_id)
                    .ok_or_else(|| JobStoreError::MissingCancellationControl(job_id.to_string()))?
                    .completion
                    .clone(),
                "completed" | "failed" | "cancelled" => {
                    return Err(JobStoreError::AlreadyTerminal {
                        job_id: job_id.to_string(),
                        status: job.status.clone(),
                    });
                }
                status => {
                    return Err(JobStoreError::AlreadyTerminal {
                        job_id: job_id.to_string(),
                        status: status.to_string(),
                    });
                }
            }
        };

        let is_complete = *completion.borrow();
        if !is_complete {
            completion
                .changed()
                .await
                .map_err(|_| JobStoreError::CompletionSignalClosed(job_id.to_string()))?;
        }
        let job = self
            .get(job_id)?
            .ok_or_else(|| JobStoreError::NotFound(job_id.to_string()))?;
        if job.status == "cancelled" {
            return Ok(());
        }
        Err(JobStoreError::CancellationCleanupFailed {
            job_id: job_id.to_string(),
            detail: job
                .error
                .unwrap_or_else(|| format!("job ended as {}", job.status)),
        })
    }

    pub fn active_count(&self) -> Result<usize, JobStoreError> {
        self.active
            .lock()
            .map(|active| active.len())
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "active",
                detail: error.to_string(),
            })
    }

    fn finish_published(
        &self,
        job_id: &str,
        result: serde_json::Value,
        publication: &mut crate::assets::StagedJobPublication,
    ) -> Result<bool, JobStoreError> {
        let mut active = self
            .active
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "active",
                detail: error.to_string(),
            })?;
        let mut records = self
            .records
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })?;
        let job = records
            .get_mut(job_id)
            .ok_or_else(|| JobStoreError::NotFound(job_id.to_string()))?;
        if job.status == "cancelling" {
            return Ok(false);
        }
        if !matches!(job.status.as_str(), "queued" | "running") {
            return Err(JobStoreError::AlreadyTerminal {
                job_id: job_id.to_string(),
                status: job.status.clone(),
            });
        }

        publication.commit();
        job.status = "completed".to_string();
        job.completed_at = Some(hkask_types::time::now_rfc3339());
        job.result = Some(result);
        job.error = None;
        active.remove(job_id);
        let active_ids = active.keys().cloned().collect::<Vec<_>>();
        prune_terminal_records(&mut records, &active_ids);
        Ok(true)
    }

    fn finish_job(&self, job_id: &str, outcome: JobOutcome) -> Result<(), JobStoreError> {
        let mut active = self
            .active
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "active",
                detail: error.to_string(),
            })?;
        let mut records = self
            .records
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "records",
                detail: error.to_string(),
            })?;
        let job = records
            .get_mut(job_id)
            .ok_or_else(|| JobStoreError::NotFound(job_id.to_string()))?;
        if job.status != "cancelled" {
            let outcome = if job.status == "cancelling"
                && !matches!(&outcome, JobOutcome::CancellationFailed(_))
            {
                JobOutcome::Cancelled
            } else {
                outcome
            };
            job.completed_at = Some(hkask_types::time::now_rfc3339());
            match outcome {
                JobOutcome::Completed(result) => {
                    job.status = "completed".to_string();
                    job.result = Some(result);
                    job.error = None;
                }
                JobOutcome::Failed(error) => {
                    job.status = "failed".to_string();
                    job.result = None;
                    job.error = Some(error);
                }
                JobOutcome::Cancelled => {
                    job.status = "cancelled".to_string();
                    job.result = None;
                    job.error = None;
                }
                JobOutcome::CancellationFailed(error) => {
                    job.status = "failed".to_string();
                    job.result = None;
                    job.error = Some(error);
                }
            }
        }
        active.remove(job_id);
        let active_ids = active.keys().cloned().collect::<Vec<_>>();
        prune_terminal_records(&mut records, &active_ids);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_publication_gate_for_test(
        &self,
        entered: Arc<Semaphore>,
        release: Arc<Semaphore>,
    ) -> Result<(), JobStoreError> {
        let mut gate =
            self.publication_gate
                .lock()
                .map_err(|error| JobStoreError::LockPoisoned {
                    part: "publication gate",
                    detail: error.to_string(),
                })?;
        *gate = Some(TestPublicationGate { entered, release });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn publication_checkpoint_for_test(&self) -> Result<(), JobStoreError> {
        let gate = self
            .publication_gate
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "publication gate",
                detail: error.to_string(),
            })?
            .as_ref()
            .map(|gate| (gate.entered.clone(), gate.release.clone()));
        if let Some((entered, release)) = gate {
            entered.add_permits(1);
            let permit = release
                .acquire_owned()
                .await
                .map_err(|_| JobStoreError::AdmissionClosed)?;
            permit.forget();
        }
        Ok(())
    }
}

impl JobLease {
    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Atomically let completion or cancellation win while publication remains rollback-armed.
    pub(crate) fn finish_published(
        &mut self,
        result: serde_json::Value,
        publication: &mut crate::assets::StagedJobPublication,
    ) -> Result<bool, JobStoreError> {
        if !self
            .store
            .finish_published(&self.job_id, result, publication)?
        {
            return Ok(false);
        }
        self.finished = true;
        self.permit.take();
        self.completion.send_replace(true);
        Ok(true)
    }

    pub fn finish(mut self, outcome: JobOutcome) -> Result<(), JobStoreError> {
        self.store.finish_job(&self.job_id, outcome)?;
        self.finished = true;
        self.permit.take();
        self.completion.send_replace(true);
        Ok(())
    }
}

impl Drop for JobLease {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Err(error) = self.store.finish_job(
            &self.job_id,
            JobOutcome::Failed("job task terminated unexpectedly (panic or abort)".to_string()),
        ) {
            tracing::warn!(
                target: "hkask.mcp.media.jobs",
                job_id = %self.job_id,
                error = %error,
                "Failed to terminalize dropped media job"
            );
        }
        self.permit.take();
        self.completion.send_replace(true);
    }
}

pub fn is_terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn prune_terminal_records(records: &mut HashMap<String, JobRecord>, active_ids: &[String]) {
    while records
        .values()
        .filter(|job| is_terminal_status(&job.status) && !active_ids.contains(&job.id))
        .count()
        > MAX_TERMINAL_RECORDS
    {
        let oldest = records
            .values()
            .filter(|job| is_terminal_status(&job.status) && !active_ids.contains(&job.id))
            .min_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.cmp(&right.id))
            })
            .map(|job| job.id.clone());
        let Some(oldest) = oldest else {
            break;
        };
        records.remove(&oldest);
    }
}

/// Create a new empty ephemeral job controller.
pub fn new_job_store() -> JobStore {
    Arc::new(JobController::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: usize) -> JobRecord {
        JobRecord {
            id: format!("job-{id:03}"),
            op: "generate_image".to_string(),
            status: "queued".to_string(),
            created_at: format!("2026-09-13T00:{id:03}:00Z"),
            completed_at: None,
            result: None,
            error: None,
        }
    }

    /// expect: A fifth media job is visibly rejected while four admitted jobs remain active.
    #[test]
    fn admission_is_bounded_at_four_active_jobs() -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let leases = (0..MAX_CONCURRENT_HEAVY_OPERATIONS)
            .map(|id| store.admit(record(id)))
            .collect::<Result<Vec<_>, _>>()?;

        let error = match store.admit(record(MAX_CONCURRENT_HEAVY_OPERATIONS)) {
            Ok(_) => return Err("the fifth active job was admitted".into()),
            Err(error) => error,
        };
        assert!(matches!(error, JobStoreError::Overloaded { limit: 4 }));
        assert_eq!(leases.len(), MAX_CONCURRENT_HEAVY_OPERATIONS);
        Ok(())
    }

    /// expect: Direct heavy calls and jobs exhaust one shared four-operation capacity.
    #[test]
    fn direct_and_job_admission_share_one_fail_fast_capacity()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let direct_leases = (0..3)
            .map(|_| store.admit_direct())
            .collect::<Result<Vec<_>, _>>()?;
        let job = store.admit(record(1))?;

        let direct_error = store
            .admit_direct()
            .expect_err("the fifth combined operation was admitted directly");
        assert!(matches!(
            direct_error,
            JobStoreError::Overloaded { limit: 4 }
        ));
        let job_error = match store.admit(record(2)) {
            Ok(_) => return Err("the fifth combined operation was admitted as a job".into()),
            Err(error) => error,
        };
        assert!(matches!(job_error, JobStoreError::Overloaded { limit: 4 }));

        drop(direct_leases);
        let replacement = store.admit(record(2))?;
        assert_eq!(replacement.job_id, "job-002");
        drop(job);
        Ok(())
    }

    /// expect: Job history remains bounded without deleting active work.
    #[test]
    fn retention_evicts_only_the_oldest_terminal_records() -> Result<(), Box<dyn std::error::Error>>
    {
        let store = new_job_store();
        let active = store.admit(record(999))?;

        for id in 0..=MAX_TERMINAL_RECORDS {
            store
                .admit(record(id))?
                .finish(JobOutcome::Completed(serde_json::json!({"id": id})))?;
        }

        let records = store.list()?;
        let terminal_count = records
            .iter()
            .filter(|job| is_terminal_status(&job.status))
            .count();
        assert_eq!(terminal_count, MAX_TERMINAL_RECORDS);
        assert!(records.iter().any(|job| job.id == active.job_id));
        assert!(!records.iter().any(|job| job.id == "job-000"));
        Ok(())
    }

    /// expect: Once cancellation is visible, later task cleanup cannot rewrite it.
    #[tokio::test]
    async fn cancelled_state_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let lease = store.admit(record(1))?;
        let token = lease.cancellation_token();
        let task = tokio::spawn(async move {
            token.cancelled().await;
            lease.finish(JobOutcome::Completed(
                serde_json::json!({"unexpected": true}),
            ))
        });
        store.cancel("job-001").await?;
        task.await??;

        let job = store
            .get("job-001")?
            .ok_or_else(|| "cancelled record was not retained".to_string())?;
        assert_eq!(job.status, "cancelled");
        assert!(job.result.is_none());
        Ok(())
    }

    /// expect: Dropping an unfinished task lease cannot leave a job active forever.
    #[test]
    fn dropped_lease_marks_job_failed() -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let lease = store.admit(record(1))?;
        store.mark_running(&lease.job_id)?;
        drop(lease);

        let job = store
            .get("job-001")?
            .ok_or_else(|| "failed record was not retained".to_string())?;
        assert_eq!(job.status, "failed");
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("panic or abort"))
        );
        Ok(())
    }

    /// expect: A panic in the actual spawned task records failure and immediately frees capacity.
    #[tokio::test]
    async fn spawned_task_panic_marks_failed_and_releases_slot()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let lease = store.admit(record(1))?;
        store.mark_running(&lease.job_id)?;
        let handle = tokio::spawn(async move {
            let _lease = lease;
            panic!("intentional media job panic");
        });
        assert!(handle.await.is_err());

        let job = store
            .get("job-001")?
            .ok_or_else(|| "panicked job record missing".to_string())?;
        assert_eq!(job.status, "failed");
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("panic or abort"))
        );
        assert_eq!(store.active_count()?, 0);
        let replacement = store.admit(record(2))?;
        assert_eq!(replacement.job_id, "job-002");
        Ok(())
    }

    /// expect: Aborting the actual spawned task records failure and immediately frees capacity.
    #[tokio::test]
    async fn aborted_join_handle_marks_failed_and_releases_slot()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let lease = store.admit(record(1))?;
        store.mark_running(&lease.job_id)?;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let _lease = lease;
            if entered_tx.send(()).is_err() {
                return;
            }
            std::future::pending::<()>().await;
        });
        entered_rx.await?;
        handle.abort();
        let join_error = handle
            .await
            .expect_err("aborted media task must report a cancelled join");
        assert!(join_error.is_cancelled());

        let job = store
            .get("job-001")?
            .ok_or_else(|| "aborted job record missing".to_string())?;
        assert_eq!(job.status, "failed");
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("panic or abort"))
        );
        assert_eq!(store.active_count()?, 0);
        let replacement = store.admit(record(2))?;
        assert_eq!(replacement.job_id, "job-002");
        Ok(())
    }
}
