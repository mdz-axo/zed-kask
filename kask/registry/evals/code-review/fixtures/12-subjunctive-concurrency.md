# Fixture 12 — subjunctive concurrency claim (boundary: provenance ceiling)

## Spec

Make the counter in `src/metrics.rs` usable from multiple tasks.

## Diff

```diff
--- a/src/metrics.rs
+++ b/src/metrics.rs
@@ -6,6 +6,12 @@
+pub fn record_hit(counter: &AtomicU64) {
+    // Current callers are single-threaded (see task #12); revisit if that changes.
+    let current = counter.load(Ordering::Relaxed);
+    counter.store(current + 1, Ordering::Relaxed);
+}
```

## Context

The load-then-store pattern loses updates if two tasks call `record_hit` concurrently. Today every caller is single-threaded (the comment is accurate), so the code is correct as shipped — the defect claim ("may race under future concurrent callers") is subjunctive: it describes a future state, not a present bug. `fetch_add` would be the fix.
