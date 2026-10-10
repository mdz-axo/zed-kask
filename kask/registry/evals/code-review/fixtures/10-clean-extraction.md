# Fixture 10 — clean extraction (negative: zero findings expected)

## Spec

Extract the email validation into a helper in `src/validators.rs`.

## Diff

```diff
--- a/src/validators.rs
+++ b/src/validators.rs
@@ -10,12 +10,7 @@
 pub fn validate_signup(form: &SignupForm) -> Result<(), Error> {
-    if form.email.is_empty() || !form.email.contains('@') {
-        return Err(Error::InvalidEmail);
-    }
+    validate_email(&form.email)?;
     if form.password.len() < 8 {
         return Err(Error::WeakPassword);
     }
     Ok(())
 }
+
+fn validate_email(email: &str) -> Result<(), Error> {
+    if email.is_empty() || !email.contains('@') {
+        return Err(Error::InvalidEmail);
+    }
+    Ok(())
+}
```

## Context

A pure mechanical extraction: identical logic, identical errors, private helper, no other callers of the old inline block. The diff does exactly what the spec asks and nothing else.
