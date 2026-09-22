use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{HMem, HMemStore};
use hkask_types::{HMemOntology, WebID};

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

/// expect: "A child write raced with procedure deletion cannot survive without its parent."
/// [P3] Motivating: Generative Space — aggregate lifecycle owns every procedure row.
/// [P2] Constraining: Transparent Imperfection — transaction ordering cannot expose an orphan.
/// pre: one procedure root exists; guarded child publication and procedure deletion start together
/// post: after both operations finish, no row for that procedure remains
#[test]
fn guarded_publication_raced_with_procedure_delete_leaves_no_orphans() -> anyhow::Result<()> {
    let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
    let owner = WebID::new();
    let procedure = "board-race";
    let board = HMem::new(
        "kanban:board",
        procedure,
        serde_json::json!({"board_id": procedure}),
        owner,
    )
    .with_ontology(HMemOntology {
        pko_procedure: Some(procedure.to_string()),
        ..Default::default()
    });
    store.insert(&board)?;
    let task = HMem::new(
        "kanban:task",
        "task-race",
        serde_json::json!({"board_id": procedure}),
        owner,
    )
    .with_ontology(HMemOntology::process(procedure, "task-race", "kanban"));
    let index = HMem::new(
        "kanban:board_tasks:board-race",
        "task-race",
        serde_json::json!("task-race"),
        owner,
    )
    .with_ontology(HMemOntology::process(
        procedure,
        "task-race",
        "kanban:index",
    ));

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let insert_store = store.clone();
    let insert_barrier = barrier.clone();
    let insert = std::thread::spawn(move || {
        insert_barrier.wait();
        insert_store.insert_batch_if_key_exists_atomic(&[task, index], "kanban:board", procedure)
    });
    let delete_store = store.clone();
    let delete = std::thread::spawn(move || {
        barrier.wait();
        delete_store.delete_by_pko_procedure_atomic(procedure)
    });

    insert
        .join()
        .map_err(|_| anyhow::anyhow!("guarded insert thread panicked"))??;
    delete
        .join()
        .map_err(|_| anyhow::anyhow!("procedure delete thread panicked"))??;
    assert!(store.query_by_pko_procedure(procedure)?.is_empty());
    assert!(store.query_by_entity("kanban:board")?.is_empty());
    assert!(store.query_by_entity("kanban:task")?.is_empty());
    assert!(
        store
            .query_by_entity_prefix("kanban:board_tasks:", 100)?
            .is_empty()
    );
    Ok(())
}
