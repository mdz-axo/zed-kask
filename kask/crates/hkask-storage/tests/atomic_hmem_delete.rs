use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{HMem, HMemStore};
use hkask_types::{HMemOntology, WebID};

/// expect: "Deleting a related h_mem set is all-or-nothing when a late row refuses deletion."
/// [P3] Motivating: Generative Space — compound domain state must remain coherent.
/// [P2] Constraining: Transparent Imperfection — a failed delete preserves the last durable state.
/// pre: two target rows and one unrelated row exist; deletion of the second target is forced to fail
/// post: both target rows and the unrelated row remain; a later successful retry deletes only the targets
#[test]
fn atomic_key_delete_rolls_back_earlier_rows_on_late_failure() -> anyhow::Result<()> {
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
    store.driver().execute_batch(
        "CREATE TRIGGER fail_second_target_delete BEFORE DELETE ON hmems
         WHEN OLD.entity = 'delete:target' AND OLD.attribute = 'second'
         BEGIN SELECT RAISE(FAIL, 'forced late delete failure'); END;",
    )?;

    assert!(
        store
            .delete_related_keys_atomic("delete:target", "first", "delete:target", "second")
            .is_err()
    );
    for memory in [&first, &second, &unrelated] {
        assert!(store.get_by_id(&memory.id)?.is_some());
    }

    store
        .driver()
        .execute_batch("DROP TRIGGER fail_second_target_delete;")?;
    assert!(store.delete_related_keys_atomic(
        "delete:target",
        "first",
        "delete:target",
        "second"
    )?);
    assert!(store.get_by_id(&first.id)?.is_none());
    assert!(store.get_by_id(&second.id)?.is_none());
    assert!(store.get_by_id(&unrelated.id)?.is_some());
    Ok(())
}

/// expect: "Related-key deletion removes the current payload even if an update replaced its row ID."
/// [P3] Motivating: Generative Space — deletion cannot leave an invisible task payload.
/// [P2] Constraining: Transparent Imperfection — row identity changes do not silently degrade cleanup.
/// pre: a primary record is replaced by HMemStore::update before deletion
/// post: both current primary and dependent key are absent; unrelated records remain
#[test]
fn atomic_key_delete_follows_replaced_row_identity() -> anyhow::Result<()> {
    let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
    let owner = WebID::new();
    let payload = HMem::new("kanban:task", "task-1", serde_json::json!("before"), owner);
    let index = HMem::new(
        "kanban:board_tasks:board-1",
        "task-1",
        serde_json::json!("task-1"),
        owner,
    );
    for row in [&payload, &index] {
        store.insert(row)?;
    }
    store.update(&payload.id, serde_json::json!("after"), 1.0)?;
    assert!(store.get_by_id(&payload.id)?.is_none());
    assert!(store.delete_related_keys_atomic(
        "kanban:task",
        "task-1",
        "kanban:board_tasks:board-1",
        "task-1",
    )?);
    assert!(store.query_by_entity("kanban:task")?.is_empty());
    assert!(
        store
            .query_by_entity("kanban:board_tasks:board-1")?
            .is_empty()
    );
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
        delete_store.delete_by_pko_procedure_if_key_exists_atomic(
            procedure,
            "kanban:board",
            procedure,
        )
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

/// expect: "A board whose root does not belong to its procedure cannot lose its children."
/// [P2] Motivating: Transparent Imperfection — malformed roots fail visibly without partial deletion.
/// pre: the board key exists but has no matching procedure anchor
/// post: deletion fails and every row is unchanged
#[test]
fn procedure_delete_rejects_unanchored_or_mismatched_root_without_deleting_children()
-> anyhow::Result<()> {
    for root_ontology in [None, Some("another-procedure")] {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let owner = WebID::new();
        let mut root = HMem::new("kanban:board", "board-1", serde_json::json!("board"), owner);
        if let Some(procedure) = root_ontology {
            root = root.with_ontology(HMemOntology::process(procedure, "root", "kanban"));
        }
        let child = HMem::new("kanban:task", "task-1", serde_json::json!("task"), owner)
            .with_ontology(HMemOntology::process("board-1", "task-1", "kanban"));
        store.insert(&root)?;
        store.insert(&child)?;
        let error = store
            .delete_by_pko_procedure_if_key_exists_atomic("board-1", "kanban:board", "board-1")
            .expect_err("root must be part of the procedure being deleted");
        assert!(error.to_string().contains("procedure root"), "{error}");
        assert!(store.get_by_id(&root.id)?.is_some());
        assert!(store.get_by_id(&child.id)?.is_some());
    }
    Ok(())
}

/// expect: "A second goal transition reads the first committed verdict, not a stale snapshot."
/// [P2] Motivating: Transparent Imperfection — successful verdicts remain durable.
/// pre: the first writer holds its IMMEDIATE transaction while the second begins
/// post: the second writer sees the first verdict and commits both in order
#[test]
fn goal_value_transitions_serialize_under_immediate_write_lock() -> anyhow::Result<()> {
    use std::sync::mpsc;
    use std::time::Duration;

    let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
    let goal = HMem::new("kanban:goal", "goal-1", serde_json::json!([]), WebID::new());
    store.insert(&goal)?;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let first_store = store.clone();
    let first = std::thread::spawn(move || -> anyhow::Result<()> {
        first_store.update_value_atomic("kanban:goal", "goal-1", |value| {
            entered_tx.send(()).expect("signal first transaction");
            release_rx.recv().expect("release first transaction");
            let mut history: Vec<String> = serde_json::from_value(value)?;
            history.push("first".into());
            Ok::<_, anyhow::Error>((Some(serde_json::to_value(history)?), ()))
        })?;
        Ok(())
    });
    entered_rx.recv_timeout(Duration::from_secs(5))?;
    let second_store = store.clone();
    let second = std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
        started_tx.send(())?;
        second_store
            .update_value_atomic("kanban:goal", "goal-1", |value| {
                let mut history: Vec<String> = serde_json::from_value(value)?;
                history.push("second".into());
                Ok::<_, anyhow::Error>((Some(serde_json::to_value(&history)?), history))
            })?
            .ok_or_else(|| anyhow::anyhow!("goal disappeared"))
    });
    started_rx.recv_timeout(Duration::from_secs(5))?;
    release_tx.send(())?;
    first
        .join()
        .map_err(|_| anyhow::anyhow!("first writer panicked"))??;
    assert_eq!(
        second
            .join()
            .map_err(|_| anyhow::anyhow!("second writer panicked"))??,
        ["first", "second"]
    );
    assert_eq!(
        store.get_by_id(&goal.id)?.expect("goal retained").value,
        serde_json::json!(["first", "second"])
    );
    Ok(())
}
