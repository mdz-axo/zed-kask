//! The GPUI spreadsheet widget (plan §8): renders a ```` ```spreadsheet ````
//! block's workbook, stages edits locally with undo/redo, and persists
//! through the governed `ToolInvoker` seam.
//!
//! Layout discipline (measured): the chrome carries at most three primary
//! actions (Undo, Redo, Save — within the ≤5 budget); fixed elements
//! (row/column headers, cell references, action labels) are
//! `flex_shrink_0`; the only flexible text columns (formula bar, status
//! line) are `min_w_0` + `truncate()`. The grid paints only the current
//! aligned fetch window — at most
//! [`WIDGET_WINDOW_ROWS`] × [`WIDGET_WINDOW_COLS`] cells — so the painted
//! element count is bounded by construction for any navigation target.

use std::sync::Arc;
use std::sync::OnceLock;

use gpui::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, Keystroke, ParentElement, Render, Styled, Window, div,
};
use gpui_util::ResultExt as _;
use hkask_spreadsheet::{ViewportContent, WorkbookDocument, WorkbookService};
use hkask_tool_invoker::{InvokeError, shared_tool_invoker};
use hkask_types::spreadsheet::{
    CellEdit, EditTransaction, SpreadsheetAccess, SpreadsheetBlock, SpreadsheetViewport, TableValue,
};
use ui::prelude::*;

use crate::block::SpreadsheetBlockBody;
use crate::logic::{
    Nav, Rect, commit_to_edit, editor_text, move_active, selection_rect, selection_to_tsv,
    tsv_to_edits, value_to_text, window_contains, window_covering,
};

/// The spreadsheet MCP server's settings id — the mutation endpoint the
/// widget dispatches `spreadsheet_apply` to.
const SPREADSHEET_SERVER: &str = "spreadsheet";
const SPREADSHEET_APPLY: &str = "spreadsheet_apply";

/// The editor-process engine actor, started lazily on first widget use.
/// Documents are keyed by (artifact, revision) inside the actor, so the
/// widget opens once per revision and holds the [`WorkbookDocument`] handle
/// — re-opening a live revision would reset its staged state.
static SHARED_SERVICE: OnceLock<Result<Arc<WorkbookService>, String>> = OnceLock::new();

