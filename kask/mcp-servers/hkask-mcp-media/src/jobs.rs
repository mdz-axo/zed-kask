//! In-memory generation job controller — tracks and bounds async media jobs.
//!
//! The controller is ephemeral (not persisted to SQLite). Persistent lineage is
//! already handled by `gallery_record_generation` / `gallery_lineage`. A
//! missing record is therefore surfaced as possible server restart data loss.

use std::collections::HashMap;
use std::sync::{Arc, LockResult, Mutex, MutexGuard};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::types::JobRecord;

pub const MAX_CONCURRENT_JOBS: usize = 4;
pub const MAX_TERMINAL_RECORDS: usize = 256;

/// Shared controller for records, cancellation, admission, and retention.
pub type JobStore = Arc<JobController>;

pub struct JobController {
    records: Mutex<HashMap<String, JobRecord>>,
    active: Mutex<HashMap<String, CancellationToken>>,
    slots: Arc<Semaphore>,
    #[cfg(test)]
    persistence_gate: Mutex<Option<TestPersistenceGate>>,
}

#[cfg(test)]
struct TestPersistenceGate {
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
}

pub enum JobOutcome {
    Completed(serde_json::Value),
    Failed(String),
    Cancelled,
}

/// Owns one concurrency permit until the admitted task reaches cleanup.
pub struct JobLease {
    store: JobStore,
    pub job_id: String,
    token: CancellationToken,
    permit: Option<OwnedSemaphorePermit>,
    finished: bool,
}

impl JobController {
    fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
            #[cfg(test)]
            persistence_gate: Mutex::new(None),
        }
    }

    /// Preserve direct record locking for existing read/list callers.
    pub fn lock(&self) -> LockResult<MutexGuard<'_, HashMap<String, JobRecord>>> {
        self.records.lock()
    }

    /// Admit a job only when it can immediately own one of the bounded slots.
    pub fn admit(self: &Arc<Self>, record: JobRecord) -> Result<JobLease, JobStoreError> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|error| match error {
                tokio::sync::TryAcquireError::NoPermits => JobStoreError::Overloaded {
                    limit: MAX_CONCURRENT_JOBS,
                },
                tokio::sync::TryAcquireError::Closed => JobStoreError::AdmissionClosed,
            })?;
        let job_id = record.id.clone();
        let token = CancellationToken::new();
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
        active.insert(job_id.clone(), token.clone());
        records.insert(job_id.clone(), record);
        drop(active);
        drop(records);

        Ok(JobLease {
            store: self.clone(),
            job_id,
            token,
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

    pub fn cancel(&self, job_id: &str) -> Result<(), JobStoreError> {
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
                let token = active
                    .get(job_id)
                    .ok_or_else(|| JobStoreError::MissingCancellationControl(job_id.to_string()))?;
                job.status = "cancelled".to_string();
                job.completed_at = Some(hkask_types::time::now_rfc3339());
                job.result = None;
                job.error = None;
                token.cancel();
                Ok(())
            }
            "completed" | "failed" | "cancelled" => Err(JobStoreError::AlreadyTerminal {
                job_id: job_id.to_string(),
                status: job.status.clone(),
            }),
            status => Err(JobStoreError::AlreadyTerminal {
                job_id: job_id.to_string(),
                status: status.to_string(),
            }),
        }
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
            }
        }
        active.remove(job_id);
        let active_ids = active.keys().cloned().collect::<Vec<_>>();
        prune_terminal_records(&mut records, &active_ids);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_persistence_gate_for_test(
        &self,
        entered: Arc<Semaphore>,
        release: Arc<Semaphore>,
    ) -> Result<(), JobStoreError> {
        let mut gate =
            self.persistence_gate
                .lock()
                .map_err(|error| JobStoreError::LockPoisoned {
                    part: "persistence gate",
                    detail: error.to_string(),
                })?;
        *gate = Some(TestPersistenceGate { entered, release });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn persistence_checkpoint_for_test(&self) -> Result<(), JobStoreError> {
        let gate = self
            .persistence_gate
            .lock()
            .map_err(|error| JobStoreError::LockPoisoned {
                part: "persistence gate",
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

    pub fn finish(mut self, outcome: JobOutcome) -> Result<(), JobStoreError> {
        self.store.finish_job(&self.job_id, outcome)?;
        self.finished = true;
        self.permit.take();
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
        let leases = (0..MAX_CONCURRENT_JOBS)
            .map(|id| store.admit(record(id)))
            .collect::<Result<Vec<_>, _>>()?;

        let error = match store.admit(record(MAX_CONCURRENT_JOBS)) {
            Ok(_) => return Err("the fifth active job was admitted".into()),
            Err(error) => error,
        };
        assert!(matches!(error, JobStoreError::Overloaded { limit: 4 }));
        assert_eq!(leases.len(), MAX_CONCURRENT_JOBS);
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

        let records = store.lock().map_err(|error| error.to_string())?;
        let terminal_count = records
            .values()
            .filter(|job| is_terminal_status(&job.status))
            .count();
        assert_eq!(terminal_count, MAX_TERMINAL_RECORDS);
        assert!(records.contains_key(&active.job_id));
        assert!(!records.contains_key("job-000"));
        Ok(())
    }

    /// expect: Once cancellation is visible, later task cleanup cannot rewrite it.
    #[test]
    fn cancelled_state_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        let store = new_job_store();
        let lease = store.admit(record(1))?;
        store.cancel(&lease.job_id)?;
        lease.finish(JobOutcome::Completed(
            serde_json::json!({"unexpected": true}),
        ))?;

        let records = store.lock().map_err(|error| error.to_string())?;
        let job = records
            .get("job-001")
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

        let records = store.lock().map_err(|error| error.to_string())?;
        let job = records
            .get("job-001")
            .ok_or_else(|| "failed record was not retained".to_string())?;
        assert_eq!(job.status, "failed");
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("panic or abort"))
        );
        Ok(())
    }
}
