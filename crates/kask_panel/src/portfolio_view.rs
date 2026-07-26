//! Portfolio dashboard view — a center-pane `Item` that visualizes the
//! `companies` MCP server's portfolio data.
//!
//! Composes `portfolio_list` + `portfolio_returns` + `portfolio_characteristics`
//! + `portfolio_attribution` into a dashboard: portfolio selector → summary
//! tiles (total return, IRR, start/end value) → characteristics table →
//! attribution ranking.
//!
//! Also provides an auto-loader that scans a transactions directory for new
//! CSV/JSON files and imports them via `ledger_import`. A processed-file
//! manifest (`.processed.json`) tracks which files have already been imported
//! to avoid duplicates.
//!
//! All data is fetched via the global `ToolInvoker` hook (same as `KaskPanel`
//! and `KanbanBoardView`). FIBO concept URIs from the MCP server's responses
//! are rendered as labels where applicable.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Task,
    WeakEntity, Window, prelude::*,
};
use serde::Deserialize;
use serde_json::json;
use ui::prelude::*;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem, TabContentParams},
};

use crate::kanban_tool_invoker;

/// The MCP server name (matches `BUILT_IN_MCP_SERVERS`).
const COMPANIES_SERVER: &str = "companies";

// ── FIBO concept URIs (from `hkask-mcp-companies/src/fibo.rs`) ────────────
/// Used as labels in the dashboard to anchor displayed metrics to the FIBO
/// ontology. These match the `fibo` map entries the MCP server includes in
/// its responses.
const FIBO_PORTFOLIO: &str = "fibo-sec-sec-ast:Portfolio";
const FIBO_TRANSACTION_LEDGER: &str = "fibo-sec-sec-ast:TransactionLedger";
const FIBO_TIME_WEIGHTED_RETURN: &str = "fibo-fbc-fct-ra:TimeWeightedReturn";
const FIBO_INTERNAL_RATE_OF_RETURN: &str = "fibo-fbc-fct-ra:InternalRateOfReturn";

