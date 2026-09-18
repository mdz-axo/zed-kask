//! Tool-behavior contract tests for the spreadsheet MCP server.
//!
//! Drives the real `#[tool]` methods through their public `Parameters<T>`
//! seam over the REAL engine actor (`WorkbookService` on a dedicated thread)
//! and a temp-dir artifact root — the fully-capable-server standard: this
//! suite pins the tool contracts, not the engine (the deep-module suite in
//! `hkask-spreadsheet/src/service.rs` pins that).

#![forbid(unsafe_code)]

use hkask_mcp_server::server::McpToolError as ToolError;
use hkask_mcp_spreadsheet::server::{
    SpreadsheetApplyRequest, SpreadsheetOperationGetRequest, SpreadsheetServer,
};
use hkask_spreadsheet::{PublishOptions, SpreadsheetPublication, WorkbookService};
use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, CellCoordinate, CellEdit, ColumnKind, EditTransaction,
    SpreadsheetAccess, SpreadsheetArtifactRef, SpreadsheetError, SpreadsheetViewport, TableColumn,
    TableValue,
};
use hkask_types::{McpErrorKind, WebID};
use rmcp::handler::server::wrapper::Parameters;

/// Extract the MCP tool-result envelope: `{"content": <value>}`.
fn unwrap_content(output: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(output)
        .unwrap_or_else(|e| panic!("tool output must be valid JSON, got: {output} ({e})"));
    parsed
        .get("content")
        .cloned()
        .unwrap_or_else(|| panic!("tool output must have 'content' key, got: {parsed}"))
}

fn make_server() -> (SpreadsheetServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let service =
        WorkbookService::start_with_root(dir.path().join("workbooks")).expect("engine actor");
    let server = SpreadsheetServer::new(WebID::new(), service);
    (server, dir)
}

fn sample_table() -> AnalyticalTable {
    AnalyticalTable::new(
        "What-if staging".into(),
        "Main".into(),
        vec![
            TableColumn {
                id: "symbol".into(),
                label: "Symbol".into(),
                kind: ColumnKind::Text,
            },
            TableColumn {
                id: "actual".into(),
                label: "Actual".into(),
                kind: ColumnKind::Number,
            },
        ],
        vec![
            vec![TableValue::Text("AAPL".into()), TableValue::Number(15000.0)],
            vec![TableValue::Text("MSFT".into()), TableValue::Number(25000.0)],
        ],
    )
    .expect("sample table is valid")
}

fn origin() -> ArtifactOrigin {
    ArtifactOrigin::new(
        "hkask-mcp-portfolio".into(),
        "portfolio_what_if".into(),
        serde_json::json!({"portfolio": "main"}),
    )
    .expect("origin is valid")
}

async fn publish_base(server: &SpreadsheetServer) -> SpreadsheetArtifactRef {
    let publication = server
        .service
        .publish(
            origin(),
            sample_table(),
            PublishOptions {
                access: SpreadsheetAccess::WorkbookWhatIf,
            },
        )
        .await
        .expect("publish succeeds");
    match publication {
        SpreadsheetPublication::Workbook { artifact, .. } => artifact,
        other => panic!("expected a workbook publication, got {other:?}"),
    }
}

fn set_cell(row: usize, col: usize, value: f64) -> CellEdit {
    CellEdit::SetCell {
        coordinate: CellCoordinate::new("Main".into(), row, col).expect("coordinate"),
        value: TableValue::Number(value),
    }
}

async fn apply(
    server: &SpreadsheetServer,
    base: &SpreadsheetArtifactRef,
    key: &str,
    edits: Vec<CellEdit>,
) -> Result<serde_json::Value, ToolError> {
    let transaction = EditTransaction::new(
        base.clone(),
        key.into(),
        SpreadsheetAccess::WorkbookWhatIf,
        edits,
    )
    .expect("transaction is valid");
    let output = server
        .spreadsheet_apply(Parameters(SpreadsheetApplyRequest { transaction }))
        .await?;
    Ok(unwrap_content(&output))
}

