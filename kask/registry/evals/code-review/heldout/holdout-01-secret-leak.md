# Held-out Fixture 01 — secret leakage (held out: never tune against these)

## Spec

Add the payment API call in `src/payment.rs`.

## Diff

```diff
--- a/src/payment.rs
+++ b/src/payment.rs
@@ -20,6 +20,12 @@
+pub async fn charge(amount: Cents) -> Result<Receipt, Error> {
+    tracing::info!("calling payment API with key={}", PAYMENT_API_KEY);
+    let response = HTTP.post(PAYMENT_URL).header("Authorization", &format!("Bearer {}", PAYMENT_API_KEY))
+        .json(&ChargeRequest::amount(amount)).send().await?;
+    Ok(response.json().await?)
+}
```

## Context

`PAYMENT_API_KEY` is a live credential read from the environment. Logs ship to the shared aggregator. Repo rule: secret leakage in logs/env is a critical-path security defect.
