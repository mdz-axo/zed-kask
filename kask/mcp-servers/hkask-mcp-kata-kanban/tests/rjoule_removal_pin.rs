//! Resurrection pin: the rJoule/budget system is removed from the kata-kanban
//! server and must not come back.
//!
//! Decision record (operator ruling, executed 2026-09-08): budgets and rJoules
//! are deprecated project-wide. The 2026-09-04 budget-deprecation ruling
//! ("the budget concept is deprecated and should have been removed; timeouts
//! are the enforcement/kill mechanism for runaway or looping processes") was
//! recorded narrowly (thinking budgets in D49, memory count-based enforcement)
//! and never extended to the kanban rJoule system — the system survived four
//! gas→rJoule renames while dead-code sweeps deleted its unwired consumption
//! side (`task_consume_rjoules`, `SpendEntry::rjoule_spend`), leaving a
//! set/refill/display surface that nothing enforced. This pin supersedes the
//! gas→rJoule lineage (`1f5fd478ed`, `7839d2a0ef`, `0cdde8942a`,
//! `2a43a24954`, `2268514132` "Remove gas budget tracking, keep rJoule only")
//! and closes the loop: a future agent "recovering" rJoules from git history
//! fails this test.
//!
//! Out of scope by design (distinct systems, not budgets): the swarm ABW
//! credit ledger + consent-gated hires (real cloud spend) and the per-tick
//! runaway call ceiling (`ToolPortError::EnergyBudgetExceeded` — a rate
//! limiter against tool death spirals, same family as the timeouts the
//! deprecation ruling endorses).

use std::path::PathBuf;

/// Every `.rs` file under `src/` must be free of rJoule references. The test
/// file lives under `tests/`, so it cannot match itself.
#[test]
fn kanban_source_is_rjoule_free() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    let mut stack = vec![manifest_dir.clone()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).expect("read src dir");
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let content = std::fs::read_to_string(&path).expect("read source file");
                if content.to_lowercase().contains("rjoule") {
                    offenders.push(
                        path.strip_prefix(&manifest_dir)
                            .unwrap_or(path.as_path())
                            .display()
                            .to_string(),
                    );
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "rJoule references found in kata-kanban src/ — the budget system was removed by \
         operator ruling (see this file's doc comment). Offending files: {offenders:?}"
    );
}
