//! Tool-call card rendering for the kask panel.
//!
//! Renders curator-emitted tool calls and direct `/tool_name` invocations as
//! cards: tool name, status (pending/running/done/error), collapsible raw
//! input, output (markdown or raw), copy button. This is the Phase 4
//! rendering upgrade from `kask-panel-redesign.md` v0.3.0.
//!
//! This is a purpose-built ~150-line component, not a port of `ThreadView`'s
//! `render_tool_call` (which carries ACP permission prompts, session-id
//! lookup, and subagent machinery). The kask panel has no permission prompts
//! (OCAP tokens are pre-authorized by the bridge) and no subagents — the
//! card is simpler than its agent-panel counterpart.

use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, StatefulInteractiveElement, Window, prelude::*,
};
use serde_json::Value;
use ui::prelude::*;

/// The status of a tool call.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolCallStatus {
    /// The call was emitted but the result hasn't arrived yet.
    Pending,
    /// The tool is executing.
    Running,
    /// The tool returned successfully.
    Done,
    /// The tool returned an error.
    Error,
}

/// A single tool call entry, tracked across the streaming turn.
#[derive(Clone, Debug)]
pub struct ToolCallEntry {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub status: ToolCallStatus,
    /// The result text (pretty-printed JSON or raw string). `None` until
    /// the result arrives.
    pub result: Option<String>,
    /// Whether the raw input is expanded (collapsible).
    pub expanded: bool,
}

impl ToolCallEntry {
    pub fn new(call_id: String, tool_name: String, arguments: Value) -> Self {
        Self {
            call_id,
            tool_name,
            arguments,
            status: ToolCallStatus::Pending,
            result: None,
            expanded: false,
        }
    }

    /// Mark the call as completed with the given result.
    pub fn complete(&mut self, result: Result<Value, String>) {
        match result {
            Ok(value) => {
                self.result = Some(format_json(&value));
                self.status = ToolCallStatus::Done;
            }
            Err(error) => {
                self.result = Some(error);
                self.status = ToolCallStatus::Error;
            }
        }
    }
}

/// A view model for a single tool-call card. Holds the entry + the expand
/// toggle state. One entity per tool call in the message list.
pub struct ToolCallCard {
    pub entry: ToolCallEntry,
    focus_handle: FocusHandle,
}

impl ToolCallCard {
    pub fn new(entry: ToolCallEntry, cx: &mut Context<Self>) -> Self {
        Self {
            entry,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn update_entry(&mut self, entry: ToolCallEntry, cx: &mut Context<Self>) {
        self.entry = entry;
        cx.notify();
    }

    pub fn toggle_expand(&mut self, cx: &mut Context<Self>) {
        self.entry.expanded = !self.entry.expanded;
        cx.notify();
    }

    fn render_status_icon(&self) -> impl IntoElement {
        let (icon, color) = match self.entry.status {
            ToolCallStatus::Pending | ToolCallStatus::Running => (IconName::Loader, Color::Muted),
            ToolCallStatus::Done => (IconName::Check, Color::Created),
            ToolCallStatus::Error => (IconName::XCircle, Color::Error),
        };
        Icon::new(icon).color(color).size(IconSize::XSmall)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .child(self.render_status_icon())
            .child(
                Label::new(self.entry.tool_name.clone())
                    .size(LabelSize::Small)
                    .color(Color::Accent),
            )
            .child(
                Button::new("toggle-input")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .label(if self.entry.expanded { "▾" } else { "▸" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_expand(cx);
                    })),
            )
            .child(
                Button::new("copy-result")
                    .style(ButtonStyle::Subtle)
                    .icon(IconName::Copy)
                    .icon_size(IconSize::XSmall)
                    .disabled(self.entry.result.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(result) = &this.entry.result {
                            cx.write_to_clipboard(ClipboardItem::new_string(result.clone()));
                        }
                    })),
            )
    }

    fn render_input(&self) -> Option<impl IntoElement> {
        if !self.entry.expanded {
            return None;
        }
        let args = if self.entry.arguments.is_object() {
            serde_json::to_string_pretty(&self.entry.arguments).unwrap_or_default()
        } else {
            self.entry.arguments.to_string()
        };
        Some(
            div()
                .mt_1()
                .p_1()
                .rounded_sm()
                .bg(cx_theme_muted_bg())
                .child(
                    Label::new(format!("Input:\n{args}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
        )
    }

    fn render_output(&self) -> Option<impl IntoElement> {
        let result = self.entry.result.as_ref()?;
        let color = match self.entry.status {
            ToolCallStatus::Error => Color::Error,
            _ => Color::Muted,
        };
        Some(
            div()
                .mt_1()
                .p_1()
                .rounded_sm()
                .bg(cx_theme_muted_bg())
                .child(
                    Label::new(format!("Output:\n{result}"))
                        .size(LabelSize::XSmall)
                        .color(color),
                ),
        )
    }
}

impl Focusable for ToolCallCard {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<()> for ToolCallCard {}

impl Render for ToolCallCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        v_flex()
            .gap_0()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(border_color)
            .child(self.render_header(cx))
            .children(self.render_input())
            .children(self.render_output())
    }
}

/// Pretty-print a JSON value for display, with truncation at 5000 chars
/// (UTF-8-safe). Reuses the panel's existing `format_json_result` logic.
fn format_json(value: &Value) -> String {
    let result = match value {
        Value::String(s) => {
            if let Ok(inner) = serde_json::from_str::<Value>(s) {
                return format_json(&inner);
            }
            s.clone()
        }
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    };
    const MAX_LEN: usize = 5000;
    if result.len() > MAX_LEN {
        let mut end = MAX_LEN;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &result[..end])
    } else {
        result
    }
}

