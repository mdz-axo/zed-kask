//! Shared structural bounds for the manifest cascade.

/// Shared structural bound: cascade depth and subgoal nesting (the matryoshka
/// limit consulted by the manifest executor and the registry bootstrap).
///
/// This is a runaway-recursion breaker, not an authorization limit.
pub const SYSTEM_MAX_RECURSION: u8 = 7;
