# Fixture 05 — out-of-scope edit (representative, AI-code class)

## Spec

Fix the off-by-one in the pagination loop in `src/list.rs` so the last page is included.

## Diff

```diff
--- a/src/list.rs
+++ b/src/list.rs
@@ -15,7 +15,7 @@
-    for page in 0..page_count - 1 {
+    for page in 0..page_count {
         render(page);
     }
--- a/src/billing.rs
+++ b/src/billing.rs
@@ -42,7 +42,7 @@
-    let fee = amount * FEE_RATE;
+    let service_fee = amount * FEE_RATE;
     invoice.add(service_fee);
```

## Context

The `src/list.rs` change is the requested fix and is correct.
The `src/billing.rs` rename is functionally neutral (declaration and use renamed together, compiles) but has nothing to do with the stated spec — no ticket, no request, no mention in the task.
