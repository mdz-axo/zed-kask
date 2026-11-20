# Fixture 01 — unwrap on a fallible op (representative)

## Spec

Add retry with backoff (max 3 attempts) to `fetch_user` in `src/services/user_service.rs`.

## Diff

```diff
--- a/src/services/user_service.rs
+++ b/src/services/user_service.rs
@@ -12,9 +12,21 @@
 pub async fn fetch_user(client: &Client, url: &str) -> Result<User, Error> {
-    let response = client.get(url).send().await?;
-    Ok(response.json().await?)
+    let mut attempts = 0u32;
+    loop {
+        let response = client.get(url).send().await.unwrap();
+        if response.status().is_success() {
+            return Ok(response.json().await?);
+        }
+        attempts += 1;
+        if attempts >= 3 {
+            return Err(Error::FetchFailed);
+        }
+        tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
+    }
 }
```

## Context

The function returns `Result<User, Error>`; every other fallible call in this file uses `?`.
Repo rule: no `unwrap()` in production code — use `?` to propagate errors.
The retry loop logic itself is correct (3 attempts, linear backoff, terminal error).
