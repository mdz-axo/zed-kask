use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{HMem, HMemStore};
use hkask_types::WebID;

/// expect: "Deleting a related h_mem set is all-or-nothing when a late row refuses deletion."
/// [P3] Motivating: Generative Space — compound domain state must remain coherent.
/// [P2] Constraining: Transparent Imperfection — a failed delete preserves the last durable state.
/// pre: two target rows and one unrelated row exist; deletion of the second target is forced to fail
/// post: both target rows and the unrelated row remain; a later successful retry deletes only the targets
#[test]
fn atomic_id_delete_rolls_back_earlier_rows_on_late_failure() -> anyhow::Result<()> {
    let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
    let first = HMem::new(
        "delete:target",
        "first",
        serde_json::json!("one"),
        WebID::new(),
    );
    let second = HMem::new(
        "delete:target",
        "second",
        serde_json::json!("two"),
        WebID::new(),
    );
    let unrelated = HMem::new(
        "delete:keep",
        "fact",
        serde_json::json!("keep"),
        WebID::new(),
    );
    for memory in [&first, &second, &unrelated] {
        store.insert(memory)?;
    }
    let trigger = format!(
        "CREATE TRIGGER fail_second_target_delete BEFORE DELETE ON hmems
         WHEN OLD.id = '{}'
         BEGIN SELECT RAISE(FAIL, 'forced late delete failure'); END;",
        second.id
    );
    store.driver().execute_batch(&trigger)?;

    let target_ids = [first.id, second.id];
    assert!(store.delete_batch_by_id_atomic(&target_ids).is_err());
    for memory in [&first, &second, &unrelated] {
        assert!(store.get_by_id(&memory.id)?.is_some());
    }

    store
        .driver()
        .execute_batch("DROP TRIGGER fail_second_target_delete;")?;
    assert_eq!(store.delete_batch_by_id_atomic(&target_ids)?, 2);
    assert!(store.get_by_id(&first.id)?.is_none());
    assert!(store.get_by_id(&second.id)?.is_none());
    assert!(store.get_by_id(&unrelated.id)?.is_some());
    Ok(())
}
