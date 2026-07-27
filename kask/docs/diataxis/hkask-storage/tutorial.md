---
title: "hkask-storage — Tutorial: Your First Migration"
audience: [developers new to hkask-storage]
last_updated: 2026-07-27
version: "0.1.0"
status: "Active"
domain: "Persistence"
mds_categories: [lifecycle]
---

# hkask-storage — Tutorial: Your First Migration

This tutorial walks through adding a new table to the `hkask-storage`
schema. You will learn the per-store `init_schema` pattern and how to test
your migration.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Pick the store module] --> B[Step 2: Add CREATE TABLE]
    B --> C[Step 3: Add CRUD methods]
    C --> D[Step 4: Test with in-memory DB]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-STOR-003
verified_date: 2026-07-27
verified_against: kask/crates/hkask-storage/src/core/database.rs:199,200; kask/crates/hkask-storage/src/regulation_store.rs:78
status: VERIFIED
-->

## Steps 1-2: Pick the store and add the table

Decide whether your table is core (belongs in `src/sql/schema.sql` at
`core/database.rs:200`) or store-specific (belongs in the store's
`init_schema` method at e.g. `regulation_store.rs:78`). Add a
`CREATE TABLE IF NOT EXISTS` statement with the appropriate columns and
foreign keys.

## Steps 3-4: Add CRUD methods and test

Add insert, query, update, and delete methods to the store struct. Write
a test that constructs an in-memory database, calls `init_schema`, and
verifies the CRUD methods. Run `cargo test -p hkask-storage`.

## See also

- [hkask-storage Reference](./reference.md): ERD of the full schema.
- [hkask-storage How-to](./how-to.md): procedural reference for migrations.

---

[^fowler-poeaa]: Fowler, M. (2002). *Patterns of Enterprise Application Architecture.* Addison-Wesley. <https://martinfowler.com/books/eaa.html>. The Active Record pattern that the store modules implement.
