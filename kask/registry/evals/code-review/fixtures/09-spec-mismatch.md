# Fixture 09 — spec mismatch (representative)

## Spec

Rate-limit the login endpoint to 5 attempts per minute per IP in `src/auth.rs`.

## Diff

```diff
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -40,6 +40,12 @@
+let key = format!("user:{}", credentials.user_id);
+if limiter.exceeded(&key, 5, Duration::from_secs(60)) {
+    return Err(Error::RateLimited);
+}
```

## Context

The limiter call itself is correct (5 per 60s). But the key is the user id, not the client IP. The spec asks for per-IP limiting; an attacker rotating accounts from one IP is never limited, and a shared-IP office can lock every user out when one account retries.