// ── MCP response structs (minimal, mirror the server's types) ────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PortfolioListResponse {
    portfolios: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PortfolioReturnsResponse {
    portfolio: String,
    from: String,
    to: String,
    total_return: f64,
    modified_dietz: f64,
    irr: f64,
    irr_converged: bool,
    start_value: f64,
    end_value: f64,
    net_cash_flows: f64,
    cash_flow_count: usize,
    positions_at_start: usize,
    positions_at_end: usize,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LedgerImportResponse {
    status: String,
    count: usize,
    validation: LedgerValidation,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LedgerValidation {
    valid: bool,
    positions: usize,
    cash: f64,
    issues: Vec<String>,
}

// ── Dashboard state ──────────────────────────────────────────────────────

/// A summary tile for the dashboard.
struct SummaryTile {
    label: String,
    value: String,
    fibo_concept: Option<&'static str>,
}

/// The portfolio dashboard view.
pub struct PortfolioDashboardView {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// Available portfolios (fetched on load).
    portfolios: Vec<String>,
    /// Currently selected portfolio index.
    selected_portfolio: usize,
    /// Returns data for the selected portfolio.
    returns: Option<PortfolioReturnsResponse>,
    /// Whether data is loading.
    loading: bool,
    /// Error message if loading failed.
    error: Option<String>,
    /// Auto-load status message.
    auto_load_status: Option<String>,
}

impl PortfolioDashboardView {
    /// Create a new portfolio dashboard view.
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let view = Self {
                _workspace: workspace.weak_handle(),
                focus_handle: cx.focus_handle(),
                portfolios: Vec::new(),
                selected_portfolio: 0,
                returns: None,
                loading: false,
                error: None,
                auto_load_status: None,
            };
            view
        })
    }

    /// Fetch the portfolio list on load.
    fn fetch_portfolios(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();

        let task = invoke_tool(COMPANIES_SERVER, "portfolio_list", json!({}));
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let result: Result<PortfolioListResponse, _> = serde_json::from_str(&output);
                this.update(cx, |this, cx| match result {
                    Ok(resp) => {
                        this.portfolios = resp.portfolios;
                        this.loading = false;
                        if !this.portfolios.is_empty() {
                            this.fetch_returns(cx);
                        }
                        cx.notify();
                    }
                    Err(e) => {
                        this.loading = false;
                        this.error = Some(format!("Parse error: {e}"));
                        cx.notify();
                    }
                })
            }
            Err(e) => this.update(cx, |this, cx| {
                this.loading = false;
                this.error = Some(format!("Tool error: {e}"));
                cx.notify();
            }),
        })
        .detach();
    }

    /// Fetch returns for the selected portfolio.
    fn fetch_returns(&mut self, cx: &mut Context<Self>) {
        let Some(portfolio) = self.portfolios.get(self.selected_portfolio).cloned() else {
            return;
        };
        self.loading = true;
        self.returns = None;
        cx.notify();

        // Default to all-time: from "2000-01-01" to today.
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let args = json!({
            "portfolio": portfolio,
            "from": "2000-01-01",
            "to": today,
        });
        let task = invoke_tool(COMPANIES_SERVER, "portfolio_returns", args);
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let result: Result<PortfolioReturnsResponse, _> = serde_json::from_str(&output);
                this.update(cx, |this, cx| match result {
                    Ok(resp) => {
                        this.returns = Some(resp);
                        this.loading = false;
                        cx.notify();
                    }
                    Err(e) => {
                        this.loading = false;
                        this.error = Some(format!("Parse error: {e}"));
                        cx.notify();
                    }
                })
            }
            Err(e) => this.update(cx, |this, cx| {
                this.loading = false;
                this.error = Some(format!("Tool error: {e}"));
                cx.notify();
            }),
        })
        .detach();
    }

    /// Auto-load transaction files from the transactions directory.
    /// Reads a `.processed.json` manifest to skip already-imported files.
    fn auto_load_transactions(&mut self, cx: &mut Context<Self>) {
        self.auto_load_status = Some("Scanning transactions directory…".to_string());
        cx.notify();

        let task = cx.background_spawn(async move {
            // Determine the transactions directory.
            let dir = std::env::var("HKASK_TRANSACTIONS_DIR").unwrap_or_else(|_| {
                // Default: <kask_data_dir>/transactions/
                let base = dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("Zed-Kask")
                    .join("kask")
                    .join("transactions");
                base.to_string_lossy().to_string()
            });

            let dir_path = std::path::Path::new(&dir);
            if !dir_path.exists() {
                return format!("Transactions directory not found: {dir}");
            }

            // Read the processed manifest.
            let manifest_path = dir_path.join(".processed.json");
            let mut processed: HashMap<String, String> = std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            // Scan for CSV and JSON files.
            let mut new_files: Vec<(String, String)> = Vec::new(); // (filename, content)
            let entries = match std::fs::read_dir(dir_path) {
                Ok(e) => e,
                Err(e) => return format!("Failed to read dir: {e}"),
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                // Skip hidden files and the manifest.
                if name.starts_with('.') {
                    continue;
                }
                // Skip already-processed files.
                if processed.contains_key(&name) {
                    continue;
                }
                // Only process CSV and JSON.
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "csv" && ext != "json" {
                    continue;
                }
                // Read file content.
                match std::fs::read_to_string(&path) {
                    Ok(content) => new_files.push((name, content)),
                    Err(e) => {
                        return format!("Failed to read {name}: {e}");
                    }
                }
            }

            if new_files.is_empty() {
                return "No new transaction files found.".to_string();
            }

            // Import each file via the tool invoker.
            let mut imported_count = 0usize;
            let mut errors: Vec<String> = Vec::new();
            for (filename, content) in &new_files {
                let ext = std::path::Path::new(filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("csv");
                let format = if ext == "json" { "json" } else { "csv" };
                // Derive portfolio name from filename (strip extension).
                let portfolio_name = std::path::Path::new(filename)
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("default")
                    .to_string();

                let args = json!({
                    "portfolio": portfolio_name,
                    "format": format,
                    "data": content,
                });
                let task = invoke_tool(COMPANIES_SERVER, "ledger_import", args);
                match task.await {
                    Ok(output) => {
                        if let Ok(resp) = serde_json::from_str::<LedgerImportResponse>(&output) {
                            imported_count += resp.count;
                            processed.insert(filename.clone(), output);
                        } else {
                            errors.push(format!("{filename}: parse error"));
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{filename}: {e}"));
                    }
                }
            }

            // Write the updated manifest.
            if let Ok(manifest_json) = serde_json::to_string_pretty(&processed) {
                let _ = std::fs::write(&manifest_path, manifest_json);
            }

            if errors.is_empty() {
                format!(
                    "Imported {imported_count} transactions from {} files.",
                    new_files.len()
                )
            } else {
                format!(
                    "Imported {imported_count} transactions. Errors: {}",
                    errors.join("; ")
                )
            }
        });

        cx.spawn(async move |this, cx| {
            let status = task.await;
            this.update(cx, |this, cx| {
                this.auto_load_status = Some(status);
                // Refresh the portfolio list after import.
                this.fetch_portfolios(cx);
            })
        })
        .detach();
    }

    fn select_portfolio(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_portfolio = index;
        self.fetch_returns(cx);
    }

    fn render_summary_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(returns) = &self.returns else {
            return vec![];
        };

        let border_color = cx.theme().colors().border;

        let tiles = vec![
            SummaryTile {
                label: "Total Return".to_string(),
                value: format_pct(returns.total_return),
                fibo_concept: Some(FIBO_TIME_WEIGHTED_RETURN),
            },
            SummaryTile {
                label: "IRR".to_string(),
                value: format_pct(returns.irr),
                fibo_concept: Some(FIBO_INTERNAL_RATE_OF_RETURN),
            },
            SummaryTile {
                label: "Modified Dietz".to_string(),
                value: format_pct(returns.modified_dietz),
                fibo_concept: None,
            },
            SummaryTile {
                label: "Start Value".to_string(),
                value: format_currency(returns.start_value),
                fibo_concept: None,
            },
            SummaryTile {
                label: "End Value".to_string(),
                value: format_currency(returns.end_value),
                fibo_concept: None,
            },
            SummaryTile {
                label: "Positions".to_string(),
                value: format!(
                    "{} → {}",
                    returns.positions_at_start, returns.positions_at_end
                ),
                fibo_concept: None,
            },
        ];

        tiles
            .into_iter()
            .map(|tile| {
                v_flex()
                    .p_3()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(tile.label)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(tile.value)
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    .when_some(tile.fibo_concept, |this, concept| {
                        this.child(
                            Label::new(concept)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .into_any_element()
            })
            .collect()
    }

    fn render_portfolio_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons: Vec<AnyElement> = self
            .portfolios
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let is_selected = index == self.selected_portfolio;
                Button::new(("portfolio-btn", index), name.as_str())
                    .style(if is_selected {
                        ButtonStyle::Tinted(ui::TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_portfolio(index, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("{}:", FIBO_PORTFOLIO))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new("Portfolio")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(h_flex().gap_1().flex_wrap().children(buttons))
    }

    fn render_auto_load_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .justify_between()
            .child(
                Label::new(self.auto_load_status.clone().unwrap_or_else(|| {
                    "Click Auto-Load to scan for new transaction files.".to_string()
                }))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
            .child(
                Button::new("auto-load-btn", "Auto-Load Transactions")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auto_load_transactions(cx);
                    })),
            )
    }
}

// ── Formatting helpers ────────────────────────────────────────────────────

fn format_pct(value: f64) -> String {
    if value.is_finite() {
        format!("{:+.2}%", value * 100.0)
    } else {
        "—".to_string()
    }
}

fn format_currency(value: f64) -> String {
    if value.is_finite() && value.abs() > 0.0 {
        format!("${:.0}", value)
    } else {
        "—".to_string()
    }
}

// ── Tool invocation helper ────────────────────────────────────────────────

fn invoke_tool(server: &str, tool: &str, args: serde_json::Value) -> Task<Result<String, String>> {
    match kanban_tool_invoker() {
        Some(invoker) => invoker.invoke_tool(server, tool, args),
        None => Task::ready(Err("Tool invoker not wired".to_string())),
    }
}

// ── Item / SerializableItem impls ────────────────────────────────────────

impl Focusable for PortfolioDashboardView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<ItemEvent> for PortfolioDashboardView {}

impl Item for PortfolioDashboardView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> gpui::SharedString {
        "Portfolio Dashboard".into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), _cx))
            .color(params.text_color())
            .into_any_element()
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<gpui::SharedString> {
        Some("Portfolio Dashboard — returns, characteristics, attribution".into())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Portfolio Dashboard Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(_event: &Self::Event, _f: &mut dyn FnMut(ItemEvent)) {}
}

