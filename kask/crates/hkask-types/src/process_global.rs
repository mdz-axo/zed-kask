//! The process-global hook slot — the one implementation of the re-settable
//! `set_*` composition-root pattern.
//!
//! kask wires cross-crate capabilities into upstream and leaf crates
//! through process-global hooks: the composition root (`main.rs`) calls
//! `set_*` once the required context exists (post-model-resolve,
//! panel-open, workspace-open), and consumers read the slot wherever they
//! run. Every hook in this family is a `Mutex<Option<T>>` slot —
//! re-settable, because the composition root upgrades values as context
//! resolves (logging port → real port, early condenser → model-aware
//! condenser).
//!
//! This type encodes the two invariants every hand-rolled copy had to
//! remember:
//!
//! - **Clone out of the lock.** `get` returns an owned clone, so no caller
//!   can hold the mutex across an await point or run a callback under it.
//! - **Recover from poison.** A panic in some other thread while the lock
//!   was held poisons the mutex; for a plain slot assignment the value is
//!   either written or not — there is no half-applied state — so recovery
//!   is safe and silent, and a poisoned accessor must not cascade the
//!   panic into whichever thread reads the hook next (the same failure
//!   class as the Tokio dispatch panic this family's newest hook fixed).
//!
//! The set-once family (`OnceLock` hooks such as
//! `agent::set_template_base_path`) is deliberately NOT this type: those
//! are configured once at startup, warn on the `Err` branch of `set`, and
//! never replace. Leaf crates that cannot depend on `hkask-types`
//! (`hkask-tool-invoker`, `hkask-conversation-injector`) hand-roll the same
//! two invariants — their doc comments point here.
//!
//! Per `.rules`: `Mutex` hooks are re-settable and do not need the
//! `Err`-branch warn that `OnceLock` hooks require.

use std::sync::MutexGuard;

/// A re-settable process-global slot: `Mutex<Option<T>>` plus the
/// clone-out-of-lock and poison-recovery invariants.
pub struct ProcessGlobal<T> {
    slot: std::sync::Mutex<Option<T>>,
}

impl<T: Clone> ProcessGlobal<T> {
    /// The empty slot. `const`, so a hook is a plain `static` declaration.
    pub const fn new() -> Self {
        Self {
            slot: std::sync::Mutex::new(None),
        }
    }

    /// Replace the slot's value (`None` clears it). Later calls replace
    /// earlier ones — the composition root's upgrade path.
    pub fn set(&self, value: Option<T>) {
        *self.lock() = value;
    }

    /// The slot's current value, as an owned clone — the caller never
    /// holds the lock.
    pub fn get(&self) -> Option<T> {
        self.lock().clone()
    }

    /// Lock the slot, recovering from poison: a poisoned lock carries no
    /// half-applied state for a plain slot, and cascading the panic would
    /// take down whichever thread happened to read the hook next.
    fn lock(&self) -> MutexGuard<'_, Option<T>> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl<T: Clone> Default for ProcessGlobal<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_replace_clear_roundtrip() {
        static SLOT: ProcessGlobal<u32> = ProcessGlobal::new();
        assert_eq!(SLOT.get(), None, "a fresh slot is empty");
        SLOT.set(Some(7));
        assert_eq!(SLOT.get(), Some(7));
        SLOT.set(Some(9));
        assert_eq!(SLOT.get(), Some(9), "set replaces — the upgrade path");
        SLOT.set(None);
        assert_eq!(SLOT.get(), None, "None clears");
    }

    /// A panic in another thread while the lock was held poisons the
    /// mutex. The slot's value is either written or not — no half-applied
    /// state — so accessors recover instead of cascading the panic into
    /// whichever thread reads the hook next.
    #[test]
    fn accessors_recover_from_poison() {
        static SLOT: ProcessGlobal<&'static str> = ProcessGlobal::new();
        SLOT.set(Some("wired"));
        let poisoner = std::thread::spawn(|| {
            let _guard = SLOT.lock();
            panic!("poison the slot lock");
        });
        assert!(poisoner.join().is_err(), "the poisoner must have panicked");
        assert_eq!(SLOT.get(), Some("wired"), "get recovers from poison");
        SLOT.set(Some("replaced"));
        assert_eq!(SLOT.get(), Some("replaced"), "set recovers from poison");
    }
}
