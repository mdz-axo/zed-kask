//! Startup credential-recovery sweep — the actuator for the wedged-portal
//! class (the 2026-10-04, 2026-10-25 and 2026-10-26 incidents).
//!
//! Detection for this class is complete and typed: the keystore's keychain
//! reads are deadline-bounded and distinguish `Timeout` (the secret portal
//! may be wedged) and `Platform` (D-Bus/keyring failure) from `NotFound`
//! (the key genuinely does not exist). What was missing is the actuator.
//! Nothing ever re-read after a transient failure, and both startup retry
//! loops (the inference-stack re-wire, the edit-prediction port) are keyed
//! on registry events that never fire after a failed credential read. A
//! session whose startup sweep hit a wedged portal therefore stayed
//! degraded until an operator restart even when the portal recovered
//! within seconds (live 2026-10-25: the keyring respawned at 21:36:26 and
//! the 21:46:24 distillation pass still failed on the unresolved model).
//!
//! This module owns the sweep's decision core — outcome classification,
//! the bounded backoff schedule, the next-action state machine, and the
//! canary probe itself. The composition root (zed's main.rs) drives the
//! loop and acts on the decisions: it re-probes on a GPUI timer and, when
//! the portal recovers, re-fires `SettingsStore::notify_observers` — the
//! same push the credential-write settings-UI path uses — which
//! re-triggers every credential load: the provider stack's
//! `ApiKeyState::load_if_needed` retries from `NotPresent`,
//! `build_mcp_server_env` re-reads its env (its diff restarts affected
//! children), and the registry events the startup retry loops wait for
//! actually fire. All three were observed recovering through *incidental*
//! observer fires on 2026-10-26; this sweep arms the same recovery
//! deterministically.
//!
//! Honest-failure contract preserved: the sweep adds an actuator, never a
//! suppression. The startup warnings stand; the sweep logs its arming at
//! info, its retries at debug, and one final warn when its budget expires.

use std::time::Duration;

/// Delay before the sweep's first canary probe. On a healthy startup the
/// provider credentials resolve within seconds (the edit-prediction port
/// wires through the configured model in ~3s), so a probe this late reads a
/// healthy portal and disarms without ever firing a credential re-load.
pub const FIRST_PROBE_DELAY: Duration = Duration::from_secs(20);

/// The bounded backoff between probes once a transient failure is observed:
/// five retries whose delays sum to 750s (~12.5 minutes). Exhausting the
/// schedule is the final warn's condition — the session keeps its existing
/// degraded-state warnings either way.
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
    Duration::from_secs(300),
];

/// The canary credential slot for portal-reachability probing: the single
/// internal key (`kask://credentials/hkask_db_passphrase`, D5) — the most
/// load-bearing slot in the install (it gates every SQLCipher database) and
/// one first-run provisioning guarantees exists. Both live wedges were
/// portal-wide (every URL timed out together), so one slot's reachability
/// classifies the portal. The secret itself is never read by the sweep.
const CANARY_CREDENTIAL_KEY: &str = "hkask_db_passphrase";

/// How a canary keychain probe classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The portal answered and the canary key read.
    Healthy,
    /// The portal answered; the canary key itself is absent. A genuinely
    /// absent key is the operator's configuration state — the per-credential
    /// startup warnings own it; the sweep does not act on it.
    Absent,
    /// The portal did not answer within its deadline (`Timeout`) or failed
    /// at the platform layer (`Platform` — D-Bus, locked keyring): the
    /// wedged-portal class. The key may exist and recovery is possible, so
    /// the sweep arms.
    Transient,
}

impl ProbeOutcome {
    /// Classify a canary probe result. Generic over the success payload —
    /// the sweep never reads the secret, only the portal's reachability.
    pub fn classify<T>(result: &Result<T, hkask_keystore::KeychainError>) -> Self {
        match result {
            Ok(_) => Self::Healthy,
            Err(hkask_keystore::KeychainError::NotFound(_)) => Self::Absent,
            Err(hkask_keystore::KeychainError::Timeout { .. })
            | Err(hkask_keystore::KeychainError::Platform(_)) => Self::Transient,
        }
    }
}

/// What the sweep does after observing a probe outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Nothing to recover: the portal answered and no transient was seen.
    /// Disarm — the happy path never fires a credential re-load.
    Disarm,
    /// The portal answered after transient failures: re-fire the credential
    /// loads once (the observer notify), then disarm.
    FireNotify,
    /// The portal is still wedged: probe again after the delay. `None` means
    /// the retry budget is exhausted — emit the final warn and disarm.
    ProbeAgain(Option<Duration>),
}

/// The sweep's decision state: whether a transient failure has been observed
/// and how many retry delays have been consumed.
#[derive(Debug, Default)]
pub struct CredentialRecoverySweep {
    saw_transient: bool,
    retry_index: usize,
}

impl CredentialRecoverySweep {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a transient failure has been observed — the composition root
    /// logs the sweep's arming (info) vs its retries (debug) on this edge.
    pub fn saw_transient(&self) -> bool {
        self.saw_transient
    }

    /// Observe a canary probe outcome and decide the next action.
    pub fn observe(&mut self, outcome: ProbeOutcome) -> RecoveryAction {
        match outcome {
            ProbeOutcome::Healthy | ProbeOutcome::Absent => {
                if self.saw_transient {
                    // The portal recovered — including when the canary key
                    // is now absent: the keys that failed DURING the wedge
                    // may read, so the re-fire must happen either way.
                    RecoveryAction::FireNotify
                } else {
                    RecoveryAction::Disarm
                }
            }
            ProbeOutcome::Transient => {
                self.saw_transient = true;
                let delay = RETRY_DELAYS.get(self.retry_index).copied();
                self.retry_index += 1;
                RecoveryAction::ProbeAgain(delay)
            }
        }
    }
}

