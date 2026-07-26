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

/// A single characteristic field from `portfolio_characteristics`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct CharacteristicField {
    value: Option<f64>,
    fibo: Option<String>,
    method: Option<String>,
    holdings: Option<usize>,
}

/// A single attribution row from `portfolio_attribution`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct AttributionRow {
    symbol: String,
    weight_start_pct: f64,
    weight_end_pct: f64,
    security_return_pct: f64,
    contribution_bps: f64,
    gain_loss: f64,
}

/// A position from `portfolio_comparison`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ComparisonPosition {
    symbol: String,
    shares_a: Option<f64>,
    shares_b: Option<f64>,
    shares: Option<f64>,
}

/// Aggregation method selector (matches the MCP server's enum).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AggregationMethod {
    #[default]
    WeightedArithmetic,
    WeightedHarmonic,
    WeightedMedian,
    Winsorized,
}

impl AggregationMethod {
    fn as_str(&self) -> &'static str {
        match self {
            Self::WeightedArithmetic => "weighted_arithmetic",
            Self::WeightedHarmonic => "weighted_harmonic",
            Self::WeightedMedian => "weighted_median",
            Self::Winsorized => "winsorized",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::WeightedArithmetic => "Wtd Arithmetic",
            Self::WeightedHarmonic => "Wtd Harmonic",
            Self::WeightedMedian => "Wtd Median",
            Self::Winsorized => "Winsorized 5/95",
        }
    }

    fn next(&self) -> Self {
        match self {
            Self::WeightedArithmetic => Self::WeightedHarmonic,
            Self::WeightedHarmonic => Self::WeightedMedian,
            Self::WeightedMedian => Self::Winsorized,
            Self::Winsorized => Self::WeightedArithmetic,
        }
    }
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
    /// Characteristics data for the selected portfolio (field name → field).
    characteristics: Vec<(String, CharacteristicField)>,
    /// Attribution data for the selected portfolio.
    attribution: Vec<AttributionRow>,
    /// Comparison data (if in comparison mode).
    comparison: Option<ComparisonData>,
    /// Second portfolio for comparison mode (None = single-portfolio mode).
    compare_portfolio: Option<usize>,
    /// Aggregation method for characteristics.
    aggregation: AggregationMethod,
    /// Date range for returns/attribution.
    from_date: String,
    to_date: String,
    /// Whether data is loading.
    loading: bool,
    /// Error message if loading failed.
    error: Option<String>,
    /// Auto-load status message.
    auto_load_status: Option<String>,
    /// Whether auto-load has been run this session.
    auto_loaded: bool,
}

/// Comparison data from `portfolio_comparison`.
#[derive(Debug, Clone)]
struct ComparisonData {
    portfolio_a: String,
    portfolio_b: String,
    shared: Vec<ComparisonPosition>,
    only_a: Vec<ComparisonPosition>,
    only_b: Vec<ComparisonPosition>,
}

impl PortfolioDashboardView {
    /// Create a new portfolio dashboard view.
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            Self {
                _workspace: workspace.weak_handle(),
                focus_handle: cx.focus_handle(),
                portfolios: Vec::new(),
                selected_portfolio: 0,
                returns: None,
                characteristics: Vec::new(),
                attribution: Vec::new(),
                comparison: None,
                compare_portfolio: None,
                aggregation: AggregationMethod::default(),
                from_date: "2000-01-01".to_string(),
                to_date: today,
                loading: false,
                error: None,
                auto_load_status: None,
                auto_loaded: false,
            }
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
                            this.fetch_all(cx);
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

    /// Fetch all data for the selected portfolio: returns, characteristics,
    /// and attribution. Called on portfolio selection or date/aggregation change.
    fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(portfolio) = self.portfolios.get(self.selected_portfolio).cloned() else {
            return;
        };
        self.loading = true;
        self.returns = None;
        self.characteristics = Vec::new();
        self.attribution = Vec::new();
        self.error = None;
        cx.notify();

        let from = self.from_date.clone();
        let to = self.to_date.clone();
        let aggregation = self.aggregation.as_str().to_string();
        let today = self.to_date.clone();

