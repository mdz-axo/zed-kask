//! Property layer for the workbook revision chain and crash-window
//! reconciliation contract (`kask/docs/reference/testing-protocol.md`
//! layer 2; plan §6-§7 of
//! `kask/docs/plans/logisheets-spreadsheet-capability-plan.md`).
//!
//! The expectation contract: spreadsheet mutations are exactly-once
//! transactions over an immutable revision chain. Every crash window
//! leaves the store all-old or all-new — never half-edited — and an
//! idempotency key either replays the recorded result or reports
//! `unknown`, never a silent re-application or a rewritten base.
//!
//! The crash windows follow from the apply ordering
//! (`service.rs` apply: verify → edit → `write_revision` →
//! `record_operation`): (1) before publication — nothing changes;
//! (2) after publication, before the completion record — an orphan
//! revision exists but reconciles as unknown; (3) after the record —
//! completed. The example tests construct each window directly; the
//! generated property runs whole edit chains and checks the history.

use std::collections::HashSet;

use futures::executor::block_on;
use proptest::prelude::*;

use crate::WorkbookService;
use crate::artifact_store::{ArtifactStore, digest_of};
use crate::{PublishOptions, SpreadsheetPublication};
use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, CellCoordinate, CellEdit, ColumnKind, EditTransaction,
    SpreadsheetAccess, SpreadsheetArtifactRef, TableColumn, TableValue,
};

// ── Fixtures ────────────────────────────────────────────────────────────────

fn service_in_temp() -> (std::sync::Arc<WorkbookService>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp artifact root");
    let service =
        WorkbookService::start_with_root(dir.path().to_path_buf()).expect("service starts");
    (service, dir)
}

fn origin() -> ArtifactOrigin {
    ArtifactOrigin::new(
        "hkask-mcp-portfolio".into(),
        "portfolio_what_if".into(),
        serde_json::json!({"portfolio": "main"}),
    )
    .expect("origin is valid")
}

fn table() -> AnalyticalTable {
    AnalyticalTable::new(
        "Property chain".into(),
        "Main".into(),
        vec![
            TableColumn {
                id: "label".into(),
                label: "Label".into(),
                kind: ColumnKind::Text,
            },
            TableColumn {
                id: "value".into(),
                label: "Value".into(),
                kind: ColumnKind::Number,
            },
        ],
        vec![
            vec![TableValue::Text("a".into()), TableValue::Number(1.0)],
            vec![TableValue::Text("b".into()), TableValue::Number(2.0)],
        ],
    )
    .expect("sample table is valid")
}

/// Publish the fixture table and return the base (initial) revision ref.
fn publish_base(service: &std::sync::Arc<WorkbookService>) -> SpreadsheetArtifactRef {
    let publication = block_on(service.publish(
        origin(),
        table(),
        PublishOptions {
            access: SpreadsheetAccess::WorkbookWhatIf,
        },
    ))
    .expect("publish succeeds");
    match publication {
        SpreadsheetPublication::Workbook { artifact, .. } => artifact,
        other => panic!("expected a workbook publication, got {other:?}"),
    }
}

/// A generated `SetCell` edit inside the table's sheet, with bounded
/// numeric payloads (the engine's save is byte-deterministic per the
/// Phase 0 admission record, but NaN/huge payloads are outside the
/// fixture's domain).
fn edit_strategy() -> impl Strategy<Value = CellEdit> {
    (
        (0usize..6),
        (0usize..4),
        (0u8..=3),
        (0u32..2_000_000u32),
        any::<bool>(),
    )
        .prop_map(|(row, col, kind, number, boolean)| {
            let coordinate =
                CellCoordinate::new("Main".into(), row, col).expect("bounded coordinate");
            let value = match kind {
                0 => TableValue::Number(number as f64),
                1 => TableValue::Text(format!("cell-{row}-{col}")),
                2 => TableValue::Boolean(boolean),
                _ => TableValue::Empty,
            };
            CellEdit::SetCell { coordinate, value }
        })
}

fn transaction(base: SpreadsheetArtifactRef, key: &str, edits: Vec<CellEdit>) -> EditTransaction {
    EditTransaction::new(
        base,
        key.to_string(),
        SpreadsheetAccess::WorkbookWhatIf,
        edits,
    )
    .expect("generated transaction is valid")
}

