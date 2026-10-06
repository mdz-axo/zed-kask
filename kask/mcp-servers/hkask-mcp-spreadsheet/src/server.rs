//! Spreadsheet MCP server — the central mutation owner (plan §4, §7).
//!
//! Analytical servers remain producers of domain data; `hkask-spreadsheet`
//! owns workbook construction and artifact publication; this server owns
//! persisted spreadsheet edits and interrupted-operation reconciliation.
//! Spreadsheet edits modify derived workbook revisions only — no domain
//! ledger, cache, or journal is ever touched (the §2 authoritative-state
//! boundary, enforced architecturally: this crate links no domain server).

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_spreadsheet::SpreadsheetPublication;
use hkask_spreadsheet::WorkbookService;
use hkask_types::spreadsheet::{EditTransaction, SpreadsheetError};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

hkask_mcp_server::mcp_server!(
    pub struct SpreadsheetServer {
        pub service: Arc<WorkbookService>,
    }
);

/// Dispatch a [`SpreadsheetError`] through the canonical per-variant
/// classification — [`SpreadsheetError::mcp_kind`] is the single owner of
/// the taxonomy (SP-01): this server, the portfolio server, and the
/// companies server classify identically because they all delegate there.
fn map_spreadsheet_error(e: SpreadsheetError) -> McpToolError {
    McpToolError::new(e.mcp_kind(), e.to_string())
}

// ── Request types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpreadsheetApplyRequest {
    /// The edit transaction: base artifact revision + content digest, typed
    /// cell edits, idempotency key, and the expected access mode.
    pub transaction: EditTransaction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpreadsheetOperationGetRequest {
    pub artifact_id: String,
    pub idempotency_key: String,
}

// ── Tool router ─────────────────────────────────────────────────────

#[tool_router(router = spreadsheet_router, vis = "pub")]
impl SpreadsheetServer {
    #[tool(
        description = "Apply typed cell edits against a base workbook revision: verifies the base content digest, applies the edits, and publishes a NEW immutable revision (the base file is never rewritten). Returns the workbook block as a server-authoritative display hint. Idempotent: the same idempotency_key returns the recorded result; a key reused for a different base is invalid_argument. A digest mismatch is a failed_precondition conflict."
    )]
    pub async fn spreadsheet_apply(
        &self,
        Parameters(SpreadsheetApplyRequest { transaction }): Parameters<SpreadsheetApplyRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "spreadsheet_apply", async {
            let publication = self
                .service
                .apply(transaction)
                .await
                .map_err(map_spreadsheet_error)?;
            // The fence format's one owner (SP-01): the publication formats
            // its own display hint.
            let display_hint = publication.display_hint().map_err(map_spreadsheet_error)?;
            match publication {
                SpreadsheetPublication::Workbook { artifact, .. } => Ok(serde_json::json!({
                    "status": "applied",
                    "artifact": artifact,
                    "display_hint": display_hint,
                })),
                // Unreachable in practice — `display_hint` rejects
                // non-workbook publications above — but the response
                // contract names the invariant.
                other => Err(McpToolError::internal(format!(
                    "spreadsheet_apply must produce a workbook publication, got {other:?}"
                ))),
            }
        })
        .await
    }

    #[tool(
        description = "Reconcile an interrupted spreadsheet mutation by its idempotency key. Returns status completed with the recorded result revision when the operation was recorded; status unknown otherwise — the operation may or may not have been applied, so do not blindly retry."
    )]
    pub async fn spreadsheet_operation_get(
        &self,
        Parameters(SpreadsheetOperationGetRequest {
            artifact_id,
            idempotency_key,
        }): Parameters<SpreadsheetOperationGetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "spreadsheet_operation_get", async {
            match self
                .service
                .operation_get(artifact_id, idempotency_key.clone())
                .await
                .map_err(map_spreadsheet_error)?
            {
                Some(record) => Ok(serde_json::json!({
                    "status": "completed",
                    "idempotency_key": record.idempotency_key,
                    "base": record.base,
                    "result": record.result,
                })),
                None => Ok(serde_json::json!({
                    "status": "unknown",
                    "idempotency_key": idempotency_key,
                    "note": "The operation's outcome is unknown — it may or may not have been applied. Do not blindly retry: re-open the base revision and inspect state, or retry under a new idempotency key if duplication is acceptable.",
                })),
            }
        })
        .await
    }
}

#[tool_handler(router = Self::spreadsheet_router())]
impl rmcp::ServerHandler for SpreadsheetServer {}

/// Start the server: the engine actor runs on its dedicated thread inside
/// this process, rooted at the production spreadsheet artifact tree
/// (`~/Documents/zk-data/spreadsheet-mcp/workbooks/`, honoring
/// `HKASK_ARTIFACTS_DIR`).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    let root = hkask_spreadsheet::artifact_store::production_root();
    let service = hkask_spreadsheet::WorkbookService::start_with_root(root).map_err(|error| {
        hkask_mcp_server::McpError::Infrastructure(hkask_types::InfrastructureError::Io(
            error.to_string(),
        ))
    })?;
    hkask_mcp_server::run_server(
        "hkask-mcp-spreadsheet",
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::ServerContext| Ok(SpreadsheetServer::new(ctx.webid, service)),
        vec![],
    )
    .await
}
