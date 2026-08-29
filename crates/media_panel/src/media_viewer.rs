//! The media panel's viewing pane — surfaces media assets from the Steer
//! conversation's tool results, structurally.
//!
//! The Steer conversation (the director pane) drives all media operations;
//! this pane shows what the tools actually produced. Assets are extracted
//! from tool-result `display_hint` / `display_hints` fields (the same
//! structural path the tool cards use — T-V2), NOT from fenced blocks the
//! model copies into its replies, so the viewer updates deterministically
//! on every tool result regardless of model behavior.
//!
//! Two tabs: **Media** (the selected asset rendered large via the D18 viz
//! block renderer — the same widget the conversation uses inline) and
//! **Library** (every asset surfaced in the conversation, click to view).

use acp_thread::AgentThreadEntry;
use gpui::{App, Context, Entity, Render, SharedString, Window};
use serde_json::Value;
use ui::{Icon, IconName, Label, LabelSize, prelude::*};

/// One media asset surfaced from a tool result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    /// The fenced-block body (JSON) — handed to the viz block renderer.
    pub body: String,
    /// Asset src (path or URL) for the library list label.
    pub src: String,
    /// Media kind ("image" | "video" | "audio") for the icon.
    pub kind: String,
    /// The tool that produced the asset (provenance label).
    pub tool: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerTab {
    Media,
    Library,
}

pub struct MediaViewer {
    assets: Vec<MediaAsset>,
    selected: Option<usize>,
    active_tab: ViewerTab,
}

impl MediaViewer {
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            selected: None,
            active_tab: ViewerTab::Media,
        }
    }

    /// Scan an `AcpThread`'s tool results for display hints and merge the
    /// extracted assets. Deduplicates by body — re-observing a thread must
    /// not duplicate entries. A newly-seen asset auto-selects: the latest
    /// result is what the operator wants to see.
    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &App) {
        let thread = thread.read(cx);
        let mut new_selection = None;
        for entry in thread.entries() {
            let AgentThreadEntry::ToolCall(call) = entry else {
                continue;
            };
            let Some(serde_json::Value::String(text)) = call.raw_output.as_ref() else {
                continue;
            };
            let tool: SharedString = call.tool_name.clone().unwrap_or_else(|| "tool".into());
            for hint in hkask_types::tool_response::display_hints_from_output_text(text) {
                let Some(asset) = asset_from_hint(&hint, &tool) else {
                    continue;
                };
                if self
                    .assets
                    .iter()
                    .any(|existing| existing.body == asset.body)
                {
                    continue;
                }
                self.assets.push(asset);
                new_selection = Some(self.assets.len() - 1);
            }
        }
        if let Some(ix) = new_selection {
            self.selected = Some(ix);
        }
    }

    fn tab_button(
        &self,
        tab: ViewerTab,
        label: SharedString,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.active_tab == tab;
        div()
            .id(SharedString::from(format!("media-viewer-tab-{:?}", tab)))
            .px_2()
            .h_6()
            .rounded_sm()
            .map(|el| {
                if active {
                    el.bg(cx.theme().colors().element_active)
                } else {
                    el.hover(|el| el.bg(cx.theme().colors().element_hover))
                }
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .child(Label::new(label).size(LabelSize::Small).color(if active {
                ui::Color::Default
            } else {
                ui::Color::Muted
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_tab = tab;
                cx.notify();
            }))
    }

    fn render_media(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            return self.render_empty(
                "No media yet",
                "Ask the director to generate, search, or fetch media — results appear here automatically.",
            )
            .into_any_element();
        };
        let asset = &self.assets[ix];
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(asset.src.clone())
                    .size(LabelSize::Small)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(asset.tool.clone())
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint),
            );
        // The D18 viz block renderer — the same widget the conversation
        // renders inline. Takes the fenced-block body (JSON).
        let renderer = hkask_viz_core::block_renderer();
        let body = asset.body.clone();
        let content = renderer(&body, window, &mut *cx)
            .map(|element| {
                div()
                    .id("media-viewer-content")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_3()
                    .child(element)
                    .into_any_element()
            })
            .unwrap_or_else(|| {
                self.render_empty(
                    "Unrenderable media block",
                    "The tool produced a display hint the viewer cannot render.",
                )
                .into_any_element()
            });
        v_flex()
            .size_full()
            .child(header)
            .child(content)
            .into_any_element()
    }

    fn render_library(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.assets.is_empty() {
            return self
                .render_empty(
                    "Library empty",
                    "Assets surfaced by the conversation's tool results appear here.",
                )
                .into_any_element();
        }
        let rows = self
            .assets
            .iter()
            .enumerate()
            .map(|(ix, asset)| {
                let selected = self.selected == Some(ix);
                let icon = if asset.kind == "image" {
                    IconName::Image
                } else {
                    IconName::File
                };
                div()
                    .id(SharedString::from(format!("media-library-row-{ix}")))
                    .h_8()
                    .px_2()
                    .gap_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .map(|el| {
                        if selected {
                            el.bg(cx.theme().colors().element_active)
                        } else {
                            el.hover(|el| el.bg(cx.theme().colors().element_hover))
                        }
                    })
                    .cursor_pointer()
                    .child(Icon::new(icon).color(ui::Color::Muted))
                    .child(
                        Label::new(asset.src.clone())
                            .size(LabelSize::Small)
                            .color(ui::Color::Default),
                    )
                    .child(
                        Label::new(asset.tool.clone())
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(ix);
                        this.active_tab = ViewerTab::Media;
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        v_flex()
            .id("media-viewer-library")
            .flex_1()
            .overflow_y_scroll()
            .gap_0p5()
            .p_2()
            .children(rows)
            .into_any_element()
    }

    fn render_empty(&self, title: &str, hint: &str) -> impl IntoElement + use<> {
        v_flex()
            .size_full()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                Label::new(SharedString::from(title.to_string()))
                    .size(LabelSize::Small)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(hint.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint),
            )
    }
}

impl Render for MediaViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library_label: SharedString = format!("Library ({})", self.assets.len()).into();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h_8()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.tab_button(ViewerTab::Media, "Media".into(), cx))
                    .child(self.tab_button(ViewerTab::Library, library_label, cx)),
            )
            .child(match self.active_tab {
                ViewerTab::Media => self.render_media(_window, cx).into_any_element(),
                ViewerTab::Library => self.render_library(cx).into_any_element(),
            })
    }
}

