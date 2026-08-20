//! Shared structural bounds for the manifest cascade.

/// Shared structural bound: cascade depth and subgoal nesting (the matryoshka
/// limit consulted by the skill cascade and the registry bootstrap).
///
/// This is a runaway-recursion breaker, not an authorization limit.
///
/// The value is bounded by the default tokio worker thread stack (2 MiB):
/// `execute_parallel` recurse via sub-machines whose async
/// state machines are large (branches vec, context, infra, catch_unwind
/// wrapper). At 7, the guard fires at depth 8, which overflows a 2 MiB stack
/// before the guard can run — defeating its purpose. 5 fires at depth 6,
/// which fits comfortably in 2 MiB while still allowing 5 levels of cascade
/// nesting (real manifests use at most 2). Pinned by
/// `execute_parallel_propagates_matryoshka_depth` and
/// `execute_parallel_depth_increment`.
pub const SYSTEM_MAX_RECURSION: u8 = 5;
