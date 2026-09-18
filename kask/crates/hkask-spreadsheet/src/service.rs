//! The spreadsheet service: a dedicated-thread engine actor plus its
//! runtime-agnostic async handles (plan §5.1 conceptual surface:
//! `WorkbookService::publish/open/apply`, `WorkbookDocument::
//! viewport/stage/undo/redo`).
//!
//! The LogiSheets `Workbook` is `!Send + !Sync`, so it is created, used, and
//! dropped on the actor thread only. Callers — MCP servers under tokio, the
//! GPUI widget on the foreground executor — send `Send` commands and await
//! `Send` responses. If the actor thread dies (a panic leaves an open
//! workbook in unknown state; it is not masked), subsequent calls surface a
//! typed `Engine` error naming the dead actor rather than hanging.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use futures::channel::oneshot;
use hkask_types::BlockProvenance;
use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, CellEdit, SpreadsheetAccess, SpreadsheetArtifactRef,
    SpreadsheetBlock, SpreadsheetError, SpreadsheetViewport, TableValue,
};
use hkask_types::spreadsheet::{InlineTableBlock, MAX_VIEWPORT_COLS, MAX_VIEWPORT_ROWS};

use crate::artifact_store::{ArtifactMeta, ArtifactStore, OperationRecord, digest_of};
use crate::engine;
use crate::{SPREADSHEET_APPLY_TOOL, SPREADSHEET_MCP_SERVER_ID};

/// The presentation choice for a publication (§6: callers explicitly choose;
/// there is no hidden threshold and no default).
pub struct PublishOptions {
    pub access: SpreadsheetAccess,
}

/// The result of a publication: either a bounded inline table (no
/// persistence) or an immutable workbook revision and its workbook block.
#[derive(Debug, Clone, PartialEq)]
pub enum SpreadsheetPublication {
    Inline(InlineTableBlock),
    Workbook {
        artifact: SpreadsheetArtifactRef,
        block: SpreadsheetBlock,
    },
}

/// One bounded window of cell values, extracted from an open document.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewportContent {
    pub window: SpreadsheetViewport,
    /// Row-major cells: `window.row_count` rows × `window.col_count` columns.
    pub cells: Vec<Vec<TableValue>>,
    /// Row-major formula text per cell (empty string = data cell). An editor
    /// shows the formula, not the evaluated value — editing
    /// `=SUM(B2:B3)` as the number `40000` would silently destroy the
    /// formula.
    pub formulas: Vec<Vec<String>>,
}

/// Commands cross the actor boundary; every field type is `Send`.
enum Command {
    Publish {
        origin: ArtifactOrigin,
        table: AnalyticalTable,
        access: SpreadsheetAccess,
        respond: oneshot::Sender<Result<SpreadsheetPublication, SpreadsheetError>>,
    },
    Open {
        artifact: SpreadsheetArtifactRef,
        respond: oneshot::Sender<Result<(), SpreadsheetError>>,
    },
    Apply {
        transaction: hkask_types::spreadsheet::EditTransaction,
        respond: oneshot::Sender<Result<SpreadsheetPublication, SpreadsheetError>>,
    },
    OperationGet {
        artifact_id: String,
        idempotency_key: String,
        respond: oneshot::Sender<Result<Option<OperationRecord>, SpreadsheetError>>,
    },
    Viewport {
        key: DocumentKey,
        window: SpreadsheetViewport,
        respond: oneshot::Sender<Result<ViewportContent, SpreadsheetError>>,
    },
    Stage {
        key: DocumentKey,
        edits: Vec<CellEdit>,
        respond: oneshot::Sender<Result<(), SpreadsheetError>>,
    },
    Undo {
        key: DocumentKey,
        respond: oneshot::Sender<Result<bool, SpreadsheetError>>,
    },
    Redo {
        key: DocumentKey,
        respond: oneshot::Sender<Result<bool, SpreadsheetError>>,
    },
    Sheets {
        key: DocumentKey,
        respond: oneshot::Sender<Result<Vec<String>, SpreadsheetError>>,
    },
}

/// An open document's identity on the actor side.
type DocumentKey = (String, String);

fn actor_down() -> SpreadsheetError {
    SpreadsheetError::Engine {
        detail: "spreadsheet engine actor is down (thread exited or panicked)".into(),
    }
}

/// Deliver a response to its requester. A send fails only when the requester
/// dropped its receiver (cancelled); that is a normal cancellation, logged at
/// debug — never a silent discard.
fn deliver<T>(sender: oneshot::Sender<T>, value: T) {
    if sender.send(value).is_err() {
        tracing::debug!(target: "hkask.spreadsheet", "response channel dropped before delivery");
    }
}

/// The deep module's public handle (plan §5.1). Cloning is not needed:
/// `Arc<WorkbookService>` is the shareable form.
pub struct WorkbookService {
    tx: mpsc::Sender<Command>,
}

impl WorkbookService {
    /// Start the service with the production artifact root
    /// (`~/Documents/zk-data/spreadsheet-mcp/workbooks/`, §7).
    pub fn start() -> Result<Arc<Self>, SpreadsheetError> {
        Self::start_with_root(crate::artifact_store::production_root())
    }

