//! Service→Storage contract tests — Wave 4 Task 4.2
//!
//! Verifies that storage operations produce correct and deduplicated state.
//! Uses TestDb from hkask-test-harness for isolated database instances.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): storage schema changes must not break service expectations
//! - P8 (Semantic Grounding): each contract asserts a stated behavioral property

use hkask_storage::HMemStore;
use hkask_test_harness::{TestWebId, test_h_mem};
use serde_json::json;

// Storage operations produce correct and deduplicated state.

#[test]
fn hmem_insert_and_query() {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);

    let h_mem = test_h_mem("entity:test", "attr:name", json!("value"), None);
    store.insert(&h_mem).expect("insert should succeed");

    let results = store
        .query_by_entity("entity:test")
        .expect("query should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity, "entity:test");
    assert_eq!(results[0].attribute, "attr:name");
}

#[test]
fn hmem_query_by_attribute() {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);

    let t1 = test_h_mem("entity:a", "attr:shared", json!("v1"), None);
    let t2 = test_h_mem("entity:b", "attr:shared", json!("v2"), None);
    store.insert(&t1).expect("insert t1");
    store.insert(&t2).expect("insert t2");

    let results = store
        .query_by_attribute("attr:shared")
        .expect("query should succeed");
    assert_eq!(results.len(), 2);
}

#[test]
fn hmem_count_is_accurate() {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);

    assert_eq!(store.count_semantic().unwrap(), 0);

    store
        .insert(&test_h_mem("e1", "a1", json!("v1"), None))
        .unwrap();
    store
        .insert(&test_h_mem("e2", "a2", json!("v2"), None))
        .unwrap();
    store
        .insert(&test_h_mem("e3", "a3", json!("v3"), None))
        .unwrap();

    assert_eq!(store.count_semantic().unwrap(), 3);
}

#[test]
fn hmem_delete_removes_correctly() {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);

    let h_mem = test_h_mem("entity:del", "attr:test", json!("value"), None);
    store.insert(&h_mem).unwrap();
    assert_eq!(store.count_semantic().unwrap(), 1);

    store
        .delete_by_id(&h_mem.id)
        .expect("delete should succeed");
    assert_eq!(store.count_semantic().unwrap(), 0);
}

#[test]
fn hmem_owner_webid_is_preserved() {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);

    let owner = TestWebId::alice();
    let h_mem =
        hkask_test_harness::test_h_mem("entity:owned", "attr:owner", json!("data"), Some(owner));
    store.insert(&h_mem).unwrap();

    let results = store.query_by_entity("entity:owned").unwrap();
    assert_eq!(results.len(), 1);
    // Verify owner is stored (access.owner_webid)
    assert_eq!(results[0].access.owner_webid, owner);
}
