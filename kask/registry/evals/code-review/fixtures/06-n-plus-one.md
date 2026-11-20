# Fixture 06 — N+1 query pattern (representative)

## Spec

Return order totals for a list of order ids in `src/report.rs`.

## Diff

```diff
--- a/src/report.rs
+++ b/src/report.rs
@@ -10,6 +10,16 @@
+pub async fn order_totals(repo: &Repo, ids: &[OrderId]) -> Result<Vec<Cents>, Error> {
+    let mut totals = Vec::with_capacity(ids.len());
+    for id in ids {
+        let order = repo.get_order(id).await?;
+        totals.push(order.total());
+    }
+    Ok(totals)
+}
```

## Context

`Repo::get_order` issues one database query per call. `Repo::get_orders(&[OrderId])` (batched, one query) exists and is used elsewhere for multi-order reads. Callers pass lists of 50–500 ids.
