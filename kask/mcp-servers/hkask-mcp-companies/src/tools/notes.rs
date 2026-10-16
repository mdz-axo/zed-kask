//! Company-research notes.
use crate::{
    CompaniesServer, map_research_error,
    screen_store::{ResearchError, ResearchStore},
    types::{NoteAddRequest, NoteDeleteRequest, NoteListRequest},
};
use hkask_mcp_server::server::{McpToolError, execute_tool, map_join_error};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

pub(crate) async fn run_store<T>(
    store: ResearchStore,
    operation: impl FnOnce(ResearchStore) -> Result<T, ResearchError> + Send + 'static,
) -> Result<T, McpToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || operation(store))
        .await
        .map_err(|error| map_join_error(error, "research store task failed"))?
        .map_err(map_research_error)
}

#[tool_router(router = notes_router, vis = "pub")]
impl CompaniesServer {
    // ── Notes tools ─────────────────────────────────────────────

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
        execute_tool(self, "note_list", async {
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
        })
        .await
    }

    #[tool(description = "Delete a note by ID")]
    pub async fn note_delete(
        &self,
        Parameters(NoteDeleteRequest { note_id }): Parameters<NoteDeleteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "note_delete", async {
            let response_note_id = note_id.clone();
            run_store(self.research.clone(), move |manager| {
                manager.delete_note(&note_id)
            })
            .await?;
            Ok(serde_json::json!({"status": "deleted", "id": response_note_id}))
        })
        .await
    }
}
