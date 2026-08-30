//! Company-research notes and file attachments.
use crate::{
    CompaniesServer, map_portfolio_error,
    research_store::{PortfolioError, ResearchStore},
    types::{
        FileAttachRequest, FileDeleteRequest, FileListRequest, NoteAddRequest, NoteDeleteRequest,
        NoteListRequest,
    },
};
use hkask_mcp_server::server::{McpToolError, execute_tool, map_join_error};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

pub(crate) async fn run_store<T>(
    store: ResearchStore,
    operation: impl FnOnce(ResearchStore) -> Result<T, PortfolioError> + Send + 'static,
) -> Result<T, McpToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || operation(store))
        .await
        .map_err(|error| map_join_error(error, "research store task failed"))?
        .map_err(map_portfolio_error)
}

#[tool_router(router = notes_router, vis = "pub")]
impl CompaniesServer {
    // ── Notes & Files tools ─────────────────────────────────────

    #[tool(description = "Add a note to a company/security as of a date")]
    pub async fn note_add(
        &self,
        Parameters(NoteAddRequest {
            portfolio,
            symbol,
            date,
            title,
            body,
            tags,
        }): Parameters<NoteAddRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "note_add", async {
            let id = run_store(self.research.clone(), move |manager| {
                manager.add_note(&portfolio, &symbol, &date, &title, &body, &tags)
            })
            .await?;
            Ok(serde_json::json!({"status": "created", "id": id}))
        })
        .await
    }

    #[tool(description = "List notes for a symbol, optionally filtered by date range or tags")]
    pub async fn note_list(
        &self,
        Parameters(NoteListRequest {
            portfolio,
            symbol,
            date_from,
            date_to,
            tags,
        }): Parameters<NoteListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "note_list",
            async {
                let notes = run_store(self.research.clone(), move |manager| {
                    manager.list_notes(
                        &portfolio,
                        &symbol,
                        date_from.as_deref(),
                        date_to.as_deref(),
                        tags.as_deref(),
                    )
                })
                .await?;
                Ok(serde_json::json!({"notes": notes}))
            },
        )
        .await
    }

    #[tool(description = "Delete a note by ID")]
    pub async fn note_delete(
        &self,
        Parameters(NoteDeleteRequest { note_id }): Parameters<NoteDeleteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "note_delete",
            async {
                let response_note_id = note_id.clone();
                run_store(self.research.clone(), move |manager| {
                    manager.delete_note(&note_id)
                })
                .await?;
                Ok(serde_json::json!({"status": "deleted", "id": response_note_id}))
            },
        )
        .await
    }

    #[tool(description = "Attach a file (base64-encoded) to a company/security")]
    pub async fn file_attach(
        &self,
        Parameters(FileAttachRequest {
            portfolio,
            symbol,
            date,
            filename,
            mime_type,
            data,
            notes,
        }): Parameters<FileAttachRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "file_attach",
            async {
                let id = run_store(self.research.clone(), move |manager| {
                    manager.attach_file(
                        &portfolio, &symbol, &date, &filename, &mime_type, &data, &notes,
                    )
                })
                .await?;
                Ok(serde_json::json!({"status": "attached", "id": id}))
            },
        )
        .await
    }

    #[tool(description = "List attached files for a symbol in a portfolio")]
    pub async fn file_list(
        &self,
        Parameters(FileListRequest { portfolio, symbol }): Parameters<FileListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "file_list",
            async {
                let files = run_store(self.research.clone(), move |manager| {
                    manager.list_files(&portfolio, &symbol)
                })
                .await?;
                Ok(serde_json::json!({"files": files}))
            },
        )
        .await
    }

    #[tool(description = "Delete an attached file by ID — removes record and file from disk")]
    pub async fn file_delete(
        &self,
        Parameters(FileDeleteRequest { file_id }): Parameters<FileDeleteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "file_delete",
            async {
                let response_file_id = file_id.clone();
                run_store(self.research.clone(), move |manager| {
                    manager.delete_file(&file_id)
                })
                .await?;
                Ok(serde_json::json!({"status": "deleted", "id": response_file_id}))
            },
        )
        .await
    }

    // ── Analysis tools ───────────────────────────────────────
}
