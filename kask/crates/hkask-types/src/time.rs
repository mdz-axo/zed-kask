//! Time utilities — Cross-cutting infrastructure
//!
//! P4.3: `now_rfc3339()` consolidates the repeated `Utc::now().to_rfc3339()`
//! pattern across all crates. Lives in `hkask-types` (the foundation crate)
//! so CLI and storage can all use it without circular dependencies.

/// Produce an RFC 3339 timestamp string for the current moment.
///
/// This is the canonical helper for "now as a string" across hKask.
/// Prefer it over inlining `chrono::Utc::now().to_rfc3339()` so that
/// any future change to the timestamp format (e.g., adding nanosecond
/// precision, switching to a different underlying clock) propagates
/// uniformly across crates.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  (none — always callable, no arguments)
/// post: returns a valid RFC 3339 timestamp string for the current UTC moment
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Produce an RFC 3339 timestamp string for the current moment, using the
/// `Z` UTC-designator suffix (e.g. `"2026-08-20T12:00:00.000Z"`).
///
/// This exists alongside `now_rfc3339()` because the event store's retention
/// boundary cutoffs are written with a `Z` suffix (e.g. the far-future sentinel
/// `"9999-01-01T00:00:00Z"`). Retention (`compact`, `strip_bodies`) selects
/// rows by lexically comparing `created_at` against the cutoff, so the stored
/// timestamp MUST use the same suffix shape as the cutoff. Mixed suffixes
/// break lexical ordering: `'+'` (0x2B) sorts before `'Z'` (0x5A), so a
/// `+00:00`-suffixed `created_at` and a `Z`-suffixed cutoff of the same
/// instant would compare unequal, and the relative order of timestamps written
/// with different suffixes would no longer match chronological order. Using
/// the `Z` suffix everywhere keeps lexical comparison aligned with
/// chronological order against `Z`-suffix cutoffs. Prefer this helper for any
/// timestamp that will be compared against a `Z`-suffix retention cutoff.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  (none — always callable, no arguments)
/// post: returns a valid RFC 3339 timestamp string for the current UTC moment
///       with a `Z` UTC-designator suffix
pub fn now_rfc3339_z() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
