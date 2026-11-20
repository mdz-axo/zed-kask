# Fixture 03 — invented API (representative, AI-code class)

## Spec

Use the new pagination helper for `list_orders` in `src/orders.rs`.

## Diff

```diff
--- a/src/orders.rs
+++ b/src/orders.rs
@@ -40,8 +40,9 @@
 pub async fn list_orders(repo: &Repo, page: usize, size: usize) -> Result<Vec<Order>, Error> {
     let all = repo.all_orders().await?;
-    let offset = (page - 1) * size;
-    let window = &all[offset..offset + size];
+    let window = all.paginate(page, size);
     Ok(window.to_vec())
 }
```

## Context

The pagination helper added last week lives in `src/pagination.rs`:

```rust
pub fn paginate_orders(orders: &[Order], page: usize, size: usize) -> Vec<&Order>
```

It is a free function, not a method on `Vec<Order>`. No `paginate` method exists anywhere in the codebase (grep confirms: zero definitions, zero other call sites).
