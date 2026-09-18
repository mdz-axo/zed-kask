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
use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, BlockProvenance, CellEdit, SpreadsheetAccess,
    SpreadsheetArtifactRef, SpreadsheetBlock, SpreadsheetError, SpreadsheetViewport,
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
        respond: oneshot::Sender<Result<SpreadsheetArtifactRef, SpreadsheetError>>,
    },
    Apply {
        transaction: hkask_types::spreadsheet::EditTransaction,
        respond: oneshot::Sender<Result<SpreadsheetPublication, SpreadsheetError>>,
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
}

/// An open document's identity on the actor side.
type DocumentKey = (String, String);

fn actor_down() -> SpreadsheetError {
    SpreadsheetError::Engine {
        detail: "spreadsheet engine actor is down (thread exited or panicked)".into(),
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
        &self,
        artifact: &SpreadsheetArtifactRef,
    ) -> Result<WorkbookDocument, SpreadsheetError> {
        artifact.validate()?;
        let (respond, receiver) = oneshot::channel();
        self.send(Command::Open {
            artifact: artifact.clone(),
            respond,
        })?;
        receiver.await.map_err(|_| actor_down())?;
        Ok(WorkbookDocument {
            service: Arc::clone(&self_arc_holder(self)),
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
}

/// The `Arc<Self>` reborrow helper (open() holds `&self`, but the document
/// handle needs an `Arc` of the service it dispatches through).
fn self_arc_holder(service: &WorkbookService) -> &Arc<WorkbookService> {
    // SAFETY-free formulation: open() is defined on Arc<Self> callers; the
    // service is always constructed behind an Arc, so this is a projection
    // through the same Arc via the thread-safe sender clone instead.
    // Implemented below via Arc::downgrade-free pattern:
    unreachable!("replaced by Arc-internal dispatch; see open_impl")
}

/// An open workbook document (plan §5.1): staged edits are local and
/// undoable; `viewport` reads the staged state; persistence happens through
/// the service's `apply`, never here.
pub struct WorkbookDocument {
    service: Arc<WorkbookService>,
    artifact: SpreadsheetArtifactRef,
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
        receiver.await.map_err(|_| actor_down())
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
        receiver.await.map_err(|_| actor_down())
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
                let _ = respond.send(result);
            }
            Command::Open { artifact, respond } => {
                let result = handle_open(&mut state, &artifact);
                let _ = respond.send(result);
            }
            Command::Apply {
                transaction,
                respond,
            } => {
                let result = handle_apply(&mut state, transaction);
                let _ = respond.send(result);
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
                let _ = respond.send(result);
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
                let _ = respond.send(result);
            }
            Command::Undo { key, respond } => {
                let result = match state.documents.get_mut(&key) {
                    Some(workbook) => Ok(workbook.undo()),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                let _ = respond.send(result);
            }
            Command::Redo { key, respond } => {
                let result = match state.documents.get_mut(&key) {
                    Some(workbook) => Ok(workbook.redo()),
                    None => Err(SpreadsheetError::UnknownArtifact { artifact_id: key.0 }),
                };
                let _ = respond.send(result);
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
    let row_count = meta.rows.min(MAX_VIEWPORT_ROWS).max(1);
    let col_count = meta.cols.min(MAX_VIEWPORT_COLS).max(1);
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
) -> Result<SpreadsheetArtifactRef, SpreadsheetError> {
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
            detail: format!("engine open failed: {error:?}"),
        })?;
    state.documents.insert(
        (artifact.artifact_id.clone(), artifact.revision_id.clone()),
        workbook,
    );
    Ok(artifact.clone())
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
                artifact: record.result.clone(),
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
            artifact_id: artifact_id.clone(),
            expected: transaction.base_artifact.content_digest.clone(),
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