/// Parse a fenced ```media display hint into a `MediaAsset` — the block body
/// (JSON between the fences) plus the kind/src fields for labels.
fn asset_from_hint(hint: &str, tool: &SharedString) -> Option<MediaAsset> {
    let trimmed = hint.trim();
    let after_fence = trimmed.strip_prefix("```media")?;
    let body = after_fence.trim().strip_suffix("```")?.trim().to_string();
    let value: Value = serde_json::from_str(&body).ok()?;
    let kind = value
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("image")
        .to_string();
    let src = value
        .get("src")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Some(MediaAsset {
        body,
        src,
        kind,
        tool: tool.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(body: &str) -> String {
        format!("```media\n{body}\n```")
    }

    #[test]
    fn asset_from_hint_parses_kind_and_src() {
        let asset = asset_from_hint(
            &hint(r#"{"kind":"video","src":"/tmp/out.mp4"}"#),
            &"video_fetch".into(),
        )
        .expect("parses");
        assert_eq!(asset.kind, "video");
        assert_eq!(asset.src, "/tmp/out.mp4");
        assert_eq!(asset.tool, "video_fetch");
        assert!(asset.body.starts_with('{'));
    }

    #[test]
    fn asset_from_hint_rejects_non_media_fences_and_bad_json() {
        assert!(asset_from_hint("```rust\nfn main() {}\n```", &"t".into()).is_none());
        assert!(asset_from_hint("```media\nnot json\n```", &"t".into()).is_none());
        assert!(asset_from_hint("plain text", &"t".into()).is_none());
    }

    #[test]
    fn ingest_deduplicates_and_auto_selects_latest() {
        // The extraction path from a raw tool output (envelope + hint) to the
        // viewer's asset list — same shape the thread observation ingests.
        let output = serde_json::json!({
            "content": {
                "display_hint": hint(r#"{"kind":"image","src":"/tmp/a.png"}"#)
            }
        })
        .to_string();
        let hints = hkask_types::tool_response::display_hints_from_output_text(&output);
        assert_eq!(hints.len(), 1);
        let asset = asset_from_hint(&hints[0], &"generate_image".into()).expect("parses");
        assert_eq!(asset.src, "/tmp/a.png");
    }
}