        // Fetch returns.
        let returns_args = json!({
            "portfolio": portfolio,
            "from": from,
            "to": to,
        });
        let returns_task = invoke_tool(COMPANIES_SERVER, "portfolio_returns", returns_args);

        // Fetch characteristics (as-of the `to` date).
        let char_args = json!({
            "portfolio": portfolio,
            "date": today,
            "aggregation": aggregation,
        });
        let char_task = invoke_tool(COMPANIES_SERVER, "portfolio_characteristics", char_args);

        // Fetch attribution.
        let attr_args = json!({
            "portfolio": portfolio,
            "from": from,
            "to": to,
        });
        let attr_task = invoke_tool(COMPANIES_SERVER, "portfolio_attribution", attr_args);

        cx.spawn(async move |this, cx| {
            // Run all three fetches concurrently.
            let (returns_result, char_result, attr_result) =
                futures::join!(returns_task, char_task, attr_task);

            this.update(cx, |this, cx| {
                this.loading = false;

                // Process returns.
                match returns_result {
                    Ok(output) => match serde_json::from_str::<PortfolioReturnsResponse>(&output) {
                        Ok(resp) => this.returns = Some(resp),
                        Err(e) => {
                            this.error = Some(format!("Returns parse error: {e}"));
                        }
                    },
                    Err(e) => {
                        this.error = Some(format!("Returns tool error: {e}"));
                    }
                }

                // Process characteristics.
                if let Ok(output) = char_result {
                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&output) {
                        if let Some(chars) = obj.get("characteristics").and_then(|c| c.as_object())
                        {
                            this.characteristics = chars
                                .iter()
                                .filter_map(|(k, v)| {
                                    serde_json::from_value::<CharacteristicField>(v.clone())
                                        .ok()
                                        .map(|f| (k.clone(), f))
                                })
                                .collect();
                        }
                    }
                }

                // Process attribution.
                if let Ok(output) = attr_result {
                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&output) {
                        if let Some(rows) = obj.get("attribution").and_then(|a| a.as_array()) {
                            this.attribution = rows
                                .iter()
                                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                .collect();
                        }
                    }
                }

                cx.notify();
            })
        })
        .detach();
    }

    /// Fetch comparison data between two portfolios.
    fn fetch_comparison(&mut self, cx: &mut Context<Self>) {
        let Some(portfolio_a) = self.portfolios.get(self.selected_portfolio).cloned() else {
            return;
        };
        let Some(portfolio_b) = self
            .compare_portfolio
            .and_then(|i| self.portfolios.get(i).cloned())
        else {
            return;
        };
        self.loading = true;
        self.comparison = None;
        cx.notify();

        let args = json!({
            "portfolio_a": portfolio_a,
            "portfolio_b": portfolio_b,
        });
        let task = invoke_tool(COMPANIES_SERVER, "portfolio_comparison", args);
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => this.update(cx, |this, cx| {
                this.loading = false;
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&output) {
                    let shared = obj
                        .get("shared_positions")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    let only_a = obj
                        .get("only_in_a")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    let only_b = obj
                        .get("only_in_b")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    this.comparison = Some(ComparisonData {
                        portfolio_a,
                        portfolio_b,
                        shared,
                        only_a,
                        only_b,
                    });
                }
                cx.notify();
            }),
            Err(e) => this.update(cx, |this, cx| {
                this.loading = false;
                this.error = Some(format!("Comparison error: {e}"));
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
        self.fetch_all(cx);
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

    /// Render the aggregation method selector and date range controls.
    fn render_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .justify_between()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Label::new("Aggregation:")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Button::new("agg-btn", self.aggregation.label())
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.aggregation = this.aggregation.next();
                                this.fetch_all(cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("{} → {}", self.from_date, self.to_date))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Button::new("ytd-btn", "YTD")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let year = chrono::Utc::now().format("%Y").to_string();
                                this.from_date = format!("{year}-01-01");
                                this.to_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
                                this.fetch_all(cx);
                            })),
                    )
                    .child(
                        Button::new("all-btn", "All")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.from_date = "2000-01-01".to_string();
                                this.to_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
                                this.fetch_all(cx);
                            })),
                    ),
            )
    }

    /// Render the characteristics table.
    fn render_characteristics(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        if self.characteristics.is_empty() {
            return div().into_any_element();
        }

        let rows: Vec<AnyElement> = self
            .characteristics
            .iter()
            .map(|(field, char)| {
                let value = char
                    .value
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "—".to_string());
                let fibo = char.fibo.clone().unwrap_or_default();
                let holdings = char.holdings.unwrap_or(0);
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Label::new(field.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(value)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(format!("({holdings} holdings)"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .when(!fibo.is_empty(), |this| {
                        this.child(Label::new(fibo).size(LabelSize::XSmall).color(Color::Muted))
                    })
                    .into_any_element()
            })
            .collect();

        div()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new("Characteristics")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }

    /// Render the attribution ranking.
    fn render_attribution(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        if self.attribution.is_empty() {
            return div().into_any_element();
        }

        let rows: Vec<AnyElement> = self
            .attribution
            .iter()
            .take(20) // Top 20 by absolute contribution (already sorted by server)
            .map(|row| {
                let color = if row.contribution_bps >= 0.0 {
                    Color::Created
                } else {
                    Color::Error
                };
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(row.symbol.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("{:+.0} bps", row.contribution_bps))
                            .size(LabelSize::XSmall)
                            .color(color),
                    )
                    .child(
                        Label::new(format!("{:+.2}%", row.security_return_pct))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("{:.1}% wgt", row.weight_start_pct))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new("Attribution (top 20 by impact)")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }

    /// Render the comparison table (if in comparison mode).
    fn render_comparison(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let Some(comp) = &self.comparison else {
            return div().into_any_element();
        };

        let shared_rows: Vec<AnyElement> = comp
            .shared
            .iter()
            .map(|pos| {
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(pos.symbol.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!(
                            "A: {} / B: {}",
                            pos.shares_a.unwrap_or(0.0),
                            pos.shares_b.unwrap_or(0.0)
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        let only_a_rows: Vec<AnyElement> = comp
            .only_a
            .iter()
            .map(|pos| {
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(pos.symbol.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("A only: {}", pos.shares.unwrap_or(0.0)))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        let only_b_rows: Vec<AnyElement> = comp
            .only_b
            .iter()
            .map(|pos| {
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(pos.symbol.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("B only: {}", pos.shares.unwrap_or(0.0)))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!(
                    "Comparison: {} vs {}",
                    comp.portfolio_a, comp.portfolio_b
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(
                Label::new(format!(
                    "Shared: {} | Only A: {} | Only B: {}",
                    comp.shared.len(),
                    comp.only_a.len(),
                    comp.only_b.len()
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
            .children(shared_rows)
            .children(only_a_rows)
            .children(only_b_rows)
            .into_any_element()
    }

    /// Render the comparison mode toggle and second portfolio selector.
    fn render_compare_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let compare_buttons: Vec<AnyElement> = self
            .portfolios
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let is_selected = self.compare_portfolio == Some(index);
                Button::new(("compare-btn", index), name.as_str())
                    .style(if is_selected {
                        ButtonStyle::Tinted(ui::TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.compare_portfolio == Some(index) {
                            // Deselect → exit comparison mode.
                            this.compare_portfolio = None;
                            this.comparison = None;
                        } else {
                            this.compare_portfolio = Some(index);
                            this.fetch_comparison(cx);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .child(
                Label::new("Compare with (click to toggle):")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(h_flex().gap_1().flex_wrap().children(compare_buttons))
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

        // Auto-load on first render if not yet done.
        if !self.auto_loaded && !self.loading {
            self.auto_loaded = true;
            self.auto_load_transactions(cx);
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
            .child(self.render_compare_selector(cx))
            .child(self.render_controls(cx))
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
            // Characteristics table
            .child(self.render_characteristics(cx))
            // Attribution ranking
            .child(self.render_attribution(cx))
            // Comparison (if in comparison mode)
            .child(self.render_comparison(cx))
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