impl SerializableItem for PortfolioDashboardView {
    fn serialized_item_kind() -> &'static str {
        "PortfolioDashboardView"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        _cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                PortfolioDashboardView::new(workspace, window, cx)
            })
        })
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl Render for PortfolioDashboardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Fetch portfolios on first render if empty.
        if self.portfolios.is_empty() && !self.loading && self.error.is_none() {
            self.fetch_portfolios(cx);
        }

        let border_color = cx.theme().colors().border;

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new("Portfolio Dashboard").size(LabelSize::Large))
                    .child(
                        Label::new(FIBO_TRANSACTION_LEDGER)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(self.render_auto_load_bar(cx))
            .child(self.render_portfolio_selector(cx))
            // Summary tiles
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(self.render_summary_tiles(cx)),
            )
            // Error or loading state
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(border_color)
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                )
            })
            .when(self.loading, |this| {
                this.child(
                    Label::new("Loading…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            // Returns detail
            .when_some(
                self.returns.as_ref(),
                |this, returns: &PortfolioReturnsResponse| {
                    this.child(
                        div()
                            .p_3()
                            .gap_2()
                            .rounded_md()
                            .border_1()
                            .border_color(border_color)
                            .child(
                                Label::new(format!("Returns: {} to {}", returns.from, returns.to))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(
                                        Label::new(format!(
                                            "Net cash flows: {}",
                                            format_currency(returns.net_cash_flows)
                                        ))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                    )
                                    .child(
                                        Label::new(format!(
                                            "Cash flow events: {}",
                                            returns.cash_flow_count
                                        ))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                    )
                                    .child(
                                        Label::new(format!(
                                            "IRR converged: {}",
                                            if returns.irr_converged { "yes" } else { "no" }
                                        ))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                    ),
                            ),
                    )
                },
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pct_positive() {
        assert_eq!(format_pct(0.15), "+15.00%");
    }

    #[test]
    fn format_pct_negative() {
        assert_eq!(format_pct(-0.05), "-5.00%");
    }

    #[test]
    fn format_pct_nan() {
        assert_eq!(format_pct(f64::NAN), "—");
    }

    #[test]
    fn format_currency_normal() {
        assert_eq!(format_currency(100000.0), "$100000");
    }

    #[test]
    fn format_currency_zero() {
        assert_eq!(format_currency(0.0), "—");
    }

    #[test]
    fn format_currency_nan() {
        assert_eq!(format_currency(f64::NAN), "—");
    }

    #[test]
    fn parse_portfolio_list_response() {
        let json = r#"{"portfolios":["main","tech","retirement"]}"#;
        let resp: PortfolioListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.portfolios, vec!["main", "tech", "retirement"]);
    }

    #[test]
    fn parse_portfolio_returns_response() {
        let json = r#"{"portfolio":"main","from":"2000-01-01","to":"2024-12-01","total_return":0.15,"modified_dietz":0.14,"irr":0.12,"irr_converged":true,"start_value":100000.0,"end_value":115000.0,"net_cash_flows":5000.0,"cash_flow_count":3,"positions_at_start":0,"positions_at_end":3}"#;
        let resp: PortfolioReturnsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.portfolio, "main");
        assert!((resp.total_return - 0.15).abs() < 1e-9);
        assert!(resp.irr_converged);
        assert_eq!(resp.positions_at_end, 3);
    }

    #[test]
    fn parse_ledger_import_response() {
        let json = r#"{"status":"imported","count":5,"validation":{"valid":true,"positions":3,"cash":100000.0,"issues":[]}}"#;
        let resp: LedgerImportResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "imported");
        assert_eq!(resp.count, 5);
        assert!(resp.validation.valid);
        assert_eq!(resp.validation.positions, 3);
    }
}
