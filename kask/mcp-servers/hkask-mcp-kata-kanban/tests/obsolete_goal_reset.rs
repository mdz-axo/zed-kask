//! One-time obsolete-goal reset (GOAL_INTERFACE.md Forward-only policy).
//!
//! Operator ruling 2026-11-02 (disposition B): every stored goal row that does
//! not decode under the current schema-3 shape is obsolete and is removed —
//! together with its recall projection — by this explicit one-time reset,
//! never by runtime compatibility. The test is `#[ignore]`d and env-gated so
//! it can never run inside an ordinary suite invocation:
//!
//!   HKASK_GOAL_RESET_MODE=census  — print every stored goal row with its
//!                                   decode disposition; write nothing.
//!   HKASK_GOAL_RESET_MODE=delete  — delete every row that does not decode,
//!                                   print receipts, verify the host scan.
//!
//! The store is resolved exactly as the server resolves it (`HKASK_KANBAN_DB`
//! or the default data-dir path), and the passphrase through the canonical
//! env → keychain chain; the test fails closed when either does not resolve.

use hkask_mcp_kata_kanban::{Goal, KanbanService};
use hkask_storage::HMemStore;
use hkask_types::agent_paths::{mcp_server_db, resolve_under_data_dir};
use std::sync::Arc;

const GOAL_ENTITY: &str = "kanban:goal";

#[test]
#[ignore = "one-time operator-ruled reset; env-gated against production data"]
fn obsolete_goal_rows_are_reset_once_by_ruling() {
    let mode = std::env::var("HKASK_GOAL_RESET_MODE").unwrap_or_default();
    let delete = match mode.as_str() {
        "census" => false,
        "delete" => true,
        other => panic!(
            "refusing to run (mode {other:?}): set HKASK_GOAL_RESET_MODE=census \
             (read-only) or HKASK_GOAL_RESET_MODE=delete (removes obsolete rows)"
        ),
    };
    let path = match std::env::var("HKASK_KANBAN_DB") {
        Ok(configured) => std::path::PathBuf::from(configured),
        Err(_) => resolve_under_data_dir(&mcp_server_db("kata-kanban", "kanban")),
    };
    let passphrase = hkask_keystore::keychain::resolve_db_passphrase_string()
        .expect("HKASK_DB_PASSPHRASE must resolve via env or the keychain");
    let path_str = path.to_string_lossy().to_string();
    let db = hkask_storage::open_or_repair(&path_str, &passphrase)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    let pool = db.sqlite_pool().expect("sqlite pool");
    let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, path_str.as_str()),
    );
    let store = HMemStore::from_driver(driver).expect("hmem store");

    let rows = store.query_by_entity(GOAL_ENTITY).expect("goal rows");
    let mut current = Vec::new();
    let mut obsolete = Vec::new();
    for row in rows {
        let version = row.value.get("schema_version").and_then(|v| v.as_u64());
        let decodes =
            version == Some(3) && serde_json::from_value::<Goal>(row.value.clone()).is_ok();
        if decodes {
            current.push(row);
        } else {
            obsolete.push(row);
        }
    }
    for row in &obsolete {
        let version = row.value.get("schema_version").and_then(|v| v.as_u64());
        println!(
            "obsolete: key={} schema_version={:?} owner={}",
            row.attribute, version, row.access.owner_webid
        );
    }
    for row in &current {
        println!("current: key={}", row.attribute);
    }
    println!(
        "census: {} goal rows ({} current, {} obsolete) in {}",
        current.len() + obsolete.len(),
        current.len(),
        obsolete.len(),
        path.display()
    );
    if !delete {
        return;
    }
    for row in &obsolete {
        store
            .delete_by_id(&row.id)
            .unwrap_or_else(|error| panic!("delete {}: {error}", row.attribute));
        println!("deleted: key={}", row.attribute);
    }
    // Post-reset verification: the host scan — the read path that was down —
    // now succeeds over the remaining rows.
    let service = KanbanService::new(store);
    let scanned = service.goal_scan_for_host().expect("host scan after reset");
    println!(
        "post-reset scan: {} owner groups, {} goals",
        scanned.len(),
        scanned.values().map(Vec::len).sum::<usize>()
    );
}
