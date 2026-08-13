//! F4 regression tests — `resolve_under_data_dir` delegates to `resolve_data_dir`.
//!
//! `hkask-types` has `#![forbid(unsafe_code)]` unconditionally, so env-var
//! mutation (unsafe since Rust 2024) cannot live in the inline `mod tests`.
//! These integration tests are a separate compilation unit and can use the
//! `unsafe` `std::env::set_var` / `remove_var` required to exercise the
//! `HKASK_DATA_DIR` / `XDG_DATA_HOME` fallback chain.
//!
//! The bug (F4): `resolve_data_dir` and `resolve_under_data_dir` previously
//! duplicated the env-var fallback chain but diverged on the `HKASK_DATA_DIR`
//! rule — `resolve_data_dir` honored it only when absolute or `.`-prefixed,
//! while `resolve_under_data_dir` honored it unconditionally. A relative
//! `HKASK_DATA_DIR=foo` resolved to `foo` under one and
//! `$XDG_DATA_HOME/hkask/foo` under the other, splitting agent DBs across two
//! trees. The fix makes `resolve_under_data_dir` delegate to
//! `resolve_data_dir` so there is exactly one regulator.

#![allow(unsafe_code)]

use hkask_types::agent_paths::{resolve_data_dir, resolve_under_data_dir};

/// Serializes env-var-mutating tests so a `set_var` in one doesn't race a
/// `remove_var` in another.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Both helpers honor an absolute `HKASK_DATA_DIR`, and `resolve_under_data_dir`
/// produces `resolve_data_dir().join(relative)` — the single-regulator rule.
#[test]
fn resolve_under_data_dir_delegates_to_resolve_data_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let abs = tmp.path().to_path_buf();
    unsafe {
        std::env::set_var("HKASK_DATA_DIR", &abs);
    }
    let data_dir = resolve_data_dir();
    let under = resolve_under_data_dir(std::path::Path::new("threads/threads.db"));
    unsafe {
        std::env::remove_var("HKASK_DATA_DIR");
    }
    assert_eq!(
        data_dir, abs,
        "resolve_data_dir must honor absolute HKASK_DATA_DIR"
    );
    assert_eq!(
        under,
        abs.join("threads/threads.db"),
        "resolve_under_data_dir must delegate to resolve_data_dir (same HKASK_DATA_DIR rule)"
    );
}

/// A relative `HKASK_DATA_DIR` is treated as misconfig by both helpers (falls
/// through to XDG/HOME). Previously `resolve_under_data_dir` honored it
/// unconditionally, landing agent DBs in an arbitrary CWD. Pin the fix: both
/// helpers now apply the same (reject-relative) rule.
#[test]
fn resolve_under_data_dir_ignores_relative_hkask_data_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");
    let tmp = tempfile::TempDir::new().expect("tempdir");
    unsafe {
        std::env::set_var("HKASK_DATA_DIR", "relative-not-absolute");
        std::env::set_var("XDG_DATA_HOME", tmp.path());
    }
    let data_dir = resolve_data_dir();
    let under = resolve_under_data_dir(std::path::Path::new("x"));
    unsafe {
        std::env::remove_var("HKASK_DATA_DIR");
        std::env::remove_var("XDG_DATA_HOME");
    }
    // Both must fall through to $XDG_DATA_HOME/hkask (relative HKASK_DATA_DIR rejected).
    assert_eq!(
        data_dir,
        tmp.path().join("hkask"),
        "resolve_data_dir must reject a relative HKASK_DATA_DIR"
    );
    assert_eq!(
        under,
        tmp.path().join("hkask").join("x"),
        "resolve_under_data_dir must apply the same rule as resolve_data_dir (reject relative)"
    );
}

/// A `.`-prefixed `HKASK_DATA_DIR` (e.g. `./data`) is honored by both helpers —
/// `resolve_data_dir`'s rule explicitly allows `.`-prefixed paths. Pin that
/// the delegation preserves this.
#[test]
fn resolve_under_data_dir_honors_dot_prefixed_hkask_data_dir() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");
    let tmp = tempfile::TempDir::new().expect("tempdir");
    // Use a `.`-prefixed absolute path by chdir-ing into tmp and using `./data`.
    let cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(tmp.path()).expect("chdir");
    unsafe {
        std::env::set_var("HKASK_DATA_DIR", "./data");
    }
    let data_dir = resolve_data_dir();
    let under = resolve_under_data_dir(std::path::Path::new("agents/alice/pod.db"));
    unsafe {
        std::env::remove_var("HKASK_DATA_DIR");
    }
    std::env::set_current_dir(cwd).expect("restore cwd");
    assert_eq!(
        data_dir,
        std::path::PathBuf::from("./data"),
        "resolve_data_dir must honor a .-prefixed HKASK_DATA_DIR"
    );
    assert_eq!(
        under,
        std::path::PathBuf::from("./data").join("agents/alice/pod.db"),
        "resolve_under_data_dir must delegate (.-prefixed path preserved)"
    );
}