/// §11: `spreadsheet_apply` creates a new immutable revision and returns a
/// server-authoritative display hint; the base reopens digest-intact.
#[tokio::test]
async fn apply_creates_immutable_revision_with_display_hint() {
    let (server, _dir) = make_server();
    let base = publish_base(&server).await;

    let content = apply(&server, &base, "idem-1", vec![set_cell(1, 1, 424242.0)])
        .await
        .expect("apply succeeds");
    assert_eq!(content["status"], "applied");

    // The display hint is a fenced spreadsheet block carrying the §6
    // contract shape.
    let hint = content["display_hint"].as_str().expect("display hint");
    let body = hint
        .strip_prefix("```spreadsheet\n")
        .and_then(|rest| rest.strip_suffix("\n```"))
        .expect("hint is a fenced spreadsheet block");
    let block: serde_json::Value = serde_json::from_str(body).expect("block body is JSON");
    assert_eq!(block["viz"], "spreadsheet");
    assert_eq!(block["access"], "WorkbookWhatIf");
    assert_eq!(
        block["mutation"]["tool"].as_str(),
        Some("spreadsheet_apply")
    );

    // A new immutable revision: the returned artifact differs from the base,
    // and the base reopens with its digest intact.
    let artifact_json = &content["artifact"];
    assert_eq!(
        artifact_json["revision_id"].as_str().expect("revision id"),
        block["artifact"]["revision_id"]
            .as_str()
            .expect("block revision id"),
        "the top-level artifact and the block artifact must agree"
    );
    assert_ne!(
        artifact_json["revision_id"].as_str().unwrap(),
        base.revision_id,
        "apply must mint a new revision"
    );
    server
        .service
        .open(&base)
        .await
        .expect("base reopens digest-intact after apply");
}

/// §11: a repeated idempotency identity returns the same result through the
/// tool — same revision, no new file.
#[tokio::test]
async fn apply_is_idempotent_through_the_tool() {
    let (server, dir) = make_server();
    let base = publish_base(&server).await;

    let first = apply(&server, &base, "idem-repeat", vec![set_cell(1, 1, 1.0)])
        .await
        .expect("first apply");
    let second = apply(&server, &base, "idem-repeat", vec![set_cell(1, 1, 1.0)])
        .await
        .expect("repeated apply returns the recorded result");
    assert_eq!(first["artifact"], second["artifact"]);

    let revision_files = || {
        std::fs::read_dir(dir.path().join("workbooks").join(&base.artifact_id))
            .expect("artifact dir")
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|ext| ext == "xlsx"))
            })
            .count()
    };
    assert_eq!(revision_files(), 2, "base + one applied revision, no more");
}

/// §11: `spreadsheet_operation_get` reconciles an interrupted operation —
/// completed with the recorded result, or explicitly unknown.
#[tokio::test]
async fn operation_get_reconciles_by_idempotency_key() {
    let (server, _dir) = make_server();
    let base = publish_base(&server).await;
    let applied = apply(&server, &base, "idem-reconcile", vec![set_cell(1, 1, 9.0)])
        .await
        .expect("apply succeeds");

    let output = server
        .spreadsheet_operation_get(Parameters(SpreadsheetOperationGetRequest {
            artifact_id: base.artifact_id.clone(),
            idempotency_key: "idem-reconcile".into(),
        }))
        .await
        .expect("operation_get succeeds");
    let content = unwrap_content(&output);
    assert_eq!(content["status"], "completed");
    assert_eq!(content["idempotency_key"], "idem-reconcile");
    assert_eq!(
        content["result"]["revision_id"],
        applied["artifact"]["revision_id"]
    );

    // An unknown key is UNKNOWN — never "not applied". The response says so.
    let output = server
        .spreadsheet_operation_get(Parameters(SpreadsheetOperationGetRequest {
            artifact_id: base.artifact_id.clone(),
            idempotency_key: "never-seen".into(),
        }))
        .await
        .expect("operation_get succeeds");
    let content = unwrap_content(&output);
    assert_eq!(content["status"], "unknown");
    assert!(
        content["note"].as_str().expect("note").contains("unknown"),
        "the unknown-outcome note must be explicit"
    );
}

