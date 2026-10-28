# Fixture 02 — silent error discard + hardcoded duplicate (representative)

## Spec

Cache the parsed config in `src/config.rs` so `parse_config` runs once per process.

## Diff

```diff
--- a/src/config.rs
+++ b/src/config.rs
@@ -28,10 +28,17 @@
 pub fn parse_config(path: &str) -> Result<Config, Error> {
     let raw = fs::read_to_string(path)?;
     let mut config: Config = toml::from_str(&raw)?;
+    let _ = config.validate();
+    let timeout = Duration::from_secs(30);
+    config.timeout = timeout;
+    CACHED.get_or_init(|| config.clone());
     Ok(config)
 }
```

## Context

`Config::validate(&self) -> Result<(), Error>` is fallible and load-bearing (rejects malformed config).
`pub const DEFAULT_TIMEOUT_SECS: u64 = 30;` is defined in this same module and is the canonical source for the default timeout.
`CACHED: OnceLock<Config>` exists above.
Repo rules: never silently discard errors with `let _ =` on fallible operations; hardcoded values duplicating existing config/constants are a maintenance defect.
