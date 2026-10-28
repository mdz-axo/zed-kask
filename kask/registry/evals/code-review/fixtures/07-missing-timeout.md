# Fixture 07 — missing timeout on external call (representative)

## Spec

Call the inventory service from `src/checkout.rs` to reserve items.

## Diff

```diff
--- a/src/checkout.rs
+++ b/src/checkout.rs
@@ -30,6 +30,14 @@
+pub async fn reserve(items: &[Item]) -> Result<Reservation, Error> {
+    let response = HTTP.get(&format!("{INVENTORY_URL}/reserve"))
+        .json(&ReserveRequest::from(items))
+        .send().await?
+        .error_for_status()?;
+    Ok(response.json().await?)
+}
```

## Context

`INVENTORY_URL` is a remote service over the network. Every other outbound HTTP call in the codebase sets an explicit timeout (e.g. `.timeout(Duration::from_secs(5))`); the HTTP client's default is no timeout. Checkout is on the user-facing request path.
