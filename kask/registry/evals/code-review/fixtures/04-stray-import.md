# Fixture 04 — stray import (representative, tool-covered)

## Spec

Remove the debug logging from `src/handler.rs`.

## Diff

```diff
--- a/src/handler.rs
+++ b/src/handler.rs
@@ -20,9 +20,6 @@
 pub async fn handle(req: Request) -> Response {
     let start = Instant::now();
-    debug!("incoming request: {:?}", req.path);
-    debug!("headers: {:?}", req.headers);
     let response = route(req).await;
     info!("handled {} in {:?}", req.path, start.elapsed());
     response
```

## Context

The top of `src/handler.rs` (untouched by this diff):

```rust
use tracing::debug;
use tracing::info;
```

After this diff, no `debug!` call sites remain in the file. The Rust compiler emits an unused-import warning for `use tracing::debug;`, and the repo's CI runs `cargo check` with warnings denied.