fn apply_workbook(
    service: &std::sync::Arc<WorkbookService>,
    txn: EditTransaction,
) -> SpreadsheetArtifactRef {
    let publication = block_on(service.apply(txn)).expect("apply succeeds");
    match publication {
        SpreadsheetPublication::Workbook { artifact, .. } => artifact,
        other => panic!("expected a workbook publication, got {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Hypothesis: a chain of sequential edit transactions preserves the
    /// full immutable revision history. After every link: each published
    /// revision is readable with a digest matching its bytes, the base
    /// revision's bytes never change, every publication mints a fresh
    /// revision id, and replaying any link's idempotency key (same base,
    /// same edits) returns the recorded result revision — never a fresh
    /// publication. Domain: 1-6 transactions of 1-3 generated SetCell
    /// edits each, applied over the fixture table.
    #[test]
    fn revision_chain_preserves_every_intermediate_and_base(
        edit_sequences in prop::collection::vec(prop::collection::vec(edit_strategy(), 1..=3), 1..=6),
    ) {
        let (service, dir) = service_in_temp();
        let reader = ArtifactStore::at(dir.path().to_path_buf())
            .expect("separate store handle opens");
        let base = publish_base(&service);
        let original_base_bytes = reader
            .read_revision(&base.artifact_id, &base.revision_id)
            .expect("base revision readable");

        let mut current = base.clone();
        let mut chain = vec![base.clone()];
        let mut applied = Vec::new();

        for (i, edits) in edit_sequences.into_iter().enumerate() {
            let key = format!("chain-key-{i}");
            let txn = transaction(current.clone(), &key, edits);
            let artifact = apply_workbook(&service, txn.clone());
            let bytes = reader
                .read_revision(&artifact.artifact_id, &artifact.revision_id)
                .expect("published revision readable");
            prop_assert_eq!(&digest_of(&bytes), &artifact.content_digest);
            chain.push(artifact.clone());
            applied.push((txn, artifact.clone()));
            current = artifact;
        }

        // History conservation: every revision, including the base, is
        // still readable and digest-matching.
        for revision in &chain {
            let bytes = reader
                .read_revision(&revision.artifact_id, &revision.revision_id)
                .expect("every revision in the chain stays readable");
            prop_assert_eq!(&digest_of(&bytes), &revision.content_digest);
        }
        let base_now = reader
            .read_revision(&base.artifact_id, &base.revision_id)
            .expect("base stays readable");
        prop_assert_eq!(
            base_now, original_base_bytes,
            "the base revision file must never change"
        );
        let ids: HashSet<&str> = chain.iter().map(|r| r.revision_id.as_str()).collect();
        prop_assert_eq!(ids.len(), chain.len(), "every publication mints a fresh revision id");

        // Exactly-once: replaying each link's key returns the recorded
        // result revision, not a fresh publication.
        for (txn, recorded) in &applied {
            let replayed = apply_workbook(&service, txn.clone());
            prop_assert_eq!(
                &replayed.revision_id, &recorded.revision_id,
                "idempotent replay must return the recorded revision"
            );
        }
    }
}

/// Crash window 2, constructed directly: the revision is durably
/// published but the completion record was never written (the window
/// between `write_revision` and `record_operation`). Reconciliation
/// reports `unknown` (no record), the orphan revision stays readable
/// (immutable history), the base is unchanged, and re-applying under
/// the same key publishes a fresh revision and records THAT — the
/// documented "retry under the same key is safe; duplication is
/// visible" semantics, never a silent no-op or replay of the orphan.
#[test]
fn orphan_revision_window_reconciles_unknown_then_reapplies_fresh() {
    let (service, dir) = service_in_temp();
    let reader = ArtifactStore::at(dir.path().to_path_buf()).expect("store handle opens");
    let base = publish_base(&service);
    let original_base_bytes = reader
        .read_revision(&base.artifact_id, &base.revision_id)
        .expect("base readable");

    let key = "orphan-key";
    let edits = vec![CellEdit::SetCell {
        coordinate: CellCoordinate::new("Main".into(), 0, 0).expect("bounded coordinate"),
        value: TableValue::Number(42.0),
    }];
    let txn = transaction(base.clone(), key, edits);
    let recorded = apply_workbook(&service, txn.clone());

    // Simulate the crash: the revision is on disk, the record is not.
    let op_file = dir
        .path()
        .join(&base.artifact_id)
        .join("ops")
        .join(format!("{}.json", digest_of(key.as_bytes())));
    std::fs::remove_file(&op_file).expect("completion record exists after apply");

    let reconciled = block_on(service.operation_get(base.artifact_id.clone(), key.to_string()))
        .expect("operation_get ok");
    assert!(
        reconciled.is_none(),
        "a missing completion record must reconcile as unknown"
    );

    // The orphan revision is still readable and the base is unchanged.
    let orphan_bytes = reader
        .read_revision(&recorded.artifact_id, &recorded.revision_id)
        .expect("the published revision exists (orphan, but immutable history)");
    assert_eq!(digest_of(&orphan_bytes), recorded.content_digest);
    let base_now = reader
        .read_revision(&base.artifact_id, &base.revision_id)
        .expect("base stays readable");
    assert_eq!(base_now, original_base_bytes, "base bytes never change");

    // Re-applying under the same key is safe: a fresh revision is
    // published and recorded; the orphan stays untouched.
    let fresh = apply_workbook(&service, txn);
    assert_ne!(
        fresh.revision_id, recorded.revision_id,
        "an unrecorded key re-applies to a fresh revision"
    );
    let reconciled = block_on(service.operation_get(base.artifact_id, key.to_string()))
        .expect("operation_get ok");
    let record = reconciled.expect("the re-application is recorded");
    assert_eq!(
        record.result.revision_id, fresh.revision_id,
        "the record points at the fresh revision"
    );
}

/// Crash window 1, residue variant: `write_revision` is atomic
/// (temp + fsync + rename), so a crash mid-write can leave at most a
/// dot-tmp file. Pin that such residue is never readable as a revision
/// and that the already-published revision is untouched by it.
#[test]
fn crash_leftover_temp_file_is_never_readable_as_a_revision() {
    let (service, dir) = service_in_temp();
    let reader = ArtifactStore::at(dir.path().to_path_buf()).expect("store handle opens");
    let base = publish_base(&service);

    // The exact temp name write_revision would leave behind.
    let tmp = dir
        .path()
        .join(&base.artifact_id)
        .join(format!(".{}.xlsx.tmp", "crashed-rev"));
    std::fs::write(&tmp, b"partial bytes from a crashed write").expect("residue file written");

    let residue = reader
        .read_revision(&base.artifact_id, "crashed-rev")
        .expect_err("a tmp file must never resolve as a revision");
    assert!(
        matches!(
            residue,
            hkask_types::spreadsheet::SpreadsheetError::UnknownArtifact { .. }
        ),
        "residue must surface as unknown, got: {residue:?}"
    );
    // The real revision still reads its own bytes.
    let bytes = reader
        .read_revision(&base.artifact_id, &base.revision_id)
        .expect("the published revision is unaffected by residue");
    assert_eq!(digest_of(&bytes), base.content_digest);
}

/// Immutability at the store boundary: a second `write_revision` under
/// the same revision id is rejected, and the original bytes are
/// unchanged — the final name is never overwritten.
#[test]
fn same_id_rewrite_is_rejected_and_bytes_unchanged() {
    let (service, dir) = service_in_temp();
    let reader = ArtifactStore::at(dir.path().to_path_buf()).expect("store handle opens");
    let base = publish_base(&service);
    let original = reader
        .read_revision(&base.artifact_id, &base.revision_id)
        .expect("base readable");

    let store = ArtifactStore::at(dir.path().to_path_buf()).expect("store opens");
    let rejected = store
        .write_revision(&base.artifact_id, &base.revision_id, b"other bytes")
        .expect_err("rewriting an existing revision id must be rejected");
    assert!(
        rejected.to_string().contains("revisions are immutable"),
        "the rejection must name immutability, got: {rejected:?}"
    );
    let after = reader
        .read_revision(&base.artifact_id, &base.revision_id)
        .expect("revision still readable after the rejected write");
    assert_eq!(
        after, original,
        "bytes must be unchanged by the rejected write"
    );
}