    /// Start the service with an explicit artifact root (the test seam; the
    /// server may also pass a configured root).
    pub fn start_with_root(root: PathBuf) -> Result<Arc<Self>, SpreadsheetError> {
        let store = ArtifactStore::at(root)?;
        let (tx, rx) = mpsc::channel::<Command>();
        let spawned = std::thread::Builder::new()
            .name("hkask-spreadsheet-engine".into())
            .spawn(move || actor_loop(store, rx));
        let handle = spawned.map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot spawn the engine actor thread: {error}"),
        })?;
        tracing::info!(target: "hkask.spreadsheet", pid = ?handle, "spreadsheet engine actor started");
        Ok(Arc::new(Self { tx }))
    }

    fn send(&self, command: Command) -> Result<(), SpreadsheetError> {
        self.tx.send(command).map_err(|_| actor_down())
    }

    /// Publish a table under an explicit presentation mode (§6).
    pub async fn publish(
        &self,
        origin: ArtifactOrigin,
        table: AnalyticalTable,
        options: PublishOptions,
    ) -> Result<SpreadsheetPublication, SpreadsheetError> {
        origin.validate()?;
        table.validate()?;
        let (respond, receiver) = oneshot::channel();
        self.send(Command::Publish {
            origin,
            table,
            access: options.access,
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// Open one immutable revision as a staging document: local edits are
    /// undoable in-memory; nothing persists until `apply` produces a new
    /// revision. The stored revision's digest is verified at open (§6:
    /// digest mismatch is a conflict, not silent acceptance).
    pub async fn open(
        self: &Arc<Self>,
        artifact: &SpreadsheetArtifactRef,
    ) -> Result<WorkbookDocument, SpreadsheetError> {
        artifact.validate()?;
        let (respond, receiver) = oneshot::channel();
        self.send(Command::Open {
            artifact: artifact.clone(),
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())??;
        Ok(WorkbookDocument {
            service: Arc::clone(self),
            artifact: artifact.clone(),
        })
    }

    /// Apply a mutation against a base revision (§7): verifies the base
    /// digest, applies the edits, and publishes a NEW immutable revision —
    /// the base file is never rewritten. A repeated idempotency identity
    /// returns the recorded result; a key reused for a different base is a
    /// typed error, not a silent overwrite.
    pub async fn apply(
        &self,
        transaction: hkask_types::spreadsheet::EditTransaction,
    ) -> Result<SpreadsheetPublication, SpreadsheetError> {
        transaction.validate()?;
        let (respond, receiver) = oneshot::channel();
        self.send(Command::Apply {
            transaction,
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// Look up a completed operation by its idempotency key (§7
    /// reconciliation for interrupted mutations). `Ok(None)` means the
    /// outcome is UNKNOWN — the operation may or may not have been applied;
    /// callers must not blindly retry (the widget does not auto-replay).
    pub async fn operation_get(
        &self,
        artifact_id: String,
        idempotency_key: String,
    ) -> Result<Option<OperationRecord>, SpreadsheetError> {
        let (respond, receiver) = oneshot::channel();
        self.send(Command::OperationGet {
            artifact_id,
            idempotency_key,
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }
}

// The `Arc<Self>` reborrow note: `open` uses the `&Arc<Self>` receiver so
// the returned `WorkbookDocument` shares the service handle it dispatches
// through — no Arc is conjured from `&self`.

/// An open workbook document (plan §5.1): staged edits are local and
/// undoable; `viewport` reads the staged state; persistence happens through
/// the service's `apply`, never here. Cheap to clone (an `Arc` and an
/// artifact reference) so widgets can hold one handle across async
/// operations.
#[derive(Clone)]
pub struct WorkbookDocument {
    service: Arc<WorkbookService>,
    artifact: SpreadsheetArtifactRef,
}

impl std::fmt::Debug for WorkbookDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkbookDocument")
            .field("artifact", &self.artifact)
            .finish()
    }
}

impl WorkbookDocument {
    /// The revision this document was opened from.
    pub fn artifact(&self) -> &SpreadsheetArtifactRef {
        &self.artifact
    }

    pub async fn viewport(
        &self,
        window: SpreadsheetViewport,
    ) -> Result<ViewportContent, SpreadsheetError> {
        window.validate()?;
        let (respond, receiver) = oneshot::channel();
        self.service.send(Command::Viewport {
            key: (
                self.artifact.artifact_id.clone(),
                self.artifact.revision_id.clone(),
            ),
            window,
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// Stage typed cell edits locally (undoable; never persisted).
    pub async fn stage(&self, edits: Vec<CellEdit>) -> Result<(), SpreadsheetError> {
        for edit in &edits {
            edit.validate()?;
        }
        let (respond, receiver) = oneshot::channel();
        self.service.send(Command::Stage {
            key: (
                self.artifact.artifact_id.clone(),
                self.artifact.revision_id.clone(),
            ),
            edits,
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// Undo one staged step; returns whether a step existed.
    pub async fn undo(&self) -> Result<bool, SpreadsheetError> {
        let (respond, receiver) = oneshot::channel();
        self.service.send(Command::Undo {
            key: (
                self.artifact.artifact_id.clone(),
                self.artifact.revision_id.clone(),
            ),
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// Redo one undone staged step; returns whether a step existed.
    pub async fn redo(&self) -> Result<bool, SpreadsheetError> {
        let (respond, receiver) = oneshot::channel();
        self.service.send(Command::Redo {
            key: (
                self.artifact.artifact_id.clone(),
                self.artifact.revision_id.clone(),
            ),
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }

    /// The sheet names of the open workbook, in workbook order.
    pub async fn sheets(&self) -> Result<Vec<String>, SpreadsheetError> {
        let (respond, receiver) = oneshot::channel();
        self.service.send(Command::Sheets {
            key: (
                self.artifact.artifact_id.clone(),
                self.artifact.revision_id.clone(),
            ),
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?
    }
}

/// The actor's on-thread state: the store and every open document.
struct ActorState {
    store: ArtifactStore,
    documents: HashMap<DocumentKey, logisheets_rs::Workbook>,
}

fn actor_loop(store: ArtifactStore, rx: mpsc::Receiver<Command>) {
    let mut state = ActorState {
        store,
        documents: HashMap::new(),
    };
    while let Ok(command) = rx.recv() {
        match command {
            Command::Publish {
                origin,
                table,
                access,
                respond,
            } => {
                let result = handle_publish(&mut state, origin, table, access);
                deliver(respond, result);
            }
            Command::Open { artifact, respond } => {
                let result = handle_open(&mut state, &artifact);
                deliver(respond, result);
            }
            Command::Apply {
                transaction,
                respond,
            } => {
                let result = handle_apply(&mut state, transaction);
                deliver(respond, result);
            }
            Command::OperationGet {
                artifact_id,
                idempotency_key,
                respond,
            } => {
                let result = state.store.find_operation(&artifact_id, &idempotency_key);
                deliver(respond, result);
            }
            Command::Viewport {
                key,
                window,
                respond,
            } => {
                let result = match state.documents.get(&key) {
                    Some(workbook) => engine::extract_viewport(workbook, &window),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                deliver(respond, result);
            }
            Command::Stage {
                key,
                edits,
                respond,
            } => {
                let result = match state.documents.get_mut(&key) {
                    Some(workbook) => engine::apply_edits(workbook, &edits, true),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                deliver(respond, result);
            }
            Command::Undo { key, respond } => {
                let result = match state.documents.get_mut(&key) {
                    Some(workbook) => Ok(workbook.undo()),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                deliver(respond, result);
            }
            Command::Redo { key, respond } => {
                let result = match state.documents.get_mut(&key) {
                    Some(workbook) => Ok(workbook.redo()),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                deliver(respond, result);
            }
            Command::Sheets { key, respond } => {
                let result = match state.documents.get(&key) {
                    Some(workbook) => Ok((0..workbook.get_sheet_count())
                        .filter_map(|index| workbook.get_sheet_name_by_idx(index).ok())
                        .collect()),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                deliver(respond, result);
            }
        }
    }
}

/// Build the workbook block for a revision of a known artifact (plan §6 field
/// list): the mutation endpoint is server-authored — incomplete provenance is
/// rejected at the contract, so it is fully populated here.
fn build_block(
    meta: &ArtifactMeta,
    artifact: &SpreadsheetArtifactRef,
) -> Result<SpreadsheetBlock, SpreadsheetError> {
    let row_count = meta.rows.clamp(1, MAX_VIEWPORT_ROWS);
    let col_count = meta.cols.clamp(1, MAX_VIEWPORT_COLS);
    let viewport = SpreadsheetViewport::new(meta.sheet_name.clone(), 0, 0, row_count, col_count)?;
    SpreadsheetBlock::new(
        meta.title.clone(),
        meta.sheet_name.clone(),
        artifact.clone(),
        viewport,
        meta.origin.clone(),
        BlockProvenance {
            tool: Some(SPREADSHEET_APPLY_TOOL.to_string()),
            server: Some(SPREADSHEET_MCP_SERVER_ID.to_string()),
            args: serde_json::json!({
                "artifact_id": artifact.artifact_id,
                "revision_id": artifact.revision_id,
                "content_digest": artifact.content_digest,
            }),
            span_id: None,
        },
    )
}

fn handle_publish(
    state: &mut ActorState,
    origin: ArtifactOrigin,
    table: AnalyticalTable,
    access: SpreadsheetAccess,
) -> Result<SpreadsheetPublication, SpreadsheetError> {
    match access {
        SpreadsheetAccess::DataOnly => Err(SpreadsheetError::AccessMismatch {
            detail: "DataOnly publishes no block; pass InlineTable or WorkbookWhatIf".into(),
        }),
        SpreadsheetAccess::InlineTable => {
            let block = InlineTableBlock::from_table(origin, table)?;
            Ok(SpreadsheetPublication::Inline(block))
        }
        SpreadsheetAccess::WorkbookWhatIf => {
            let workbook = engine::build_workbook(&table)?;
            let bytes = workbook.save().map_err(|error| SpreadsheetError::Engine {
                detail: format!("engine save failed: {error:?}"),
            })?;
            let digest = digest_of(&bytes);
            let artifact_id = uuid::Uuid::new_v4().simple().to_string();
            let revision_id = uuid::Uuid::new_v4().simple().to_string();
            let artifact =
                SpreadsheetArtifactRef::new(artifact_id.clone(), revision_id.clone(), digest)?;
            state
                .store
                .write_revision(&artifact_id, &revision_id, &bytes)?;
            let meta = ArtifactMeta {
                origin,
                title: table.title.clone(),
                sheet_name: table.sheet_name.clone(),
                rows: table.rows.len() + 1,
                cols: table.columns.len(),
            };
            state.store.write_metadata(&artifact_id, &meta)?;
            let block = build_block(&meta, &artifact)?;
            tracing::info!(
                target: "hkask.spreadsheet",
                artifact_id = %artifact_id,
                revision_id = %revision_id,
                rows = meta.rows,
                "published workbook revision"
            );
            Ok(SpreadsheetPublication::Workbook { artifact, block })
        }
    }
}

fn handle_open(
    state: &mut ActorState,
    artifact: &SpreadsheetArtifactRef,
) -> Result<(), SpreadsheetError> {
    let key = (artifact.artifact_id.clone(), artifact.revision_id.clone());
    // Idempotent open: a document for this (artifact, revision) pair may
    // already be open (a second widget for the same block body, a cache-evict
    // re-render). Revisions are immutable and the digest was verified at the
    // first open, so re-opening must keep the existing workbook — an
    // unconditional insert would silently destroy another handle's staged
    // state.
    if state.documents.contains_key(&key) {
        return Ok(());
    }
    let bytes = state
        .store
        .read_revision(&artifact.artifact_id, &artifact.revision_id)?;
    let actual = digest_of(&bytes);
    if actual != artifact.content_digest {
        return Err(SpreadsheetError::Conflict {
            artifact_id: artifact.artifact_id.clone(),
            expected: artifact.content_digest.clone(),
            found: actual,
        });
    }
    let workbook = logisheets_rs::Workbook::from_file(&bytes, artifact.artifact_id.clone())
        .map_err(|error| SpreadsheetError::Engine {
            detail: format!("engine open failed: {error}"),
        })?;
    state.documents.insert(key, workbook);
    Ok(())
}

fn handle_apply(
    state: &mut ActorState,
    transaction: hkask_types::spreadsheet::EditTransaction,
) -> Result<SpreadsheetPublication, SpreadsheetError> {
    let artifact_id = transaction.base_artifact.artifact_id.clone();

    // Idempotency (§7): a repeated identity returns the recorded result; a
    // key reused for a different base is a typed error.
    if let Some(record) = state
        .store
        .find_operation(&artifact_id, &transaction.idempotency_key)?
    {
        if record.base == transaction.base_artifact {
            let meta = state.store.read_metadata(&record.result.artifact_id)?;
            let block = build_block(&meta, &record.result)?;
            return Ok(SpreadsheetPublication::Workbook {
                artifact: record.result,
                block,
            });
        }
        return Err(SpreadsheetError::InvalidTransaction {
            detail: format!(
                "idempotency key {:?} was already used for a different base revision",
                transaction.idempotency_key
            ),
        });
    }

    // Optimistic concurrency: the base digest must match the stored revision.
    let base_bytes = state
        .store
        .read_revision(&artifact_id, &transaction.base_artifact.revision_id)?;
    let actual = digest_of(&base_bytes);
    if actual != transaction.base_artifact.content_digest {
        return Err(SpreadsheetError::Conflict {
            artifact_id: artifact_id,
            expected: transaction.base_artifact.content_digest,
            found: actual,
        });
    }

    let mut workbook = logisheets_rs::Workbook::from_file(&base_bytes, artifact_id.clone())
        .map_err(|error| SpreadsheetError::Engine {
            detail: format!("engine open failed: {error:?}"),
        })?;
    engine::apply_edits(&mut workbook, &transaction.edits, false)?;

    let bytes = workbook.save().map_err(|error| SpreadsheetError::Engine {
        detail: format!("engine save failed: {error:?}"),
    })?;
    let digest = digest_of(&bytes);
    let revision_id = uuid::Uuid::new_v4().simple().to_string();
    let result_ref = SpreadsheetArtifactRef::new(artifact_id.clone(), revision_id.clone(), digest)?;
    state
        .store
        .write_revision(&artifact_id, &revision_id, &bytes)?;

    let meta = state.store.read_metadata(&artifact_id)?;
    let block = build_block(&meta, &result_ref)?;

    state.store.record_operation(
        &artifact_id,
        &OperationRecord {
            idempotency_key: transaction.idempotency_key.clone(),
            base: transaction.base_artifact.clone(),
            result: result_ref.clone(),
        },
    )?;
    tracing::info!(
        target: "hkask.spreadsheet",
        artifact_id = %artifact_id,
        revision_id = %revision_id,
        edits = transaction.edits.len(),
        "applied edit transaction"
    );
    Ok(SpreadsheetPublication::Workbook {
        artifact: result_ref,
        block,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use hkask_types::spreadsheet::{
        AnalyticalTable, CellEdit, EditTransaction, SpreadsheetArtifactRef, SpreadsheetViewport,
        TableColumn,
    };

    fn sample_table() -> AnalyticalTable {
        AnalyticalTable::new(
            "What-if staging".into(),
            "Main".into(),
            vec![
                TableColumn {
                    id: "2024".into(),
                    label: "2024".into(),
                    kind: hkask_types::spreadsheet::ColumnKind::Text,
                },
                TableColumn {
                    id: "actual".into(),
                    label: "Actual".into(),
                    kind: hkask_types::spreadsheet::ColumnKind::Number,
                },
                TableColumn {
                    id: "hypothetical".into(),
                    label: "Hypothetical".into(),
                    kind: hkask_types::spreadsheet::ColumnKind::Number,
                },
            ],
            vec![
                vec![
                    hkask_types::spreadsheet::TableValue::Text("123".into()),
                    hkask_types::spreadsheet::TableValue::Number(15000.0),
                    hkask_types::spreadsheet::TableValue::Number(18000.0),
                ],
                vec![
                    hkask_types::spreadsheet::TableValue::Text("'quoted".into()),
                    hkask_types::spreadsheet::TableValue::Number(25000.0),
                    hkask_types::spreadsheet::TableValue::Boolean(false),
                ],
                vec![
                    hkask_types::spreadsheet::TableValue::Empty,
                    hkask_types::spreadsheet::TableValue::Boolean(true),
                    hkask_types::spreadsheet::TableValue::Empty,
                ],
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

    fn publish_workbook(
        service: &Arc<WorkbookService>,
        table: AnalyticalTable,
    ) -> (SpreadsheetArtifactRef, SpreadsheetBlock) {
        let publication = block_on(service.publish(
            origin(),
            table,
            PublishOptions {
                access: SpreadsheetAccess::WorkbookWhatIf,
            },
        ))
        .expect("publish succeeds");
        match publication {
            SpreadsheetPublication::Workbook { artifact, block } => (artifact, block),
            other => panic!("expected a workbook publication, got {other:?}"),
        }
    }

    fn full_viewport(sheet: &str, rows: usize, cols: usize) -> SpreadsheetViewport {
        SpreadsheetViewport::new(
            sheet.into(),
            0,
            0,
            rows.min(hkask_types::spreadsheet::MAX_VIEWPORT_ROWS),
            cols.min(hkask_types::spreadsheet::MAX_VIEWPORT_COLS),
        )
        .expect("viewport is valid")
    }

    fn cell(
        content: &ViewportContent,
        row: usize,
        col: usize,
    ) -> &hkask_types::spreadsheet::TableValue {
        &content.cells[row][col]
    }

    /// §11: AnalyticalTable → XLSX → reopen preserves values. Text fidelity
    /// is pinned hard: numeric-looking text, already-quoted text, a
    /// numeric-looking header label, booleans, and empty cells all survive
    /// publish → reopen exactly.
    #[test]
    fn table_to_xlsx_roundtrip_preserves_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let table = sample_table();
        let (artifact, _block) = publish_workbook(&service, table);

        let document = block_on(service.open(&artifact)).expect("reopen");
        let content = block_on(document.viewport(full_viewport("Main", 4, 3))).expect("viewport");
        // Header labels (numeric-looking label must stay text).
        assert_eq!(cell(&content, 0, 0), &TableValue::Text("2024".into()));
        assert_eq!(cell(&content, 0, 1), &TableValue::Text("Actual".into()));
        // Data fidelity.
        assert_eq!(cell(&content, 1, 0), &TableValue::Text("123".into()));
        assert_eq!(cell(&content, 1, 1), &TableValue::Number(15000.0));
        assert_eq!(cell(&content, 1, 2), &TableValue::Number(18000.0));
        assert_eq!(cell(&content, 2, 0), &TableValue::Text("'quoted".into()));
        assert_eq!(cell(&content, 2, 1), &TableValue::Number(25000.0));
        assert_eq!(cell(&content, 2, 2), &TableValue::Boolean(false));
        assert_eq!(cell(&content, 3, 0), &TableValue::Empty);
        assert_eq!(cell(&content, 3, 1), &TableValue::Boolean(true));
        assert_eq!(cell(&content, 3, 2), &TableValue::Empty);
    }

    /// §11: formula edits recalculate dependent cells (after apply, the new
    /// revision evaluates the formula).
    #[test]
    fn formula_edits_recalculate_dependents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (base, _block) = publish_workbook(&service, sample_table());

        let transaction = EditTransaction::new(
            base.clone(),
            "idem-formula".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::SetFormula {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 4, 1)
                    .expect("coordinate"),
                formula: "=SUM(B2:B4)".into(),
            }],
        )
        .expect("transaction is valid");
        let publication = block_on(service.apply(transaction)).expect("apply succeeds");
        let (applied, _block) = match publication {
            SpreadsheetPublication::Workbook { artifact, block } => (artifact, block),
            other => panic!("expected a workbook publication, got {other:?}"),
        };
        assert_ne!(
            applied.revision_id, base.revision_id,
            "apply must mint a new revision"
        );

        let document = block_on(service.open(&applied)).expect("reopen applied revision");
        let content = block_on(document.viewport(full_viewport("Main", 5, 3))).expect("viewport");
        // B2:B4 = 15000 + 25000 + TRUE(1)? — the sum covers data rows 1..3 of
        // column B: 15000, 25000, and TRUE. Booleans are ignored by SUM in
        // Excel, so the result is 40000.
        assert_eq!(cell(&content, 4, 1), &TableValue::Number(40000.0));
    }

    /// §11: base revision remains unchanged after commit — reopening the base
    /// (digest-verified) still shows the original values.
    #[test]
    fn base_revision_unchanged_after_apply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (base, _block) = publish_workbook(&service, sample_table());

        let publication = block_on(
            service.apply(
                EditTransaction::new(
                    base.clone(),
                    "idem-base".into(),
                    SpreadsheetAccess::WorkbookWhatIf,
                    vec![CellEdit::SetCell {
                        coordinate: hkask_types::spreadsheet::CellCoordinate::new(
                            "Main".into(),
                            1,
                            1,
                        )
                        .expect("coordinate"),
                        value: TableValue::Number(999.0),
                    }],
                )
                .expect("transaction is valid"),
            ),
        )
        .expect("apply succeeds");

        // The base reopens with its digest intact — immutability, not a
        // rewrite — and still shows the original value.
        let base_document = block_on(service.open(&base)).expect("base reopens (digest intact)");
        let content =
            block_on(base_document.viewport(full_viewport("Main", 4, 3))).expect("base viewport");
        assert_eq!(cell(&content, 1, 1), &TableValue::Number(15000.0));

        // The applied revision shows the edit.
        let applied = match publication {
            SpreadsheetPublication::Workbook { artifact, .. } => artifact,
            other => panic!("expected a workbook publication, got {other:?}"),
        };
        let applied_document = block_on(service.open(&applied)).expect("applied reopens");
        let content = block_on(applied_document.viewport(full_viewport("Main", 4, 3)))
            .expect("applied viewport");
        assert_eq!(cell(&content, 1, 1), &TableValue::Number(999.0));
    }

    /// §11: path traversal and unknown artifact identities are rejected
    /// (path escape at the contract; unknown id at the store).
    #[test]
    fn path_traversal_and_unknown_ids_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");

        let escape = SpreadsheetArtifactRef::new("a/b".into(), "rev".into(), "a".repeat(64));
        assert!(matches!(escape, Err(SpreadsheetError::PathEscape { .. })));

        let unknown =
            SpreadsheetArtifactRef::new("no-such-artifact".into(), "rev-1".into(), "a".repeat(64))
                .expect("ref shape is valid");
        let error = block_on(service.open(&unknown)).expect_err("unknown artifact must fail");
        assert!(
            matches!(error, SpreadsheetError::UnknownArtifact { .. }),
            "unexpected error: {error}"
        );

        let transaction = EditTransaction::new(
            unknown,
            "idem".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::ClearCell {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 1, 1)
                    .expect("coordinate"),
            }],
        )
        .expect("transaction is valid");
        let error = block_on(service.apply(transaction)).expect_err("unknown base must fail");
        assert!(
            matches!(error, SpreadsheetError::UnknownArtifact { .. }),
            "unexpected error: {error}"
        );
    }

    /// §11: digest mismatch produces Conflict — never a silent overwrite of a
    /// stale base. Also at open: a corrupted/tampered revision is a conflict.
    #[test]
    fn digest_mismatch_produces_conflict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (base, _block) = publish_workbook(&service, sample_table());

        let stale =
            SpreadsheetArtifactRef::new(base.artifact_id.clone(), base.revision_id, "b".repeat(64))
                .expect("ref shape is valid");
        let error = block_on(service.open(&stale)).expect_err("wrong digest at open must fail");
        assert!(
            matches!(error, SpreadsheetError::Conflict { .. }),
            "unexpected error: {error}"
        );

        let transaction = EditTransaction::new(
            stale,
            "idem-stale".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::ClearCell {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 1, 1)
                    .expect("coordinate"),
            }],
        )
        .expect("transaction is valid");
        let error = block_on(service.apply(transaction)).expect_err("stale base must conflict");
        assert!(
            matches!(error, SpreadsheetError::Conflict { .. }),
            "unexpected error: {error}"
        );
    }

    /// §11: repeated idempotency identity returns the same result — and does
    /// not mint a second revision.
    #[test]
    fn repeated_idempotency_returns_same_result() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (base, _block) = publish_workbook(&service, sample_table());

        let transaction = EditTransaction::new(
            base.clone(),
            "idem-repeat".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::SetCell {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 1, 1)
                    .expect("coordinate"),
                value: TableValue::Number(1234.0),
            }],
        )
        .expect("transaction is valid");
        let first = block_on(service.apply(transaction.clone())).expect("first apply");
        let first_ref = match &first {
            SpreadsheetPublication::Workbook { artifact, .. } => artifact.clone(),
            other => panic!("expected a workbook publication, got {other:?}"),
        };
        let revision_files = || {
            std::fs::read_dir(dir.path().join("workbooks").join(&base.artifact_id))
                .expect("artifact dir")
                .filter_map(|entry| {
                    let path = entry.expect("entry").path();
                    (path.extension().is_some_and(|e| e == "xlsx")).then_some(path)
                })
                .count()
        };
        assert_eq!(revision_files(), 2, "base + first applied revision");

        let second = block_on(service.apply(transaction))
            .expect("repeated apply returns the recorded result");
        let second_ref = match &second {
            SpreadsheetPublication::Workbook { artifact, .. } => artifact.clone(),
            other => panic!("expected a workbook publication, got {other:?}"),
        };
        assert_eq!(
            first_ref, second_ref,
            "repeated identity must return the same result"
        );
        assert_eq!(revision_files(), 2, "no new revision minted on repeat");

        // A key reused for a different base is a typed error.
        let wrong_base = EditTransaction::new(
            SpreadsheetArtifactRef::new(
                base.artifact_id.clone(),
                second_ref.revision_id.clone(),
                second_ref.content_digest,
            )
            .expect("ref shape is valid"),
            "idem-repeat".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::ClearCell {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 1, 1)
                    .expect("coordinate"),
            }],
        )
        .expect("transaction is valid");
        let error = block_on(service.apply(wrong_base))
            .expect_err("key reuse for a different base must fail");
        assert!(
            matches!(error, SpreadsheetError::InvalidTransaction { .. }),
            "unexpected error: {error}"
        );
    }

    /// §11: oversized inline data returns a visible typed error (never
    /// truncation) — and the caller may then explicitly choose a workbook.
    #[test]
    fn oversized_inline_publishes_typed_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let big = AnalyticalTable::new(
            "Big".into(),
            "Main".into(),
            vec![TableColumn {
                id: "n".into(),
                label: "N".into(),
                kind: hkask_types::spreadsheet::ColumnKind::Number,
            }],
            (0..(hkask_types::spreadsheet::MAX_INLINE_ROWS + 1))
                .map(|_| vec![TableValue::Number(1.0)])
                .collect(),
        )
        .expect("table is valid");
        let error = block_on(service.publish(
            origin(),
            big,
            PublishOptions {
                access: SpreadsheetAccess::InlineTable,
            },
        ))
        .expect_err("over-cap inline must reject");
        assert!(
            matches!(error, SpreadsheetError::TooLargeForInline { .. }),
            "unexpected error: {error}"
        );
        // The bounded table itself publishes fine as an inline block.
        let small = AnalyticalTable::new(
            "Small".into(),
            "Main".into(),
            vec![TableColumn {
                id: "n".into(),
                label: "N".into(),
                kind: hkask_types::spreadsheet::ColumnKind::Number,
            }],
            vec![vec![TableValue::Number(1.0)]],
        )
        .expect("table is valid");
        let publication = block_on(service.publish(
            origin(),
            small,
            PublishOptions {
                access: SpreadsheetAccess::InlineTable,
            },
        ))
        .expect("small table publishes inline");
        assert!(matches!(publication, SpreadsheetPublication::Inline(_)));
    }

    /// §11: the published SpreadsheetBlock round-trips exactly (byte-exact
    /// JSON) and revalidates.
    #[test]
    fn published_block_round_trips_and_revalidates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (_artifact, block) = publish_workbook(&service, sample_table());

        let json = serde_json::to_string(&block).expect("serialize block");
        let back: SpreadsheetBlock = serde_json::from_str(&json).expect("deserialize block");
        assert_eq!(&back, &block, "round trip changed the block");
        let again = serde_json::to_string(&back).expect("reserialize block");
        assert_eq!(json, again, "round trip changed the bytes");
        back.validate().expect("wire shape revalidates");
        assert_eq!(back.viz, hkask_types::spreadsheet::SPREADSHEET_VIZ);
        assert_eq!(back.mutation.tool.as_deref(), Some(SPREADSHEET_APPLY_TOOL));
    }

    /// §11: unsupported formulas surface as spreadsheet errors. A
    /// parse-invalid formula is rejected typed before apply; a parse-valid
    /// formula with an unknown function is admitted and its cell reads back
    /// as the engine's error value — surfaced, never silent.
    #[test]
    fn unsupported_formula_surfaces_as_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (base, _block) = publish_workbook(&service, sample_table());

        let parse_invalid = EditTransaction::new(
            base.clone(),
            "idem-parse".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::SetFormula {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 4, 1)
                    .expect("coordinate"),
                formula: "=SUM(B2:B3".into(),
            }],
        )
        .expect("transaction is valid");
        let error =
            block_on(service.apply(parse_invalid)).expect_err("parse-invalid formula must reject");
        assert!(
            matches!(error, SpreadsheetError::FormulaInvalid { .. }),
            "unexpected error: {error}"
        );

        let unknown_function = EditTransaction::new(
            base,
            "idem-unknown-fn".into(),
            SpreadsheetAccess::WorkbookWhatIf,
            vec![CellEdit::SetFormula {
                coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 4, 1)
                    .expect("coordinate"),
                formula: "=DEFINITELY_NOT_A_FUNCTION(1)".into(),
            }],
        )
        .expect("transaction is valid");
        let publication =
            block_on(service.apply(unknown_function)).expect("parse-valid formula applies");
        let applied = match publication {
            SpreadsheetPublication::Workbook { artifact, .. } => artifact,
            other => panic!("expected a workbook publication, got {other:?}"),
        };
        let document = block_on(service.open(&applied)).expect("reopen");
        let content = block_on(document.viewport(full_viewport("Main", 5, 3))).expect("viewport");
        match cell(&content, 4, 1) {
            TableValue::Text(s) if s.starts_with('#') => {}
            other => {
                panic!("unknown-function cell must surface an engine error value, got {other:?}")
            }
        }
    }

    /// §6/§5.1: staged edits are local and undoable; viewport reads the
    /// staged state; nothing persists until apply.
    #[test]
    fn staged_edits_undo_redo_via_viewport() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (artifact, _block) = publish_workbook(&service, sample_table());
        let document = block_on(service.open(&artifact)).expect("open");

        let edit = |row: usize, col: usize, value: f64| CellEdit::SetCell {
            coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), row, col)
                .expect("coordinate"),
            value: TableValue::Number(value),
        };

        let before =
            block_on(document.viewport(full_viewport("Main", 4, 3))).expect("viewport before");
        assert_eq!(cell(&before, 1, 1), &TableValue::Number(15000.0));

        block_on(document.stage(vec![edit(1, 1, 424242.0)])).expect("stage");
        let staged =
            block_on(document.viewport(full_viewport("Main", 4, 3))).expect("viewport staged");
        assert_eq!(cell(&staged, 1, 1), &TableValue::Number(424242.0));

        assert!(
            block_on(document.undo()).expect("undo"),
            "an undo step existed"
        );
        let undone =
            block_on(document.viewport(full_viewport("Main", 4, 3))).expect("viewport undone");
        assert_eq!(cell(&undone, 1, 1), &TableValue::Number(15000.0));

        assert!(
            block_on(document.redo()).expect("redo"),
            "a redo step existed"
        );
        let redone =
            block_on(document.viewport(full_viewport("Main", 4, 3))).expect("viewport redone");
        assert_eq!(cell(&redone, 1, 1), &TableValue::Number(424242.0));

        // Staging never persisted: a FRESH ACTOR on the same root reads the
        // revision from disk and still shows the original (the live document
        // handle intentionally keeps its staged state — idempotent open).
        let second_service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("second actor");
        let fresh = block_on(second_service.open(&artifact)).expect("fresh open");
        let original =
            block_on(fresh.viewport(full_viewport("Main", 4, 3))).expect("fresh viewport");
        assert_eq!(cell(&original, 1, 1), &TableValue::Number(15000.0));
    }

    /// §6: DataOnly publishes no block — a typed rejection, not a JSON dump.
    #[test]
    fn dataonly_publish_rejects() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let error = block_on(service.publish(
            origin(),
            sample_table(),
            PublishOptions {
                access: SpreadsheetAccess::DataOnly,
            },
        ))
        .expect_err("DataOnly must reject");
        assert!(
            matches!(error, SpreadsheetError::AccessMismatch { .. }),
            "unexpected error: {error}"
        );
    }

    /// §11 (block constraints): the published block carries no absolute
    /// filesystem path, no workbook bytes, and a bounded viewport.
    #[test]
    fn published_block_carries_only_opaque_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (_artifact, block) = publish_workbook(&service, sample_table());
        let json = serde_json::to_value(&block).expect("serialize block");
        let text = json.to_string();
        assert!(
            !text.contains(dir.path().to_str().expect("utf8 tempdir")),
            "block leaks a filesystem path"
        );
        assert!(block.viewport.row_count <= hkask_types::spreadsheet::MAX_VIEWPORT_ROWS);
        assert!(block.viewport.col_count <= hkask_types::spreadsheet::MAX_VIEWPORT_COLS);
    }

    /// A second open of the same (artifact, revision) must be idempotent: a
    /// second widget for the same block body must not destroy the first
    /// handle's staged state.
    #[test]
    fn reopen_keeps_staged_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service =
            WorkbookService::start_with_root(dir.path().join("workbooks")).expect("service starts");
        let (artifact, _block) = publish_workbook(&service, sample_table());

        let first = block_on(service.open(&artifact)).expect("first open");
        block_on(first.stage(vec![CellEdit::SetCell {
            coordinate: hkask_types::spreadsheet::CellCoordinate::new("Main".into(), 1, 1)
                .expect("coordinate"),
            value: TableValue::Number(777.0),
        }]))
        .expect("stage on first handle");

        // Second open of the same revision: must not reset the document.
        let second = block_on(service.open(&artifact)).expect("second open");
        let viewport = block_on(
            second.viewport(SpreadsheetViewport::new("Main".into(), 0, 0, 3, 3).expect("viewport")),
        )
        .expect("viewport");
        assert_eq!(viewport.cells[1][1], TableValue::Number(777.0));
    }
}