/// Probe the secret portal's reachability through the canary credential slot
/// and classify the outcome. The secret itself is dropped unread (its
/// `Zeroizing` wrapper wipes it on drop); only the portal's reachability
/// matters to the sweep.
pub async fn probe_canary_outcome() -> ProbeOutcome {
    let url = crate::inference_providers::credential_url_for_key(CANARY_CREDENTIAL_KEY);
    let result = hkask_keystore::Keychain.retrieve_by_url_async(&url).await;
    ProbeOutcome::classify(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_keystore::KeychainError;
    use hkask_types::NotFound;

    fn timeout_error() -> KeychainError {
        KeychainError::Timeout {
            key: "kask://credentials/hkask_db_passphrase".to_string(),
            timeout_secs: 5,
        }
    }

    fn platform_error() -> KeychainError {
        // The D-Bus failure observed live 2026-10-25: the keyring daemon
        // died under an in-flight read.
        KeychainError::Platform(
            "Message recipient disconnected from message bus without replying".to_string(),
        )
    }

    fn not_found_error() -> KeychainError {
        KeychainError::NotFound(NotFound {
            entity_type: "credential".to_string(),
            id: "kask://credentials/hkask_db_passphrase".to_string(),
        })
    }

    /// A healthy first probe never arms anything: no transient observed, no
    /// notify ever fires — the happy path costs one bounded read and
    /// disarms. This is every ordinary startup.
    #[test]
    fn healthy_first_probe_disarms_without_arming() {
        let mut sweep = CredentialRecoverySweep::new();
        let result: Result<String, _> = Ok("canary-secret".to_string());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&result)),
            RecoveryAction::Disarm
        );
    }

    /// A genuinely absent canary key is portal health, not a recovery
    /// condition: the operator's per-credential startup warnings own that
    /// state and the sweep must not treat it as recoverable.
    #[test]
    fn absent_canary_key_disarms_without_arming() {
        let mut sweep = CredentialRecoverySweep::new();
        let result: Result<String, _> = Err(not_found_error());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&result)),
            RecoveryAction::Disarm
        );
    }

    /// Both transient classes arm the sweep: the deadline expiry (the
    /// wedged-portal class, live 2026-10-25/26) and the platform failure
    /// (the D-Bus disconnect class, live 2026-10-25).
    #[test]
    fn timeout_and_platform_failures_arm_the_sweep() {
        let mut sweep = CredentialRecoverySweep::new();
        let timeout: Result<String, _> = Err(timeout_error());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&timeout)),
            RecoveryAction::ProbeAgain(Some(Duration::from_secs(30)))
        );

        let mut sweep = CredentialRecoverySweep::new();
        let platform: Result<String, _> = Err(platform_error());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&platform)),
            RecoveryAction::ProbeAgain(Some(Duration::from_secs(30)))
        );
    }

    /// Portal recovery after a transient arms exactly one credential
    /// re-fire — the actuator the 2026-10-25 session lacked.
    #[test]
    fn recovered_portal_after_transient_fires_notify_once() {
        let mut sweep = CredentialRecoverySweep::new();
        let transient: Result<String, _> = Err(timeout_error());
        assert!(matches!(
            sweep.observe(ProbeOutcome::classify(&transient)),
            RecoveryAction::ProbeAgain(_)
        ));
        let recovered: Result<String, _> = Ok("canary-secret".to_string());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&recovered)),
            RecoveryAction::FireNotify
        );
    }

    /// Recovery is portal-level, not key-level: the canary key turning out
    /// absent after a transient still means the portal answers again, so the
    /// keys that failed during the wedge get their re-fire.
    #[test]
    fn absent_canary_after_transient_still_fires_notify() {
        let mut sweep = CredentialRecoverySweep::new();
        let transient: Result<String, _> = Err(timeout_error());
        sweep.observe(ProbeOutcome::classify(&transient));
        let absent: Result<String, _> = Err(not_found_error());
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&absent)),
            RecoveryAction::FireNotify
        );
    }

    /// The retry budget is bounded: after the fifth transient the sweep
    /// returns `None` — the caller's final-warn condition — instead of
    /// probing forever.
    #[test]
    fn retry_budget_exhausts_after_five_retries() {
        let mut sweep = CredentialRecoverySweep::new();
        let transient: Result<String, _> = Err(timeout_error());
        for expected in RETRY_DELAYS {
            assert_eq!(
                sweep.observe(ProbeOutcome::classify(&transient)),
                RecoveryAction::ProbeAgain(Some(expected))
            );
        }
        assert_eq!(
            sweep.observe(ProbeOutcome::classify(&transient)),
            RecoveryAction::ProbeAgain(None)
        );
    }

    /// The first transient marks the sweep — the composition root uses this
    /// to log the arming at info and the retries at debug.
    #[test]
    fn saw_transient_flips_on_the_first_transient_only() {
        let mut sweep = CredentialRecoverySweep::new();
        assert!(!sweep.saw_transient());
        let transient: Result<String, _> = Err(timeout_error());
        sweep.observe(ProbeOutcome::classify(&transient));
        assert!(sweep.saw_transient());
        sweep.observe(ProbeOutcome::classify(&transient));
        assert!(sweep.saw_transient());
    }
}