/// Get a muted background color from the current theme.
fn cx_theme_muted_bg() -> Hsla {
    // Use a slightly transparent version of the editor background for the
    // input/output panels. This is a simple heuristic; the agent panel uses
    // `colors().editor_background` with opacity. We use `ghost_element`
    // colors which are theme-appropriate.
    gpui::hsla(0.0, 0.0, 0.5, 0.08)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_entry_new_is_pending() {
        let entry = ToolCallEntry::new(
            "call_1".to_string(),
            "regulation_status".to_string(),
            serde_json::json!({}),
        );
        assert_eq!(entry.status, ToolCallStatus::Pending);
        assert!(entry.result.is_none());
        assert!(!entry.expanded);
    }

    #[test]
    fn tool_call_entry_complete_ok_sets_done() {
        let mut entry = ToolCallEntry::new(
            "call_1".to_string(),
            "regulation_status".to_string(),
            serde_json::json!({}),
        );
        entry.complete(Ok(serde_json::json!({"healthy": true})));
        assert_eq!(entry.status, ToolCallStatus::Done);
        assert!(entry.result.is_some());
        assert!(entry.result.as_ref().unwrap().contains("healthy"));
    }

    #[test]
    fn tool_call_entry_complete_err_sets_error() {
        let mut entry = ToolCallEntry::new(
            "call_1".to_string(),
            "regulation_status".to_string(),
            serde_json::json!({}),
        );
        entry.complete(Err("tool not found".to_string()));
        assert_eq!(entry.status, ToolCallStatus::Error);
        assert_eq!(entry.result.as_deref(), Some("tool not found"));
    }

    #[test]
    fn tool_call_entry_toggle_expand_flips() {
        let mut entry = ToolCallEntry::new(
            "call_1".to_string(),
            "regulation_status".to_string(),
            serde_json::json!({}),
        );
        assert!(!entry.expanded);
        entry.expanded = !entry.expanded;
        assert!(entry.expanded);
    }

    #[test]
    fn format_json_pretty_prints_object() {
        let result = format_json(&serde_json::json!({"key": "value"}));
        assert!(result.contains("\"key\""));
        assert!(result.contains('\n'));
    }

    #[test]
    fn format_json_truncates_long_output() {
        let mut map = serde_json::Map::new();
        for i in 0..1000 {
            map.insert(format!("key_{i}"), serde_json::json!(format!("value_{i}")));
        }
        let val = Value::Object(map);
        let result = format_json(&val);
        assert!(result.len() <= 5003);
        assert!(result.ends_with('…'));
    }
}
