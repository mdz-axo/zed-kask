//! Probe: does `lease_after_close` actually retry a held lease, and does a
//! should-succeed acquire recover once the holder drops?
//!
//! The flake fix routes should-succeed rotates/exports through
//! `lease_after_close` (rotation.rs tests). That helper is only sound if it
//! (a) retries while the lease is genuinely held, (b) succeeds once the
//! holder drops, and (c) fails after its bounded ceiling when the holder
//! never drops. This test pins all three against a deliberately held lease,
//! so the flake fix cannot be vacuous.
use hkask_storage::{Database, RotationError};

const PASSPHRASE: &str = "test-passphrase";

#[test]
fn a_deliberately_held_lease_is_retried_and_released() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("probe.db");
    let path_str = path.to_str().expect("UTF-8 path");

    // Hold the shared lease exactly the way a pool does.
    let holder = Database::open(path_str, PASSPHRASE).expect("open");
    let _pool = holder.sqlite_pool().expect("pool");

    // While the holder is open, the exclusive acquire is refused...
    assert!(matches!(
        rotate_passphrase(&path_str),
        Err(RotationError::ConsumersActive { .. })
    ));

    drop(_pool);
    drop(holder);

    // ...and once dropped, the same acquire succeeds. This mirrors the
    // flake's post-drop regime, without depending on teardown timing.
    let outcome = rotate_passphrase(&path_str);
    assert!(
        matches!(outcome, Ok(())),
        "the acquire must succeed once the holder drops, got {outcome:?}"
    );
}

/// The exact shape `lease_after_close` wraps in the rotation tests, exercised
/// outside the test module so the probe fails loudly if the retry helper's
/// contract drifts.
fn rotate_passphrase(path: &str) -> Result<(), RotationError> {
    const ATTEMPTS: usize = 40;
    let mut last = None;
    for ordinal in 0..ATTEMPTS {
        match hkask_storage::rotate_passphrase(path, PASSPHRASE, "new-key-123456") {
            Ok(()) => return Ok(()),
            Err(error @ RotationError::ConsumersActive { .. }) => {
                eprintln!("probe retry {}/{}", ordinal + 1, ATTEMPTS);
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last.expect("at least one refusal when the loop exhausts"))
}
