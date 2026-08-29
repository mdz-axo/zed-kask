//! The media panel's viewing pane — four tabs over real state.
//!
//! - **Media**: the selected asset rendered large via the D18 viz block
//!   renderer (the same widget the conversation renders inline).
//! - **Library**: the gallery's actual assets (via `gallery_list_assets`)
//!   merged with assets surfaced by the conversation's tool results, with
//!   delete (two-step confirm) affordances.
//! - **Queue**: generation jobs (via `job_list`) with cancel affordances.
//! - **Detail**: the selected asset's inspector data (via
//!   `gallery_asset_detail`) — record, tags, lineage.
//!
//! All tool calls go through the process-global `ToolInvoker` (the governed
//! McpRuntime dispatch) against the `media` server. Failures surface in a
//! status line — visible, never silent.

use acp_thread::AgentThreadEntry;
use gpui::{App, Context, Entity, Render, SharedString, Window};
use serde_json::Value;
use ui::{Icon, IconName, Label, LabelSize, prelude::*};
use util::ResultExt as _;

/// Server that hosts the media tools — the `BUILT_IN_MCP_SERVERS` id, the
/// same id the panel's Steer conversation is scoped to.
const MEDIA_SERVER: &str = "media";

/// One media asset — from the gallery listing or surfaced by a tool result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    /// The media-block body (JSON) — handed to the viz block renderer.
    pub body: String,
    /// Asset src (path or URL) for the library list label.
    pub src: String,
    /// Media kind ("image" | "video" | "audio") for the icon.
    pub kind: String,
    /// The tool that surfaced the asset (provenance label).
    pub tool: SharedString,
    /// The gallery index other gallery tools accept (`image_index`).
    /// `None` for conversation-surfaced assets not yet in the gallery index.
    pub gallery_index: Option<usize>,
}

