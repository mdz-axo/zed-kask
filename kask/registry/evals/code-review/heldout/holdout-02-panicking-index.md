# Held-out Fixture 02 — panicking index on external data (held out: never tune against these)

## Spec

Return the first order's total in `src/report.rs`.

## Diff

```diff
--- a/src/report.rs
+++ b/src/report.rs
@@ -14,6 +14,10 @@
+pub fn first_total(orders: &[Order]) -> Cents {
+    let first = orders[0];
+    first.total()
+}
```

## Context

`orders` arrives from external JSON and may legitimately be empty. Repo rule: no panicking indexing — bounds-check external data. An empty list panics here.