/// The shared editor-process engine service. Start failures are cached (an
/// unwritable artifact root is not transient) and surfaced by the widget.
pub fn shared_spreadsheet_service() -> Result<&'static Arc<WorkbookService>, String> {
    SHARED_SERVICE
        .get_or_init(|| {
            let root = hkask_spreadsheet::artifact_store::production_root();
            WorkbookService::start_with_root(root)
                .map_err(|error| format!("engine actor failed to start: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The visible save/dispatch lifecycle (§8: "Save status with visible
/// conflict and interruption states"; the widget visibly distinguishes all
/// four `InvokeError` outcomes).
#[derive(Debug, Clone, PartialEq, Eq)]
enum SaveStatus {
    Idle,
    Saving,
    Saved,
    NotWired,
    Unavailable,
    /// The outcome is unknown; the widget never auto-replays (§7).
    Interrupted,
    Failed(String),
}

impl SaveStatus {
    fn label(&self) -> String {
        match self {
            SaveStatus::Idle => String::new(),
            SaveStatus::Saving => "Saving …".into(),
            SaveStatus::Saved => "Saved (new revision)".into(),
            SaveStatus::NotWired => "Not wired: MCP servers are not connected yet".into(),
            SaveStatus::Unavailable => "Unavailable: the request never left — retry is safe".into(),
            SaveStatus::Interrupted => {
                "Interrupted: outcome UNKNOWN — reconcile via spreadsheet_operation_get; \
                 do not blindly retry"
                    .into()
            }
            SaveStatus::Failed(message) => format!("Save failed: {message}"),
        }
    }

    fn from_invoke_error(error: &InvokeError) -> Self {
        match error {
            InvokeError::NotWired => SaveStatus::NotWired,
            InvokeError::Unavailable(_) => SaveStatus::Unavailable,
            InvokeError::Interrupted(_) => SaveStatus::Interrupted,
            InvokeError::Failed(_) => {
                let message = error.message();
                // An apply-level conflict (stale base digest) surfaces as the
                // spreadsheet server's failed_precondition envelope.
                if message.contains("failed_precondition") || message.contains("Conflict") {
                    SaveStatus::Failed(
                        "conflict — the base revision changed; re-open the current \
                         revision and restage"
                            .into(),
                    )
                } else {
                    SaveStatus::Failed(message)
                }
            }
        }
    }
}

/// The editing state: one text buffer shown in both the active cell and
/// the formula bar (they edit the same cell, Excel-style).
#[derive(Debug, Clone)]
struct EditorState {
    cell: (usize, usize),
    text: String,
}

pub struct SpreadsheetWidget {
    focus_handle: FocusHandle,
    block: Result<SpreadsheetBlock, String>,
    service_error: Option<String>,
    document: Option<WorkbookDocument>,
    sheets: Vec<String>,
    active_sheet: String,
    window: Option<SpreadsheetViewport>,
    content: Option<ViewportContent>,
    loading: bool,
    active_cell: (usize, usize),
    selection_anchor: Option<(usize, usize)>,
    selection_extent: Option<(usize, usize)>,
    editor: Option<EditorState>,
    /// The widget-side mirror of staged batches (one entry per stage/undo
    /// step); the save payload is its flattening. Cleared on successful
    /// save; kept on `Interrupted` (the outcome is unknown).
    staged_batches: Vec<Vec<CellEdit>>,
    /// Batches undone locally, eligible for redo. Cleared by new staging
    /// (matching engine semantics).
    redo_buffer: Vec<Vec<CellEdit>>,
    save_status: SaveStatus,
    load_error: Option<String>,
}

impl SpreadsheetWidget {
    /// The viz-core constructor: tolerant body in, widget out.
    pub fn new(body: SpreadsheetBlockBody, cx: &mut Context<Self>) -> Self {
        match shared_spreadsheet_service() {
            Ok(service) => Self::new_with_service(body, Arc::clone(service), cx),
            Err(error) => Self::degraded(body.strict_block(), error, cx),
        }
    }

    fn degraded(
        block: Result<SpreadsheetBlock, String>,
        error: String,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            block,
            service_error: Some(error),
            document: None,
            sheets: Vec::new(),
            active_sheet: String::new(),
            window: None,
            content: None,
            loading: false,
            active_cell: (0, 0),
            selection_anchor: None,
            selection_extent: None,
            editor: None,
            staged_batches: Vec::new(),
            redo_buffer: Vec::new(),
            save_status: SaveStatus::Idle,
            load_error: None,
        }
    }

    /// The test/construction seam: an explicit engine service.
    pub fn new_with_service(
        body: SpreadsheetBlockBody,
        service: Arc<WorkbookService>,
        cx: &mut Context<Self>,
    ) -> Self {
        let block = body.strict_block();
        let active_sheet = match &block {
            Ok(block) => block.active_sheet.clone(),
            Err(_) => String::new(),
        };
        let mut widget = Self {
            focus_handle: cx.focus_handle(),
            block: block.clone(),
            service_error: None,
            document: None,
            sheets: Vec::new(),
            active_sheet,
            window: None,
            content: None,
            loading: block.is_ok(),
            active_cell: (0, 0),
            selection_anchor: None,
            selection_extent: None,
            editor: None,
            staged_batches: Vec::new(),
            redo_buffer: Vec::new(),
            save_status: SaveStatus::Idle,
            load_error: None,
        };
        if let Ok(block) = &block {
            let artifact = block.artifact.clone();
            cx.spawn(async move |this, cx| {
                let outcome = service.open(&artifact).await;
                this.update(cx, |widget, cx| {
                    match outcome {
                        Ok(document) => {
                            widget.document = Some(document.clone());
                            widget.load_sheets(document, cx);
                        }
                        Err(error) => widget.load_error = Some(error.to_string()),
                    }
                    cx.notify();
                })
                .log_err();
            })
            .detach();
        }
        widget
    }

    /// Fetch the sheet list, then the current window, for a fresh document.
    fn load_sheets(&mut self, document: WorkbookDocument, cx: &mut Context<Self>) {
        self.window = Some(window_covering(self.active_cell, &self.active_sheet));
        cx.spawn(async move |this, cx| {
            let sheets = document.sheets().await;
            this.update(cx, |widget, cx| {
                match sheets {
                    Ok(sheets) => widget.sheets = sheets,
                    Err(error) => widget.load_error = Some(error.to_string()),
                }
                widget.fetch_window(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Fetch the current window from the open document.
    fn fetch_window(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        self.loading = true;
        cx.spawn(async move |this, cx| {
            let outcome = document.viewport(window).await;
            this.update(cx, |widget, cx| {
                widget.loading = false;
                match outcome {
                    Ok(content) => {
                        widget.window = Some(content.window.clone());
                        widget.content = Some(content);
                        widget.load_error = None;
                    }
                    Err(error) => widget.load_error = Some(error.to_string()),
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Re-window when the active cell leaves the current window.
    fn ensure_window_covers_active(&mut self, cx: &mut Context<Self>) {
        let covers = self
            .window
            .as_ref()
            .is_some_and(|window| window_contains(window, self.active_cell));
        if !covers {
            self.window = Some(window_covering(self.active_cell, &self.active_sheet));
            self.fetch_window(cx);
        }
    }

    fn move_active(&mut self, nav: Nav, extend_selection: bool, cx: &mut Context<Self>) {
        self.active_cell = move_active(self.active_cell, nav);
        if extend_selection {
            self.selection_extent = Some(self.active_cell);
        } else {
            self.selection_anchor = Some(self.active_cell);
            self.selection_extent = None;
        }
        self.ensure_window_covers_active(cx);
        cx.notify();
    }

    fn selection(&self) -> Option<Rect> {
        match (self.selection_anchor, self.selection_extent) {
            (Some(anchor), Some(extent)) => Some(selection_rect(anchor, extent)),
            _ => None,
        }
    }

    // ── editing ────────────────────────────────────────────────────────

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            return;
        }
        let (formula, value) = self.cell_state(self.active_cell);
        self.editor = Some(EditorState {
            cell: self.active_cell,
            text: editor_text(&formula, &value),
        });
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let edit = commit_to_edit(&editor.text, editor.cell, &self.active_sheet);
        self.stage_batch(vec![edit], cx);
    }

    /// The (formula, value) pair for a cell within the loaded window.
    fn cell_state(&self, cell: (usize, usize)) -> (String, TableValue) {
        let Some(content) = &self.content else {
            return (String::new(), TableValue::Empty);
        };
        let window = &content.window;
        if !window_contains(window, cell) {
            return (String::new(), TableValue::Empty);
        }
        let (r, c) = (cell.0 - window.start_row, cell.1 - window.start_col);
        (content.formulas[r][c].clone(), content.cells[r][c].clone())
    }

    fn handle_editor_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        match keystroke.key.as_str() {
            "backspace" => {
                editor.text.pop();
            }
            _ => {
                if let Some(typed) = keystroke.key_char.as_deref()
                    && typed.len() == 1
                {
                    editor.text.push(typed.chars().next().unwrap_or_default());
                }
            }
        }
        cx.notify();
    }

    /// Stage a batch of edits locally (undoable as one step) and mirror it
    /// for the save payload. New staging invalidates redo (engine
    /// semantics), so the redo buffer clears here.
    fn stage_batch(&mut self, edits: Vec<CellEdit>, cx: &mut Context<Self>) {
        if edits.is_empty() {
            return;
        }
        let Some(document) = self.document.clone() else {
            return;
        };
        self.staged_batches.push(edits.clone());
        self.redo_buffer.clear();
        self.save_status = SaveStatus::Idle;
        cx.spawn(async move |this, cx| {
            let outcome = document.stage(edits).await;
            this.update(cx, |widget, cx| {
                if let Err(error) = outcome {
                    widget.load_error = Some(format!("stage failed: {error}"));
                    widget.staged_batches.pop();
                }
                widget.fetch_window(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let outcome = document.undo().await;
            this.update(cx, |widget, cx| {
                if matches!(outcome, Ok(true)) {
                    if let Some(batch) = widget.staged_batches.pop() {
                        widget.redo_buffer.push(batch);
                    }
                }
                widget.fetch_window(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let outcome = document.redo().await;
            this.update(cx, |widget, cx| {
                if matches!(outcome, Ok(true)) {
                    if let Some(batch) = widget.redo_buffer.pop() {
                        widget.staged_batches.push(batch);
                    }
                }
                widget.fetch_window(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    // ── clipboard ───────────────────────────────────────────────────────

    fn copy_selection(&mut self, cx: &mut App) {
        let Some(rect) = self.selection() else { return };
        let Some(content) = &self.content else { return };
        let tsv = selection_to_tsv(content, rect);
        cx.write_to_clipboard(ClipboardItem::new_string(tsv));
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let text = item.text().unwrap_or_default();
        if text.is_empty() {
            return;
        }
        let edits = tsv_to_edits(&text, self.active_cell, &self.active_sheet);
        self.stage_batch(edits, cx);
    }

    // ── save (governed dispatch) ────────────────────────────────────────

    /// Persist the staged edits through `spreadsheet_apply` via the
    /// governed `ToolInvoker` (§8: persisted changes always dispatch
    /// through ToolInvoker; the four failure states are visible).
    fn dispatch_save(&mut self, cx: &mut Context<Self>) {
        let Ok(block) = self.block.clone() else {
            return;
        };
        if self.staged_batches.is_empty() {
            return;
        }
        let edits: Vec<CellEdit> = self.staged_batches.iter().flatten().cloned().collect();
        let transaction = match EditTransaction::new(
            block.artifact.clone(),
            uuid::Uuid::new_v4().simple().to_string(),
            SpreadsheetAccess::WorkbookWhatIf,
            edits,
        ) {
            Ok(transaction) => transaction,
            Err(error) => {
                self.save_status = SaveStatus::Failed(error.to_string());
                cx.notify();
                return;
            }
        };
        let args = serde_json::json!({ "transaction": transaction });
        match shared_tool_invoker() {
            None => {
                self.save_status = SaveStatus::NotWired;
                cx.notify();
            }
            Some(invoker) => {
                self.save_status = SaveStatus::Saving;
                let task = invoker.invoke_tool(SPREADSHEET_SERVER, SPREADSHEET_APPLY, args);
                cx.spawn(async move |this, cx| {
                    let outcome = task.await;
                    this.update(cx, |widget, cx| match outcome {
                        Ok(text) => widget.apply_save_response(&text, cx),
                        Err(error) => {
                            widget.save_status = SaveStatus::from_invoke_error(&error);
                            cx.notify();
                        }
                    })
                    .log_err();
                })
                .detach();
            }
        }
    }

    /// Parse a successful `spreadsheet_apply` response: the new block
    /// (from the display hint) becomes the widget's state; staged edits
    /// clear and the new revision opens.
    fn apply_save_response(&mut self, text: &str, cx: &mut Context<Self>) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.save_status = SaveStatus::Failed(format!("unreadable response: {error}"));
                cx.notify();
                return;
            }
        };
        let Some(content) = parsed.get("content") else {
            self.save_status = SaveStatus::Failed("response missing its envelope".into());
            cx.notify();
            return;
        };
        if content.get("status").and_then(|s| s.as_str()) != Some("applied") {
            let reason = content
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            self.save_status = SaveStatus::Failed(format!("apply returned {reason}"));
            cx.notify();
            return;
        }
        let Some(hint) = content.get("display_hint").and_then(|h| h.as_str()) else {
            self.save_status = SaveStatus::Failed("apply response missing display hint".into());
            cx.notify();
            return;
        };
        let Some(body) = hint
            .strip_prefix("```spreadsheet\n")
            .and_then(|rest| rest.strip_suffix("\n```"))
        else {
            self.save_status = SaveStatus::Failed("display hint is not a spreadsheet block".into());
            cx.notify();
            return;
        };
        let new_block: Result<SpreadsheetBlock, _> = serde_json::from_str(body);
        let new_block = match new_block {
            Ok(block) => block,
            Err(error) => {
                self.save_status =
                    SaveStatus::Failed(format!("new block failed to parse: {error}"));
                cx.notify();
                return;
            }
        };
        if let Err(error) = new_block.validate() {
            self.save_status = SaveStatus::Failed(format!("new block is invalid: {error}"));
            cx.notify();
            return;
        }
        self.block = Ok(new_block.clone());
        self.staged_batches.clear();
        self.redo_buffer.clear();
        self.save_status = SaveStatus::Saved;
        self.active_cell = (0, 0);
        self.selection_anchor = None;
        self.selection_extent = None;
        self.editor = None;
        self.active_sheet = new_block.active_sheet.clone();
        self.window = Some(new_block.viewport.clone());
        self.content = None;
        self.reload_document(cx);
    }

    /// Open the (post-save) current artifact as the widget's document.
    fn reload_document(&mut self, cx: &mut Context<Self>) {
        let Ok(block) = self.block.clone() else {
            return;
        };
        let Ok(service) = shared_spreadsheet_service().map(Arc::clone) else {
            return;
        };
        let artifact = block.artifact.clone();
        cx.spawn(async move |this, cx| {
            let outcome = service.open(&artifact).await;
            this.update(cx, |widget, cx| {
                match outcome {
                    Ok(document) => {
                        widget.document = Some(document.clone());
                        widget.load_error = None;
                        widget.fetch_window(cx);
                    }
                    Err(error) => widget.load_error = Some(error.to_string()),
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    // ── keyboard ────────────────────────────────────────────────────────

    fn handle_grid_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let platform = keystroke.modifiers.platform || keystroke.modifiers.control;
        if self.editor.is_some() {
            match keystroke.key.as_str() {
                "escape" => self.cancel_edit(cx),
                "enter" => {
                    self.commit_edit(cx);
                    self.move_active(Nav::Down, false, cx);
                }
                _ => self.handle_editor_keystroke(keystroke, cx),
            }
            return;
        }
        let extend = keystroke.modifiers.shift;
        match keystroke.key.as_str() {
            "up" => self.move_active(Nav::Up, extend, cx),
            "down" => self.move_active(Nav::Down, extend, cx),
            "left" => self.move_active(Nav::Left, extend, cx),
            "right" => self.move_active(Nav::Right, extend, cx),
            "tab" => self.move_active(if extend { Nav::Left } else { Nav::Right }, false, cx),
            "home" => self.move_active(Nav::Home, false, cx),
            "end" => self.move_active(Nav::End, false, cx),
            "pageup" => self.move_active(Nav::PageUp, false, cx),
            "pagedown" => self.move_active(Nav::PageDown, false, cx),
            "enter" | "f2" => self.start_edit(window, cx),
            "escape" => {
                self.selection_anchor = Some(self.active_cell);
                self.selection_extent = None;
                cx.notify();
            }
            "z" if platform => {
                if extend {
                    self.redo(cx);
                } else {
                    self.undo(cx);
                }
            }
            "y" if platform => self.redo(cx),
            "c" if platform => self.copy_selection(cx),
            "v" if platform => self.paste(cx),
            _ => {}
        }
    }

    // ── rendering ───────────────────────────────────────────────────────

    fn render_header(&self) -> AnyElement {
        let title = match &self.block {
            Ok(block) => block.title.clone(),
            Err(_) => "Spreadsheet".to_string(),
        };
        h_flex()
            .gap_1()
            .flex_shrink_0()
            .child(
                Label::new(title)
                    .size(LabelSize::Small)
                    .color(Color::Default),
            )
            .children(self.sheets.iter().enumerate().map(|(index, sheet)| {
                let active = sheet == &self.active_sheet;
                div()
                    .id(("sheet-tab", index as u64))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .flex_shrink_0()
                    .when(active, |tab| tab.bg(gpui::transparent_black().opacity(0.1)))
                    .child(
                        Label::new(sheet.clone())
                            .size(LabelSize::XSmall)
                            .color(if active { Color::Accent } else { Color::Muted }),
                    )
            }))
            .into_any_element()
    }

    fn render_formula_bar(&self) -> AnyElement {
        let (formula, value) = self.cell_state(self.active_cell);
        let text = match &self.editor {
            Some(editor) => editor.text.clone(),
            None => editor_text(&formula, &value),
        };
        let cell_ref = format!(
            "{}{}",
            col_letter(self.active_cell.1),
            self.active_cell.0 + 1
        );
        h_flex()
            .gap_1()
            .flex_shrink_0()
            .child(
                div()
                    .id("active-cell-ref")
                    .min_w_10()
                    .flex_shrink_0()
                    .child(
                        Label::new(cell_ref)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .id("formula-bar")
                    .min_w_0()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .border_1()
                    .rounded_sm()
                    .overflow_hidden()
                    .child(Label::new(text).size(LabelSize::XSmall).truncate()),
            )
            .when(self.editor.is_some(), |bar| {
                bar.child(
                    Label::new("editing — Enter commits, Esc cancels")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .flex_shrink_0(),
                )
            })
            .into_any_element()
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(content) = &self.content else {
            return div()
                .id("spreadsheet-grid-empty")
                .py_4()
                .child(self.loading_notice())
                .into_any_element();
        };
        let window = &content.window;
        let selection = self.selection();
        let handle = cx.entity().downgrade();
        let focus_target = self.focus_handle.clone();
        let mut rows = v_flex().min_w_0().flex_1();

        // Column header row.
        let mut header = h_flex().flex_shrink_0();
        header = header.child(
            div()
                .id("corner")
                .w_8()
                .flex_shrink_0()
                .py_1()
                .border_1()
                .child(Label::new("").size(LabelSize::XSmall)),
        );
        for col in window.start_col..window.start_col + window.col_count {
            header = header.child(
                div()
                    .id(("col-header", col))
                    .w_24()
                    .flex_shrink_0()
                    .py_1()
                    .border_1()
                    .child(
                        Label::new(col_letter(col))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            );
        }
        rows = rows.child(header);

        // Data rows: only the window — the paint bound.
        for (r, row_values) in content.cells.iter().enumerate() {
            let row = window.start_row + r;
            let mut row_el = h_flex().flex_shrink_0();
            row_el = row_el.child(
                div()
                    .id(("row-header", row))
                    .w_8()
                    .flex_shrink_0()
                    .py_1()
                    .border_1()
                    .child(
                        Label::new(format!("{}", row + 1))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            );
            for (c, value) in row_values.iter().enumerate() {
                let col = window.start_col + c;
                let cell = (row, col);
                let is_active = cell == self.active_cell;
                let is_selected = selection.is_some_and(|rect| {
                    rect.0 <= row && row <= rect.2 && rect.1 <= col && col <= rect.3
                });
                let is_editing = self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.cell == cell);
                let text = if is_editing {
                    self.editor
                        .as_ref()
                        .map(|editor| editor.text.clone())
                        .unwrap_or_default()
                } else {
                    value_to_text(value)
                };
                let click_handle = handle.clone();
                let click_focus = focus_target.clone();
                row_el = row_el.child(
                    div()
                        .id(("cell", (row as u64) * 16_384 + col as u64))
                        .w_24()
                        .flex_shrink_0()
                        .py_1()
                        .border_1()
                        .when(is_active, |cell| {
                            cell.bg(gpui::transparent_black().opacity(0.08))
                        })
                        .when(is_selected && !is_active, |cell| {
                            cell.bg(gpui::transparent_black().opacity(0.04))
                        })
                        .on_click(move |_event, window, cx| {
                            let Some(entity) = click_handle.upgrade() else {
                                return;
                            };
                            window.focus(&click_focus, cx);
                            entity.update(cx, |widget, cx| {
                                widget.active_cell = cell;
                                if widget.editor.is_some() {
                                    widget.editor = None;
                                }
                                cx.notify();
                            });
                        })
                        .child(
                            Label::new(text)
                                .size(LabelSize::XSmall)
                                .when(is_active, |label| label.color(Color::Accent))
                                .truncate(),
                        ),
                );
            }
            rows = rows.child(row_el);
        }
        rows.into_any_element()
    }

    fn loading_notice(&self) -> AnyElement {
        let message = if self.loading {
            "Loading workbook …"
        } else if let Some(error) = &self.load_error {
            return Label::new(format!("Workbook error: {error}"))
                .size(LabelSize::Small)
                .color(Color::Error)
                .into_any_element();
        } else if let Some(error) = &self.service_error {
            return Label::new(format!("Engine unavailable: {error}"))
                .size(LabelSize::Small)
                .color(Color::Error)
                .into_any_element();
        } else {
            "No cells loaded"
        };
        Label::new(message)
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let staged = self.staged_batches.iter().map(Vec::len).sum::<usize>();
        let status_label = self.save_status.label();
        h_flex()
            .gap_2()
            .flex_shrink_0()
            .child(self.action_label("Undo", cx, |widget, cx| widget.undo(cx)))
            .child(self.action_label("Redo", cx, |widget, cx| widget.redo(cx)))
            .child(self.action_label("Save", cx, |widget, cx| widget.dispatch_save(cx)))
            .when(staged > 0, |row| {
                row.child(
                    Label::new(format!("{staged} staged"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .flex_shrink_0(),
                )
            })
            .when(!status_label.is_empty(), |row| {
                let color = match &self.save_status {
                    SaveStatus::Saving | SaveStatus::Saved => Color::Accent,
                    SaveStatus::Idle => Color::Muted,
                    _ => Color::Error,
                };
                row.child(
                    div()
                        .id("spreadsheet-status-label")
                        .min_w_0()
                        .flex_1()
                        .child(
                            Label::new(status_label)
                                .size(LabelSize::XSmall)
                                .color(color)
                                .truncate(),
                        ),
                )
            })
            .into_any_element()
    }

    fn action_label(
        &self,
        label: &'static str,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let handle = cx.entity().downgrade();
        div()
            .id(label)
            .px_2()
            .py_1()
            .rounded_sm()
            .flex_shrink_0()
            .on_click(move |_event, _window, cx| {
                if let Some(handle) = handle.upgrade() {
                    handle.update(cx, |widget, cx| handler(widget, cx));
                }
            })
            .child(
                Label::new(label)
                    .size(LabelSize::XSmall)
                    .color(Color::Accent),
            )
            .into_any_element()
    }
}

impl Render for SpreadsheetWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error_banner = self.block.as_ref().err().map(|error| {
            Label::new(format!("Spreadsheet block error: {error}"))
                .size(LabelSize::Small)
                .color(Color::Error)
        });
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .gap_1()
            .p_2()
            .child(self.render_header())
            .children(error_banner)
            .child(self.render_formula_bar())
            .child(
                div()
                    .id("spreadsheet-grid-scroll")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        this.handle_grid_key(&event.keystroke, window, cx);
                    }))
                    .child(self.render_grid(cx)),
            )
            .child(self.render_status(cx))
    }
}

impl Focusable for SpreadsheetWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Column letter(s) for a 0-based column index: 0 → A, 25 → Z, 26 → AA.
pub fn col_letter(col: usize) -> String {
    let mut letters = String::new();
    let mut value = col;
    loop {
        letters.insert(0, (b'A' + (value % 26) as u8) as char);
        value = value / 26;
        if value == 0 {
            break;
        }
        value -= 1;
    }
    letters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::{WIDGET_WINDOW_COLS, WIDGET_WINDOW_ROWS};

    #[test]
    fn col_letters_follow_the_spreadsheet_convention() {
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(25), "Z");
        assert_eq!(col_letter(26), "AA");
        assert_eq!(col_letter(27), "AB");
        assert_eq!(col_letter(701), "ZZ");
        assert_eq!(col_letter(702), "AAA");
    }

    #[test]
    fn interrupted_maps_to_the_reconciliation_status() {
        let status =
            SaveStatus::from_invoke_error(&InvokeError::Interrupted("spreadsheet_apply".into()));
        assert_eq!(status, SaveStatus::Interrupted);
        assert!(status.label().contains("UNKNOWN"));
    }

    #[test]
    fn not_wired_and_unavailable_remain_distinct() {
        assert_eq!(
            SaveStatus::from_invoke_error(&InvokeError::NotWired),
            SaveStatus::NotWired
        );
        assert_eq!(
            SaveStatus::from_invoke_error(&InvokeError::Unavailable("x".into())),
            SaveStatus::Unavailable
        );
    }

    #[test]
    fn stale_digest_maps_to_the_conflict_status() {
        let status = SaveStatus::from_invoke_error(&InvokeError::Failed(
            "… failed_precondition … digest mismatch …".into(),
        ));
        match status {
            SaveStatus::Failed(message) => assert!(message.contains("conflict")),
            other => panic!("expected a conflict failure, got {other:?}"),
        }
    }

    /// The paint-bound invariant the plan's §8 demands: whatever the
    /// navigation target, the grid paints at most WIDGET_WINDOW_ROWS ×
    /// WIDGET_WINDOW_COLS cells.
    #[test]
    fn painted_cells_are_bounded_for_any_navigation() {
        for (row, col) in [
            (0, 0),
            (WIDGET_WINDOW_ROWS - 1, WIDGET_WINDOW_COLS - 1),
            (WIDGET_WINDOW_ROWS, WIDGET_WINDOW_COLS),
            (500_000, 15_000),
        ] {
            let window = window_covering((row, col), "Main");
            assert!(window.row_count <= WIDGET_WINDOW_ROWS);
            assert!(window.col_count <= WIDGET_WINDOW_COLS);
            assert!(window.row_count * window.col_count <= WIDGET_WINDOW_ROWS * WIDGET_WINDOW_COLS);
        }
    }
}
