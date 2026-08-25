//! Report & screen persistence tools.
//!
//! These tools let the agent (or a skill like company-research-flash) persist
//! the JSON output of any companies-server tool (`expectations_gap`,
//! `company_screener`, `stock_universe`, `dcf_valuation`, etc.) to the
//! companies server's subtree under the kask data root, per the Standardized
//! Artifact Storage layout:
//!
//! - `report_save`   → `mcp/companies/{screens|reports}/{name}.json`
//! - `report_load`   → reads a previously saved artifact
//! - `report_list`   → lists saved artifacts by kind
//!
//! The split between `screens` (a structured candidate list — e.g., the
//! output of `expectations_gap` or `company_screener`) and `reports` (a
//! free-form analysis document — e.g., a company-research-flash thesis) is
//! the caller's choice via the `kind` parameter. Both live under
//! `mcp/companies/` so an operator `ls ~/.local/share/hkask/mcp/companies/`
//! sees every artifact the companies server produced.

use crate::{CompaniesServer, report_store::ArtifactKind};
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

/// Request to save a report or screen artifact.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ReportSaveRequest {
    /// Artifact kind — `"screen"` or `"report"`. Selects the subdirectory.
    pub kind: String,
    /// Flat artifact name (no path separators). Becomes `{name}.json`.
    pub name: String,
    /// The artifact payload as a JSON object.
    pub payload: serde_json::Value,
}

/// Request to load a previously saved artifact.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ReportLoadRequest {
    /// Artifact kind — `"screen"` or `"report"`.
    pub kind: String,
    /// Flat artifact name (no path separators, no extension).
    pub name: String,
}

/// Request to list saved artifacts of a kind.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ReportListRequest {
    /// Artifact kind — `"screen"` or `"report"`.
    pub kind: String,
}

fn parse_kind(raw: &str) -> Result<ArtifactKind, McpToolError> {
    match raw.to_ascii_lowercase().as_str() {
        "screen" => Ok(ArtifactKind::Screen),
        "report" => Ok(ArtifactKind::Report),
        other => Err(McpToolError::invalid_argument(format!(
            "kind must be `screen` or `report`, got `{other}`"
        ))),
    }
}

#[tool_router(router = report_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Persist a JSON artifact (screen or report) produced by the companies server or a skill. Saves to mcp/companies/{screens|reports}/{name}.json under the kask data root. Use kind='screen' for structured candidate lists (expectations_gap, company_screener, stock_universe output) and kind='report' for free-form analysis documents (company-research-flash thesis, deep-dive notes). The name must be flat (no path separators). Returns the full path where the artifact was saved."
    )]
    pub async fn report_save(&self, Parameters(req): Parameters<ReportSaveRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_save",
            Self::ontology_anchor("report_save"),
            async {
                let kind = parse_kind(&req.kind)?;
                let path = self
                    .report_store
                    .save(kind, &req.name, &req.payload)
                    .map_err(McpToolError::internal)?;
                Ok(serde_json::json!({
                    "saved": true,
                    "kind": req.kind,
                    "name": req.name,
                    "path": path.to_string_lossy(),
                }))
            },
        )
        .await
    }

    #[tool(
        description = "Load a previously saved JSON artifact (screen or report) by name. Returns the full JSON payload. Returns a not-found error if no artifact with that name exists."
    )]
    pub async fn report_load(&self, Parameters(req): Parameters<ReportLoadRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_load",
            Self::ontology_anchor("report_load"),
            async {
                let kind = parse_kind(&req.kind)?;
                match self.report_store.load(kind, &req.name) {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) => Err(McpToolError::not_found(format!(
                        "no {} artifact named `{}` — save one via report_save first",
                        req.kind, req.name
                    ))),
                    Err(e) => Err(McpToolError::internal(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "List saved artifact names (without extension) for a kind. Returns a JSON array of names sorted alphabetically. Use kind='screen' or kind='report'."
    )]
    pub async fn report_list(&self, Parameters(req): Parameters<ReportListRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_list",
            Self::ontology_anchor("report_list"),
            async {
                let kind = parse_kind(&req.kind)?;
                let names = self
                    .report_store
                    .list(kind)
                    .map_err(McpToolError::internal)?;
                Ok(serde_json::json!({
                    "kind": req.kind,
                    "count": names.len(),
                    "artifacts": names,
                }))
            },
        )
        .await
    }
}