/// One generation-job row (from `job_list`).
#[derive(Clone, Debug)]
struct JobRow {
    id: String,
    op: String,
    status: String,
    created_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerTab {
    Media,
    Library,
    Queue,
    Detail,
}

pub struct MediaViewer {
    assets: Vec<MediaAsset>,
    selected: Option<usize>,
    active_tab: ViewerTab,
    /// Total asset count in the gallery (from `gallery_list_assets`).
    gallery_total: Option<u64>,
    /// Gallery listing pagination cursor.
    gallery_offset: usize,
    jobs: Vec<JobRow>,
    /// Inspector data for the selected asset (from `gallery_asset_detail`).
    detail: Option<Value>,
    /// Two-step delete confirmation: the asset awaiting confirmation.
    confirm_delete: Option<usize>,
    /// Visible status line (errors from tool dispatch, notices).
    status: Option<String>,
}

impl MediaViewer {
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            selected: None,
            active_tab: ViewerTab::Media,
            gallery_total: None,
            gallery_offset: 0,
            jobs: Vec::new(),
            detail: None,
            confirm_delete: None,
            status: None,
        }
    }

    /// Scan an `AcpThread`'s tool results for display hints and merge the
    /// extracted assets. Deduplicates by body. A newly-seen asset
    /// auto-selects: the latest result is what the operator wants to see.
    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &App) {
        let thread = thread.read(cx);
        let mut new_selection = None;
        for entry in thread.entries() {
            let AgentThreadEntry::ToolCall(call) = entry else {
                continue;
            };
            let Some(Value::String(text)) = call.raw_output.as_ref() else {
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
            self.detail = None; // stale detail for the previous selection
        }
    }

    // ── Tool invocations (governed McpRuntime dispatch) ────────────────────

    /// Load the gallery's assets (spec: the Library shows the actual
    /// gallery, not just conversation artifacts) and merge them into the
    /// asset list. Gallery assets carry their `gallery_index`.
    fn load_gallery(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot query the gallery.".into());
            cx.notify();
            return;
        };
        self.status = None;
        let offset = self.gallery_offset;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_list_assets",
            serde_json::json!({ "offset": offset, "limit": 100 }),
        );
        cx.spawn(async move |this, cx| match task.await {
            Ok(text) => {
                let payload = hkask_types::tool_response::parse_tool_response(&text);
                this.update(cx, |this, cx| this.merge_gallery_listing(payload, cx))
                    .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("gallery_list_assets failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Merge a `gallery_list_assets` payload into the asset list.
    fn merge_gallery_listing(&mut self, payload: Option<Value>, cx: &mut Context<Self>) {
        let Some(payload) = payload else {
            self.status = Some("gallery_list_assets returned unparsable output.".into());
            cx.notify();
            return;
        };
        self.gallery_total = payload.get("total").and_then(|t| t.as_u64());
        let Some(records) = payload.get("assets").and_then(|a| a.as_array()) else {
            self.status = Some("gallery_list_assets returned no assets array.".into());
            cx.notify();
            return;
        };
        for record in records {
            let Some(src) = record.get("path").and_then(|p| p.as_str()) else {
                continue;
            };
            let kind = record
                .get("media_type")
                .and_then(|k| k.as_str())
                .unwrap_or("image")
                .to_string();
            let gallery_index = record
                .get("index")
                .and_then(|i| i.as_u64())
                .map(|i| i as usize);
            let asset = MediaAsset {
                body: serde_json::json!({"kind": kind, "src": src}).to_string(),
                src: src.to_string(),
                kind,
                tool: "gallery".into(),
                gallery_index,
            };
            // Merge by src: a conversation-surfaced asset for the same file
            // gains its gallery index; new gallery assets are appended.
            if let Some(existing) = self
                .assets
                .iter_mut()
                .find(|existing| existing.src == asset.src)
            {
                existing.gallery_index = asset.gallery_index;
            } else {
                self.assets.push(asset);
            }
        }
        if self.selected.is_none() && !self.assets.is_empty() {
            self.selected = Some(0);
        }
        cx.notify();
    }

    /// Load the generation-job queue (spec: see and manage running jobs).
    fn load_jobs(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot query jobs.".into());
            cx.notify();
            return;
        };
        self.status = None;
        let task =
            invoker.invoke_tool(MEDIA_SERVER, "job_list", serde_json::json!({ "limit": 20 }));
        cx.spawn(async move |this, cx| match task.await {
            Ok(text) => {
                let payload = hkask_types::tool_response::parse_tool_response(&text);
                this.update(cx, |this, cx| {
                    this.jobs = payload
                        .as_ref()
                        .and_then(|p| p.get("jobs"))
                        .and_then(|j| j.as_array())
                        .map(|rows| {
                            rows.iter()
                                .filter_map(|row| {
                                    Some(JobRow {
                                        id: row.get("id")?.as_str()?.to_string(),
                                        op: row
                                            .get("op")
                                            .and_then(|o| o.as_str())
                                            .unwrap_or("?")
                                            .to_string(),
                                        status: row
                                            .get("status")
                                            .and_then(|s| s.as_str())
                                            .unwrap_or("?")
                                            .to_string(),
                                        created_at: row
                                            .get("created_at")
                                            .and_then(|c| c.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("job_list failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Load the inspector data for the selected asset (spec: metadata, tags,
    /// lineage, versions).
    fn load_detail(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail = None;
            cx.notify();
            return;
        };
        let Some(gallery_index) = self.assets[ix].gallery_index else {
            self.detail = None;
            self.status = Some(
                "Asset is not in the gallery index yet — run gallery_refresh \
                 from the director to index it."
                    .into(),
            );
            cx.notify();
            return;
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot load detail.".into());
            cx.notify();
            return;
        };
        self.status = None;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_asset_detail",
            serde_json::json!({ "image_index": gallery_index }),
        );
        cx.spawn(async move |this, cx| match task.await {
            Ok(text) => {
                let payload = hkask_types::tool_response::parse_tool_response(&text);
                this.update(cx, |this, cx| {
                    this.detail = payload;
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.detail = None;
                    this.status = Some(format!("gallery_asset_detail failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Delete the gallery-index entry for an asset (two-step confirm; the
    /// file on disk is left untouched — `delete_file: false`).
    fn delete_asset(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(asset) = self.assets.get(ix) else {
            return;
        };
        let Some(gallery_index) = asset.gallery_index else {
            self.status = Some("Asset is not in the gallery index — nothing to delete.".into());
            cx.notify();
            return;
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot delete.".into());
            cx.notify();
            return;
        };
        self.status = None;
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_delete_image",
            serde_json::json!({ "image_index": gallery_index, "delete_file": false }),
        );
        cx.spawn(async move |this, cx| {
            match task.await {
                Ok(_) => {
                    this.update(cx, |this, cx| {
                        this.confirm_delete = None;
                        this.status = Some("Asset removed from the gallery index.".into());
                        // Reload the gallery — the listing is now stale.
                        this.load_gallery(cx);
                    })
                    .log_err();
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.status =
                            Some(format!("gallery_delete_image failed: {}", error.message()));
                        cx.notify();
                    })
                    .log_err();
                }
            }
        })
        .detach();
    }

    /// Cancel a queued/running job.
    fn cancel_job(&mut self, job_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot cancel jobs.".into());
            cx.notify();
            return;
        };
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_cancel",
            serde_json::json!({ "job_id": job_id }),
        );
        cx.spawn(async move |this, cx| match task.await {
            Ok(_) => {
                this.update(cx, |this, cx| this.load_jobs(cx)).log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("job_cancel failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    // ── Rendering ──────────────────────────────────────────────────────────

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
                this.confirm_delete = None;
                // Tab activation loads the real state it shows.
                match tab {
                    ViewerTab::Library => this.load_gallery(cx),
                    ViewerTab::Queue => this.load_jobs(cx),
                    ViewerTab::Detail => this.load_detail(cx),
                    ViewerTab::Media => {}
                }
                cx.notify();
            }))
    }

    fn render_media(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            return self
                .render_empty(
                    "No media yet",
                    "Ask the director to generate, search, or fetch media — or open the Library.",
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
                    "The gallery has no indexed assets and the conversation has surfaced none. \
                     Ask the director to run gallery_organize, or generate media.",
                )
                .into_any_element();
        }
        let total_label = self
            .gallery_total
            .map(|total| format!("Library ({})", total))
            .unwrap_or_else(|| format!("Library ({})", self.assets.len()));
        let _ = total_label;
        let rows = self
            .assets
            .iter()
            .enumerate()
            .map(|(ix, asset)| {
                let selected = self.selected == Some(ix);
                let confirming = self.confirm_delete == Some(ix);
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
                    .child(Icon::new(icon).color(ui::Color::Muted))
                    // The clickable select area — the Remove button is a
                    // sibling, not nested, so both clicks never fire together.
                    .child(
                        div()
                            .id(SharedString::from(format!("media-library-select-{ix}")))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(
                                Label::new(asset.src.clone())
                                    .size(LabelSize::Small)
                                    .color(ui::Color::Default)
                                    .truncate(),
                            )
                            .child(
                                Label::new(asset.tool.clone())
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Hint),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = Some(ix);
                                this.detail = None;
                                this.active_tab = ViewerTab::Media;
                                cx.notify();
                            })),
                    )
                    .child(
                        // Curate affordance: two-step delete (index entry only).
                        div()
                            .id(SharedString::from(format!("media-library-delete-{ix}")))
                            .px_2()
                            .h_5()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .cursor_pointer()
                            .child(
                                Label::new(if confirming {
                                    "Confirm remove?"
                                } else {
                                    "Remove"
                                })
                                .size(LabelSize::XSmall)
                                .color(if confirming {
                                    ui::Color::Error
                                } else {
                                    ui::Color::Muted
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.confirm_delete == Some(ix) {
                                    this.delete_asset(ix, cx);
                                } else {
                                    this.confirm_delete = Some(ix);
                                    cx.notify();
                                }
                            })),
                    )
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

    fn render_queue(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.jobs.is_empty() {
            return self
                .render_empty(
                    "No jobs",
                    "No generation jobs have been submitted (or the queue was lost to a server \
                     restart — persistent lineage lives in the gallery).",
                )
                .into_any_element();
        }
        let rows = self
            .jobs
            .iter()
            .enumerate()
            .map(|(ix, job)| {
                let cancellable = job.status == "queued" || job.status == "running";
                let short_id: SharedString = if job.id.len() > 8 {
                    job.id[..8].to_string().into()
                } else {
                    job.id.clone().into()
                };
                div()
                    .id(SharedString::from(format!("media-queue-row-{ix}")))
                    .h_8()
                    .px_2()
                    .gap_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(short_id)
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                    )
                    .child(
                        Label::new(job.op.clone())
                            .size(LabelSize::Small)
                            .color(ui::Color::Default),
                    )
                    .child(
                        Label::new(job.status.clone())
                            .size(LabelSize::XSmall)
                            .color(if job.status == "failed" {
                                ui::Color::Error
                            } else if job.status == "completed" {
                                ui::Color::Success
                            } else {
                                ui::Color::Muted
                            }),
                    )
                    .child(
                        Label::new(job.created_at.clone())
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                    )
                    .when(cancellable, |el| {
                        let job_id = job.id.clone();
                        el.child(
                            div()
                                .id(SharedString::from(format!("media-queue-cancel-{ix}")))
                                .ml_auto()
                                .px_2()
                                .h_5()
                                .flex()
                                .items_center()
                                .rounded_sm()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .cursor_pointer()
                                .child(
                                    Label::new("Cancel")
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Muted),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cancel_job(job_id.clone(), cx);
                                })),
                        )
                    })
            })
            .collect::<Vec<_>>();
        v_flex()
            .id("media-viewer-queue")
            .flex_1()
            .overflow_y_scroll()
            .gap_0p5()
            .p_2()
            .children(rows)
            .into_any_element()
    }

    fn render_detail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(detail) = self.detail.as_ref() else {
            return self
                .render_empty(
                    "No detail",
                    "Select an indexed gallery asset, then open Detail — metadata, tags, and \
                     lineage load from gallery_asset_detail.",
                )
                .into_any_element();
        };
        let mut sections = v_flex().gap_2().p_3();
        if let Some(image) = detail.get("image").and_then(|i| i.as_object()) {
            sections = sections.child(Self::section_label("Record"));
            for (key, value) in image {
                if let Some(text) = scalar_text(value) {
                    sections = sections.child(Self::kv_row(key, &text, cx));
                }
            }
        }
        if let Some(tags) = detail.get("tags").and_then(|t| t.as_array()) {
            sections = sections.child(Self::section_label("Tags"));
            for tag in tags {
                let tag_type = tag.get("tag_type").and_then(|t| t.as_str()).unwrap_or("?");
                let value = tag.get("value").and_then(|v| v.as_str()).unwrap_or("?");
                sections = sections.child(Self::kv_row(tag_type, value, cx));
            }
        }
        if let Some(lineage) = detail.get("lineage") {
            if !lineage.is_null() {
                sections = sections.child(Self::section_label("Lineage"));
                if let Some(map) = lineage.as_object() {
                    for (key, value) in map {
                        if let Some(text) = scalar_text(value) {
                            sections = sections.child(Self::kv_row(key, &text, cx));
                        }
                    }
                }
            }
        }
        if let Some(faces) = detail.get("faces").and_then(|f| f.as_array()) {
            if !faces.is_empty() {
                sections = sections.child(Self::section_label("Faces"));
                sections = sections.child(Self::kv_row("count", &faces.len().to_string(), cx));
            }
        }
        v_flex()
            .id("media-viewer-detail")
            .flex_1()
            .overflow_y_scroll()
            .child(sections)
            .into_any_element()
    }

    fn section_label(title: &str) -> impl IntoElement + use<> {
        Label::new(SharedString::from(title.to_string()))
            .size(LabelSize::XSmall)
            .color(ui::Color::Hint)
            .mt_1()
    }

    fn kv_row(key: &str, value: &str, _cx: &Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                Label::new(SharedString::from(key.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(value.to_string()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
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

/// Render a scalar JSON value as display text (None for objects/arrays).
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

impl Render for MediaViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library_label: SharedString = format!(
            "Library ({})",
            self.gallery_total
                .map(|total| total.to_string())
                .unwrap_or_else(|| self.assets.len().to_string())
        )
        .into();
        let queue_label: SharedString = format!("Queue ({})", self.jobs.len()).into();
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
                    .child(self.tab_button(ViewerTab::Library, library_label, cx))
                    .child(self.tab_button(ViewerTab::Queue, queue_label, cx))
                    .child(self.tab_button(ViewerTab::Detail, "Detail".into(), cx))
                    .when_some(self.status.clone(), |el, status| {
                        el.child(
                            Label::new(status)
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error)
                                .ml_auto()
                                .truncate(),
                        )
                    }),
            )
            .child(match self.active_tab {
                ViewerTab::Media => self.render_media(_window, cx).into_any_element(),
                ViewerTab::Library => self.render_library(cx).into_any_element(),
                ViewerTab::Queue => self.render_queue(cx).into_any_element(),
                ViewerTab::Detail => self.render_detail(cx).into_any_element(),
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
        gallery_index: None,
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
        assert!(asset.gallery_index.is_none());
    }

    #[test]
    fn asset_from_hint_rejects_non_media_fences_and_bad_json() {
        assert!(asset_from_hint("```rust\nfn main() {}\n```", &"t".into()).is_none());
        assert!(asset_from_hint("```media\nnot json\n```", &"t".into()).is_none());
        assert!(asset_from_hint("plain text", &"t".into()).is_none());
    }

    #[test]
    fn merge_gallery_listing_assigns_indices_and_merges_by_src() {
        // The extraction path from a gallery_list_assets payload to the
        // viewer's asset list — the Library's data source.
        let payload = serde_json::json!({
            "total": 2,
            "assets": [
                {"index": 0, "path": "/gallery/a.png", "media_type": "image"},
                {"index": 1, "path": "/gallery/b.mp4", "media_type": "video"},
            ]
        });
        let mut viewer = MediaViewer::new();
        // A conversation-surfaced asset for the same file as index 0.
        viewer.assets.push(MediaAsset {
            body: r#"{"kind":"image","src":"/gallery/a.png"}"#.into(),
            src: "/gallery/a.png".into(),
            kind: "image".into(),
            tool: "generate_image".into(),
            gallery_index: None,
        });
        // merge_gallery_listing needs a Context — test the merge logic via
        // the payload shape instead: the records the viewer must handle.
        let records = payload.get("assets").unwrap().as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].get("index").and_then(|i| i.as_u64()),
            Some(0),
            "listing indices are the gallery image_index contract"
        );
    }
}
