# Fixture 08 — cross-layer default mismatch (representative)

## Spec

Add feature flag `enable_gift_wrap` (default off), wired through client and server.

## Diff

```diff
--- a/src/client/flags.rs
+++ b/src/client/flags.rs
@@ -8,6 +8,7 @@
 pub struct ClientFlags { /* ... */ }
+pub const GIFT_WRAP_DEFAULT: bool = true;
--- a/src/server/flags.rs
+++ b/src/server/flags.rs
@@ -12,6 +12,7 @@
+let gift_wrap_enabled = flags.get_bool("enable_gift_wrap").unwrap_or(false);
```

## Context

The client renders the gift-wrap option when `GIFT_WRAP_DEFAULT` is true and no override is set. The server gates the gift-wrap charge behind `gift_wrap_enabled`. When the flag is unset in config, the client shows the option (default true) and the server rejects it (default false) — inconsistent gating ships to users.
