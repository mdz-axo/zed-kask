# Fixture 11 — taste-only change (negative: never a Blocker)

## Spec

Improve readability of `parse_duration` in `src/timeutil.rs`.

## Diff

```diff
--- a/src/timeutil.rs
+++ b/src/timeutil.rs
@@ -18,11 +18,14 @@
-    match unit {
-        "s" => Ok(secs),
-        "m" => Ok(secs * 60),
-        "h" => Ok(secs * 3600),
-        _ => Err(Error::UnknownUnit),
-    }
+    if unit == "s" {
+        Ok(secs)
+    } else if unit == "m" {
+        Ok(secs * 60)
+    } else if unit == "h" {
+        Ok(secs * 3600)
+    } else {
+        Err(Error::UnknownUnit)
+    }
```

## Context

Functionally identical: same inputs, same outputs, same error. A reviewer may personally prefer `match` here — that preference is taste. No convention in `.rules` or the surrounding code picks a winner.
