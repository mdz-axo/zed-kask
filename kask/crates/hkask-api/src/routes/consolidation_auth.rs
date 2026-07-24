//! Consolidation auth — rate limiting and passphrase verification.
//!
//! Moved from `hkask-memory::consolidation_auth` — these are API-layer auth
//! gates, not memory domain logic. The memory domain crate should not depend
//! on `hkask-services-core` or `hkask-keystore`; co-locating the auth helpers
//! with their sole consumer removes those dependency edges.
//!
//! # REQ: P2 (Affirmative Consent) — consolidation requires explicit consent.

use std::sync::atomic::{AtomicU64, Ordering};

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

/// Minimum seconds between consolidation requests.
///
/// This limits online passphrase guessing for an administrative operation that
/// should run at most a few times per session.
const CONSOLIDATION_MIN_INTERVAL_SECS: u64 = 30;

/// Coarse-grained rate limiter for consolidation requests.
///
/// Uses a single `AtomicU64` timestamp (epoch seconds). Intentionally simple —
/// one global gate, not per-user. For a single-user headless system, this is sufficient.
static LAST_CONSOLIDATION_EPOCH_SECS: AtomicU64 = AtomicU64::new(0);

/// Check the rate limit for consolidation requests.
///
/// pre:  none (always succeeds or returns rate-limit error)
/// post: Ok(()) if rate limit not exceeded; Err(RateLimited) with remaining seconds if within 30s window
pub(crate) fn check_rate_limit() -> Result<(), ServiceError> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let prev = LAST_CONSOLIDATION_EPOCH_SECS.load(Ordering::Relaxed);
    if prev != 0 && now_secs.saturating_sub(prev) < CONSOLIDATION_MIN_INTERVAL_SECS {
        let remaining = CONSOLIDATION_MIN_INTERVAL_SECS - now_secs.saturating_sub(prev);
        return Err(ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Infrastructure,
            source: None,
            message: format!("Rate limited: try again in {}s", remaining),
        });
    }
    LAST_CONSOLIDATION_EPOCH_SECS.store(now_secs, Ordering::Relaxed);
    Ok(())
}

/// Verify the consolidation passphrase against the keystore.
///
/// pre:  passphrase must be non-empty; server passphrase must be configured in keystore
/// post: returns the expected passphrase string on match; Err(Keystore) if not configured; Err(InvalidPassphrase) if mismatch
pub(crate) fn verify_passphrase(passphrase: &str) -> Result<String, ServiceError> {
    let expected = hkask_keystore::keychain::resolve_db_passphrase_string().map_err(|_| {
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Infrastructure,
            source: None,
            message: "Server passphrase not configured".into(),
        }
    })?;
    if passphrase != expected.as_str() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "Passphrase verification failed".into(),
        });
    }
    Ok(expected.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn configured_db_passphrase_authorizes_without_derivation() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("HKASK_DB_PASSPHRASE");
        unsafe { std::env::set_var("HKASK_DB_PASSPHRASE", "explicit-db-passphrase") };

        let accepted = verify_passphrase("explicit-db-passphrase");
        let rejected = verify_passphrase("different-passphrase");

        unsafe {
            match old {
                Some(value) => std::env::set_var("HKASK_DB_PASSPHRASE", value),
                None => std::env::remove_var("HKASK_DB_PASSPHRASE"),
            }
        }
        assert!(accepted.is_ok());
        assert!(rejected.is_err());
    }
}