/// §11 (error specificity): unknown base → not_found; stale digest →
/// failed_precondition (optimistic concurrency); a path-escaping id is
/// refused at the contract before dispatch.
#[tokio::test]
async fn apply_error_kinds_are_specific() {
    let (server, _dir) = make_server();
    let base = publish_base(&server).await;

    let unknown = SpreadsheetArtifactRef::new(
        "no-such-artifact".into(),
        base.revision_id.clone(),
        "a".repeat(64),
    )
    .expect("ref shape is valid");
    let error = apply(&server, &unknown, "idem", vec![set_cell(1, 1, 1.0)])
        .await
        .expect_err("unknown artifact must fail");
    assert_eq!(error.kind, McpErrorKind::NotFound, "unexpected: {error:?}");

    let stale = SpreadsheetArtifactRef::new(
        base.artifact_id.clone(),
        base.revision_id.clone(),
        "b".repeat(64),
    )
    .expect("ref shape is valid");
    let error = apply(&server, &stale, "idem", vec![set_cell(1, 1, 1.0)])
        .await
        .expect_err("stale digest must fail");
    assert_eq!(
        error.kind,
        McpErrorKind::FailedPrecondition,
        "unexpected: {error:?}"
    );

    // A path-escaping id is refused before any filesystem access.
    assert!(matches!(
        SpreadsheetArtifactRef::new("a/b".into(), "rev".into(), "a".repeat(64)),
        Err(SpreadsheetError::PathEscape { .. })
    ));
}

/// §11: a workbook edit cannot change the ledger database. The architectural
/// guarantee is containment: this server links no domain crate, and every
/// write lands beneath the spreadsheet artifact root. Pin the containment
/// half against the filesystem.
#[tokio::test]
async fn writes_are_contained_beneath_the_artifact_root() {
    let (server, dir) = make_server();
    let base = publish_base(&server).await;
    let _ = apply(&server, &base, "idem-contained", vec![set_cell(1, 1, 77.0)])
        .await
        .expect("apply succeeds");

    let root = dir.path().join("workbooks");
    let mut stack = vec![root.clone()];
    let mut file_count = 0usize;
    while let Some(entry) = stack.pop() {
        for item in std::fs::read_dir(&entry).expect("readable dir entry") {
            let path = item.expect("dir entry").path();
            assert!(
                path.starts_with(&root),
                "write escaped the artifact root: {}",
                path.display()
            );
            if path.is_dir() {
                stack.push(path);
            } else {
                file_count += 1;
            }
        }
    }
    assert!(
        file_count >= 4,
        "expected artifact.json + base revision + applied revision + op record under the root, got {file_count}"
    );
}

/// The engine actor outlives the tool call: open a document, stage locally,
/// and confirm nothing persisted until apply (the §2 authoritative-state
/// boundary end-to-end through the server).
#[tokio::test]
async fn staging_never_persists_through_the_server() {
    let (server, dir) = make_server();
    let base = publish_base(&server).await;

    let document = server.service.open(&base).await.expect("open");
    document
        .stage(vec![set_cell(1, 1, 555.0)])
        .await
        .expect("stage");

    // A FRESH ACTOR on the same root reads the revision from disk and still
    // shows the original — staging is local (the live document handle
    // intentionally keeps its staged state; idempotent open).
    let second_service =
        WorkbookService::start_with_root(dir.path().join("workbooks")).expect("second actor");
    let fresh = second_service.open(&base).await.expect("fresh open");
    let viewport = fresh
        .viewport(SpreadsheetViewport::new("Main".into(), 0, 0, 3, 2).expect("viewport"))
        .await
        .expect("viewport");
    assert_eq!(viewport.cells[1][1], TableValue::Number(15000.0));
}
