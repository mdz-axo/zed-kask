//! The media panel's viewing pane — four tabs over real state.
//!
//! - **Media**: the selected asset rendered large via the D18 viz block
//!   renderer (the same widget the conversation renders inline).
//! - **Library**: the gallery's actual assets (via `gallery_list_assets`)
//!   merged with assets surfaced by the conversation's tool results, with
//!   delete (two-step confirm) affordances. Each fetched page reconciles
//!   the list — assets deleted from the gallery drop out of it.
//! - **Queue**: generation jobs (via `job_list`) with cancel affordances.
//! - **Detail**: the selected asset's inspector data (via
//!   `gallery_asset_detail`) — record, tags, lineage.
//!
//! All tool calls go through the process-global `ToolInvoker` (the governed
//! McpRuntime dispatch) against the `media` server. Failures surface in a
//! status line — visible, never silent.

use std::collections::HashSet;

use acp_thread::AgentThreadEntry;
use agent_client_protocol::schema::v1::ToolCallId;
use gpui::{Context, Entity, Render, SharedString, Window};
use hkask_mcp_media::types::JobRecord;
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
    /// The observed conversation thread (weak — the panel owns it), kept so
    /// `refresh` can re-ingest tool-result display hints without the panel
    /// forwarding the entity again.
    thread: Option<gpui::WeakEntity<acp_thread::AcpThread>>,
    /// The media widget for the currently-selected asset on the Media tab.
    /// Owned directly (not via the viz cache) so the edit toolbar can reach
    /// its playback clock and trim marks. Recreated when the selection's
    /// body changes.
    media_widget: Option<Entity<hkask_media_widget::MediaWidget>>,
    media_widget_body: Option<String>,
    /// Asset srcs queued for concatenation (`video_concat`). Two or more
    /// queue entries enable the Concat button.
    concat_queue: Vec<String>,
    /// Total asset count in the gallery (from `gallery_list_assets`).
    gallery_total: Option<u64>,
    /// Gallery listing pagination cursor.
    gallery_offset: usize,
    jobs: Vec<JobRecord>,
    /// Inspector data for the selected asset (from `gallery_asset_detail`).
    detail: Option<Value>,
    /// Two-step delete confirmation: the asset awaiting confirmation.
    confirm_delete: Option<usize>,
    /// Visible status line (errors from tool dispatch, notices).
    status: Option<String>,
    /// Tool-call ids whose results `ingest_thread` has already examined. A
    /// call's result lands once (ACP `update_fields` sets `raw_output` when
    /// the result arrives), so a first-seen id marks a newly-completed call
    /// — the signal that a gallery-mutating tool finished and the Library
    /// listing is stale. Cleared when the observed thread changes.
    processed_tool_results: HashSet<ToolCallId>,
}

impl MediaViewer {
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            selected: None,
            active_tab: ViewerTab::Media,
            thread: None,
            media_widget: None,
            media_widget_body: None,
            concat_queue: Vec::new(),
            gallery_total: None,
            gallery_offset: 0,
            jobs: Vec::new(),
            detail: None,
            confirm_delete: None,
            status: None,
            processed_tool_results: HashSet::new(),
        }
    }

    /// Scan an `AcpThread`'s tool results for display hints and merge the
    /// extracted assets. Deduplicates by body. A newly-seen asset
    /// auto-selects: the latest result is what the operator wants to see.
    /// A newly-completed gallery-mutating tool call reloads the Library
    /// listing so chat-driven mutations (delete, organize, imports) are
    /// reflected without a manual refresh.
    pub fn ingest_thread(&mut self, thread: Entity<acp_thread::AcpThread>, cx: &mut Context<Self>) {
        // A different thread (resume/open) resets completion tracking —
        // the new thread's historical calls are all first-seen again.
        if self.thread.as_ref().and_then(|weak| weak.upgrade()) != Some(thread.clone()) {
            self.processed_tool_results.clear();
        }
        self.thread = Some(thread.downgrade());
        let mut gallery_mutated = false;
        {
            let thread = thread.read(cx);
            for entry in thread.entries() {
                let AgentThreadEntry::ToolCall(call) = entry else {
                    continue;
                };
                let Some(output) = call.raw_output.as_ref() else {
                    continue; // still running — no result to ingest yet
                };
                let tool: SharedString = call.tool_name.clone().unwrap_or_else(|| "tool".into());
                // First sight of this call's result: a gallery-mutating
                // tool just completed in the conversation.
                if self.processed_tool_results.insert(call.id.clone())
                    && tool_mutates_gallery(&tool)
                {
                    gallery_mutated = true;
                }
                self.ingest_tool_result(output, &tool);
            }
        }

        // Only while the Library is on screen: switching to the tab already
        // reloads it (`tab_button`), so reloading from other tabs would be
        // wasted dispatches.
        if gallery_mutated && self.active_tab == ViewerTab::Library {
            self.load_gallery(cx);
        }
    }

    // ── Tool invocations (governed McpRuntime dispatch) ────────────────────

    /// Merge a tool result's display hints into the asset list and select
    /// the newest — the same ingestion `ingest_thread` applies to thread
    /// tool results, reused for viewer-initiated edits so a trimmed clip or
    /// concatenation surfaces immediately.
    /// expect: Show each completed artifact regardless of the tool transport.
    /// [P7] Motivating: structured and text transports share the viewer ingress.
    /// pre: output is an ACP raw output or an invoker response.
    /// post: new valid assets are selected; repeated bodies do not duplicate.
    pub fn ingest_tool_result(&mut self, output: &Value, tool: &str) {
        let hints = match output {
            Value::String(text) => hkask_types::tool_response::display_hints_from_output_text(text),
            value => hkask_types::tool_response::display_hints_from_output_value(value),
        };
        let tool: SharedString = tool.into();
        let mut new_selection = None;
        for hint in hints {
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
        if let Some(ix) = new_selection {
            self.selected = Some(ix);
            self.detail = None;
        }
    }

    /// Dispatch a media-server tool and merge its display-hint assets on
    /// success; surface failures in the status line. The governed invoker
    /// path — identical to `load_gallery`/`load_jobs`.
    fn dispatch_edit(
        &mut self,
        tool: &'static str,
        params: serde_json::Value,
        describe: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — edit actions unavailable.".into());
            cx.notify();
            return;
        };
        self.status = Some(format!("{tool}: {describe}…"));
        cx.notify();
        let task = invoker.invoke_tool(MEDIA_SERVER, tool, params);
        cx.spawn(async move |this, cx| match task.await {
            Ok(text) => {
                this.update(cx, |this, cx| {
                    this.ingest_tool_result(&Value::String(text), tool);
                    this.status = None;
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.status = Some(format!("{tool} failed: {}", error.message()));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Trim the selected asset to the widget's transport marks via
    /// `video_clip`. The result surfaces as a new playable asset.
    fn dispatch_trim(&mut self, cx: &mut Context<Self>) {
        let (in_secs, out_secs) = self
            .media_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).trim_range())
            .unwrap_or_else(|| {
                self.status = Some("Set both in and out marks before trimming.".into());
                (f64::NAN, f64::NAN)
            });
        if in_secs.is_nan() {
            cx.notify();
            return;
        }
        let src = self
            .media_widget
            .as_ref()
            .map(|widget| widget.read(cx).src().to_string())
            .unwrap_or_default();
        self.dispatch_edit(
            "video_clip",
            serde_json::json!({
                "video_url": src,
                "start_sec": in_secs,
                "end_sec": out_secs,
            }),
            format!("trimming {in_secs:.1}s–{out_secs:.1}s"),
            cx,
        );
    }

    /// Concatenate the queued assets via `video_concat`. The result surfaces
    /// as a new playable asset.
    fn dispatch_concat(&mut self, cx: &mut Context<Self>) {
        if self.concat_queue.len() < 2 {
            self.status = Some("Queue at least two clips to concatenate.".into());
            cx.notify();
            return;
        }
        let urls = self.concat_queue.clone();
        self.dispatch_edit(
            "video_concat",
            serde_json::json!({ "video_urls": urls }),
            format!("concatenating {} clips", urls.len()),
            cx,
        );
    }

    /// Force-refresh the view pane: drop every cached viz widget (a widget
    /// built against a broken environment — e.g. video decode before the
    /// feature fix — keeps rendering broken until evicted) and reload the
    /// active tab's data from its source.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        hkask_viz_core::clear_widget_cache();
        match self.active_tab {
            ViewerTab::Media => {
                // Re-ingest the thread's tool-result display hints. The
                // assets list is rebuilt from scratch so entries removed
                // from the thread disappear too.
                let thread = self.thread.clone();
                if let Some(thread) = thread
                    && let Some(thread) = thread.upgrade()
                {
                    self.assets.clear();
                    self.selected = None;
                    self.media_widget = None;
                    self.media_widget_body = None;
                    self.ingest_thread(thread, cx);
                } else {
                    self.status =
                        Some("No conversation thread to re-ingest — open a thread first.".into());
                }
            }
            ViewerTab::Library => self.load_gallery(cx),
            ViewerTab::Queue => self.load_jobs(cx),
            ViewerTab::Detail => self.load_detail(cx),
        }
        cx.notify();
    }

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
        // Capture what selection and the delete-confirmation point at
        // BEFORE the merge: gallery indexes are positional, so a deletion
        // renumbers every later asset. Re-locating by src afterwards keeps
        // the selection on the same asset, and never lets a stale confirm
        // index point at a different row — confirming would then delete
        // the WRONG asset.
        let selected_src = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        let confirmed_src = self
            .confirm_delete
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        merge_gallery_records(&mut self.assets, records, self.gallery_offset);
        if let Some(src) = selected_src {
            self.selected = self.assets.iter().position(|asset| asset.src == src);
            if self.selected.is_none() {
                self.detail = None; // the selected asset left the list
            }
        }
        if let Some(src) = confirmed_src {
            self.confirm_delete = self.assets.iter().position(|asset| asset.src == src);
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
                let jobs = hkask_mcp_media::tools::jobs::parse_job_list_response(&text);
                this.update(cx, |this, cx| {
                    match jobs {
                        Ok(jobs) => {
                            this.jobs = jobs;
                            this.status = None;
                        }
                        Err(error) => this.status = Some(format!("job_list failed: {error}")),
                    }
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
            // min_w_0 + a truncating label: without them the tab's
            // min-content (its full label, e.g. "Library (12)") fed the
            // pane's min-content and the tab bar overflowed narrow docks.
            .min_w_0()
            .overflow_hidden()
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
            .child(
                Label::new(label)
                    .size(LabelSize::Small)
                    .truncate()
                    .color(if active {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
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
        &mut self,
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
        let asset = self.assets[ix].clone();

        // The media widget for the selected asset — SHARED with the
        // conversation-inline render via the viz-core registry (one player
        // per body; two players would play two audio streams). The viewer
        // keeps the entity so the edit toolbar can reach its playback clock
        // and trim marks. Recreated when the selection changes.
        if self.media_widget_body.as_deref() != Some(asset.body.as_str()) {
            match hkask_viz_core::shared_media_widget(&asset.body, window, cx) {
                Some(widget) => {
                    self.media_widget = Some(widget);
                    self.media_widget_body = Some(asset.body.clone());
                }
                None => {
                    self.media_widget = None;
                    self.media_widget_body = None;
                    return self
                        .render_empty(
                            "Unrenderable media block",
                            "The tool produced a display hint the viewer cannot render.",
                        )
                        .into_any_element();
                }
            }
        }
        let Some(widget) = self.media_widget.clone() else {
            return self
                .render_empty(
                    "Unrenderable media block",
                    "The tool produced a display hint the viewer cannot render.",
                )
                .into_any_element();
        };

        // The header row is width-contained: the pane is a flex item whose
        // min-content is the widest row's content width, so an untruncated
        // src label here used to inflate the whole pane past the dock
        // (min_w_0 + truncate breaks that propagation; overflow_hidden is
        // the clip of last resort at extreme narrowness).
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .min_w_0()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(asset.src.clone())
                    .size(LabelSize::Small)
                    .color(ui::Color::Muted)
                    .flex_1()
                    .truncate(),
            )
            .child(
                Label::new(asset.tool.clone())
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint)
                    .truncate(),
            );

        // ── Edit toolbar: trim marks + concatenation queue ───────────────
        // Width discipline: the pane is a flex item whose min-content is
        // this row's content width, and seven buttons + two readout labels
        // ≈ 800px — wider than any dock pane. The row wraps (the same
        // overflow policy as the Detail tab's kv rows) so every action
        // stays reachable at ~320px panes; min_w_0 keeps the row's
        // min-content from inflating the pane; the readout labels truncate.
        let (position_label, marks_label, trim_ready) = widget.read(cx).edit_state_labels();
        let concat_count = self.concat_queue.len();
        let queued_current = self.concat_queue.contains(&asset.src);
        let toolbar = h_flex()
            .gap_2()
            .px_3()
            .py_1()
            .flex_wrap()
            .min_w_0()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Label::new(position_label)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .truncate(),
            )
            .child(
                Label::new(marks_label)
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .truncate(),
            )
            .child(
                ui::Button::new("mark-in", "Mark In")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.mark_in(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("mark-out", "Mark Out")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.mark_out(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("clear-marks", "Clear Marks")
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener({
                        let widget = widget.clone();
                        move |_this, _event, _window, cx| {
                            widget.update(cx, |widget, cx| widget.clear_marks(cx));
                        }
                    })),
            )
            .child(
                ui::Button::new("trim", "Trim to Marks")
                    .label_size(LabelSize::XSmall)
                    .disabled(!trim_ready)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.dispatch_trim(cx);
                    })),
            )
            .child(
                ui::Button::new(
                    "queue-concat",
                    if queued_current {
                        "Queued ✓"
                    } else {
                        "Queue for Concat"
                    },
                )
                .label_size(LabelSize::XSmall)
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    if let Some(position) = this.selected
                        && let Some(asset) = this.assets.get(position)
                        && !this.concat_queue.contains(&asset.src)
                    {
                        this.concat_queue.push(asset.src.clone());
                    }
                    cx.notify();
                })),
            )
            .child(
                ui::Button::new("concat", format!("Concat ({concat_count})"))
                    .label_size(LabelSize::XSmall)
                    .disabled(concat_count < 2)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.dispatch_concat(cx);
                    })),
            )
            .when(concat_count > 0, |el| {
                el.child(
                    ui::Button::new("clear-queue", "Clear Queue")
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.concat_queue.clear();
                            cx.notify();
                        })),
                )
            });

        // No scroll wrapper on the Media tab: a scroll container gives
        // its children indefinite height, and the video widget's
        // aspect-fit sizing needs a definite height derived from the
        // pane (min_h_0 lets flex_1 shrink below content size).
        // Library/Queue/Detail scroll; the player fills.
        let content = div()
            .id("media-viewer-content")
            .flex_1()
            .min_h_0()
            .p_3()
            .child(widget);

        // flex_1 + min_h_0, NOT size_full: this root is a flex-column child
        // of the viewer (main axis = vertical), where h_full means 100% of
        // the PARENT height — tab bar + 100% overflows the pane by the tab
        // bar's height (the "player larger than the window" bug). flex_1
        // takes the REMAINING height; min_h_0 lets it shrink below content.
        v_flex()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(toolbar)
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
                    // Width-contained: tab labels truncate and the row
                    // clips rather than inflating the pane's min-content
                    // past the dock (the tab bar is the viewer's widest
                    // fixed chrome at narrow pane widths).
                    .min_w_0()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.tab_button(ViewerTab::Media, "Media".into(), cx))
                    .child(self.tab_button(ViewerTab::Library, library_label, cx))
                    .child(self.tab_button(ViewerTab::Queue, queue_label, cx))
                    .child(self.tab_button(ViewerTab::Detail, "Detail".into(), cx))
                    // Force-refresh: rebuild cached widgets + reload the
                    // active tab. Right-aligned so it stays put as tab labels
                    // change width. flex_shrink_0 keeps the icon whole when
                    // the row is tight.
                    .child(
                        div().ml_auto().flex_shrink_0().child(
                            ui::IconButton::new("media-viewer-refresh", ui::IconName::RotateCw)
                                .icon_size(ui::IconSize::Small)
                                .tooltip(ui::Tooltip::text(
                                    "Refresh — rebuild widgets and reload this tab",
                                ))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.refresh(cx);
                                })),
                        ),
                    )
                    .when_some(self.status.clone(), |el, status| {
                        el.child(
                            Label::new(status)
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error)
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
    let payload = hkask_media_widget::MediaBlockBody::parse(&body).ok()?;
    payload.to_media_ref().ok()?;
    let kind = payload.kind;
    let src = payload.src;
    Some(MediaAsset {
        body,
        src,
        kind,
        tool: tool.clone(),
        gallery_index: None,
    })
}

/// Reconcile the asset list against one `gallery_list_assets` page.
/// Records merge by src: a conversation-surfaced asset for the same file
/// gains its gallery index; new gallery assets append. Then removals: the
/// page vouches for global indexes `[offset, offset + len)` (the server's
/// `index` is `offset + i` — the `image_index` other gallery tools accept),
/// so an indexed asset inside that window whose src is absent from the
/// page was deleted from the gallery — drop it. Unindexed
/// (conversation-surfaced) assets and assets whose index falls outside the
/// page are untouched: the page says nothing about them.
fn merge_gallery_records(assets: &mut Vec<MediaAsset>, records: &[Value], offset: usize) {
    let listed_srcs: HashSet<&str> = records
        .iter()
        .filter_map(|record| record.get("path").and_then(|path| path.as_str()))
        .collect();
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
        if let Some(existing) = assets.iter_mut().find(|existing| existing.src == asset.src) {
            existing.gallery_index = asset.gallery_index;
        } else {
            assets.push(asset);
        }
    }
    let window = offset..offset + records.len();
    assets.retain(|asset| match asset.gallery_index {
        Some(index) if window.contains(&index) => listed_srcs.contains(asset.src.as_str()),
        _ => true,
    });
}

/// Tools whose completion can change the gallery's asset listing — the
/// signal that the Library's cached listing is stale. Deliberately
/// over-inclusive on the `gallery_` prefix: a reload is idempotent, a
/// missed mutation is the stale-list bug. The importers index new assets
/// into the gallery directly (per their tool docs); generation tools do
/// not touch the index — their output surfaces via display hints.
fn tool_mutates_gallery(tool_name: &str) -> bool {
    const IMPORTERS: &[&str] = &["video_fetch", "video_extract_frames"];
    tool_name.starts_with("gallery_") || IMPORTERS.contains(&tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(body: &str) -> String {
        format!("```media\n{body}\n```")
    }

    /// expect: [P7] MediaViewer::ingest_tool_result makes new artifacts visible
    /// without an unrelated render; replaying an existing artifact stays quiet.
    #[gpui::test]
    fn ingest_tool_result_notifies_observers_only_for_new_assets(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _subscription = cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&viewer, move |_, _| {
                notifications.set(notifications.get() + 1);
            })
        });
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);

        for (index, source) in ["/tmp/structured.png", "/tmp/text.png"]
            .into_iter()
            .enumerate()
        {
            let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
                serde_json::json!({"output": source}),
                "generate_image",
                "image",
                serde_json::json!({}),
                None,
            );
            let output = if index == 0 {
                output
            } else {
                Value::String(output.to_string())
            };
            viewer.update(cx, |viewer, _| {
                viewer.ingest_tool_result(&output, "generate_image")
            });
            cx.run_until_parked();
            assert_eq!(
                notifications.get(),
                index + 1,
                "new artifact must notify GPUI observers"
            );

            viewer.update(cx, |viewer, _| {
                viewer.ingest_tool_result(&output, "generate_image")
            });
            cx.run_until_parked();
            assert_eq!(
                notifications.get(),
                index + 1,
                "duplicate must not request another redraw"
            );
        }
        viewer.update(cx, |viewer, _| {
            viewer.ingest_tool_result(&serde_json::json!({"status": "ok"}), "gallery_status");
        });
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            2,
            "hint-free output does not change the view"
        );
    }

    /// expect: [P7] The public viewer ingress accepts both live transports,
    /// retains metadata, selects new assets, and does not duplicate replays.
    #[gpui::test]
    fn ingest_tool_result_accepts_structured_and_text_transports(cx: &mut gpui::TestAppContext) {
        let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
            serde_json::json!({"output": "/tmp/雪/quote\"-back\\slash\nimage.png"}),
            "generate_image",
            "image",
            serde_json::json!({"prompt": "snow"}),
            None,
        );
        let wrapped = serde_json::json!({"content": output});
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _| {
            viewer.detail = Some(serde_json::json!({"stale": true}));
            viewer.ingest_tool_result(&wrapped, "generate_image");
            assert_eq!(viewer.assets.len(), 1);
            assert_eq!(viewer.selected, Some(0));
            assert!(viewer.detail.is_none());
            viewer.ingest_tool_result(&Value::String(wrapped.to_string()), "generate_image");
            viewer.ingest_tool_result(&output, "generate_image");
            assert_eq!(viewer.assets.len(), 1);
            let second = hkask_mcp_media::media_block::media_block("video", "/tmp/second.mp4");
            viewer.ingest_tool_result(
                &serde_json::json!({"display_hints": [second]}),
                "generate_video",
            );
            assert_eq!(viewer.selected, Some(1));
            assert_eq!(viewer.assets[1].src, "/tmp/second.mp4");
        });
    }

    struct JobListInvoker(String);

    impl hkask_tool_invoker::ToolInvoker for JobListInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: Value,
        ) -> gpui::Task<Result<String, hkask_tool_invoker::InvokeError>> {
            assert_eq!(server, MEDIA_SERVER);
            assert_eq!(tool, "job_list");
            assert_eq!(args, serde_json::json!({"limit": 20}));
            gpui::Task::ready(Ok(self.0.clone()))
        }
    }

    /// expect: [P7] Queue refresh distinguishes an empty queue from a broken
    /// response, and preserves the last good rows when refresh fails.
    #[gpui::test]
    fn load_jobs_surfaces_array_rows_and_response_failures(cx: &mut gpui::TestAppContext) {
        struct RestoreInvoker(Option<std::sync::Arc<dyn hkask_tool_invoker::ToolInvoker>>);
        impl Drop for RestoreInvoker {
            fn drop(&mut self) {
                hkask_tool_invoker::set_tool_invoker(self.0.take());
            }
        }
        let _restore = RestoreInvoker(hkask_tool_invoker::shared_tool_invoker());
        let record = hkask_mcp_media::types::JobRecord {
            id: "job-1".into(),
            op: "generate_image".into(),
            status: "running".into(),
            created_at: "2026-09-05T00:00:00Z".into(),
            completed_at: None,
            result: None,
            error: None,
        };
        let viewer = cx.new(|_| MediaViewer::new());
        let response = serde_json::json!({"content": [record]}).to_string();
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(response))));
        viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert_eq!(viewer.jobs.len(), 1);
            assert_eq!(viewer.jobs[0].id, "job-1");
            assert!(viewer.status.is_none());
        });
        for response in [
            "not json",
            "{}",
            r#"{"content":{"jobs":[]}}"#,
            r#"{"content":[{"id":"broken"}]}"#,
            r#"{"error":"job store unavailable","kind":"unavailable"}"#,
        ] {
            hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(
                response.into(),
            ))));
            viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
            cx.run_until_parked();
            viewer.update(cx, |viewer, _| {
                assert_eq!(viewer.jobs.len(), 1, "failed refresh preserves known jobs");
                let status = viewer.status.as_deref().expect("visible failure");
                assert!(status.contains("job_list"));
                if response.contains("job store unavailable") {
                    assert!(status.contains("job store unavailable"));
                }
            });
        }
        hkask_tool_invoker::set_tool_invoker(Some(std::sync::Arc::new(JobListInvoker(
            r#"{"content":[]}"#.into(),
        ))));
        viewer.update(cx, |viewer, cx| viewer.load_jobs(cx));
        cx.run_until_parked();
        viewer.update(cx, |viewer, _| {
            assert!(viewer.jobs.is_empty());
            assert!(viewer.status.is_none());
        });
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

    /// expect: [P7] Invalid media never becomes a selectable phantom asset.
    #[test]
    fn asset_from_hint_rejects_missing_src_and_unsupported_kind() {
        for body in [
            r#"{"kind":"image"}"#,
            r#"{"kind":"hologram","src":"/tmp/x"}"#,
        ] {
            assert!(asset_from_hint(&hint(body), &"generate_image".into()).is_none());
        }
    }

    /// expect: [P7] Server-authored hints preserve paths and provenance in both consumers.
    #[test]
    fn server_hint_round_trips_through_viewer_and_widget() -> anyhow::Result<()> {
        let source = "/tmp/雪/quote\"-back\\slash\nimage.png";
        for kind in ["image", "svg", "video", "audio"] {
            let output = hkask_mcp_media::media_block::enrich_with_omc_and_provenance(
                serde_json::json!({"output": source}),
                "generate_image",
                kind,
                serde_json::json!({"prompt": "雪\n\""}),
                Some("span-1".into()),
            );
            let hint = output["display_hint"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing hint"))?;
            let asset = asset_from_hint(hint, &"generate_image".into())
                .ok_or_else(|| anyhow::anyhow!("missing asset"))?;
            let body = hkask_media_widget::MediaBlockBody::parse(&asset.body)?;
            assert_eq!(asset.src, source);
            assert_eq!(body.to_media_ref()?.src(), source);
            assert_eq!(body.kind, kind);
            assert_eq!(body.ontology.as_deref(), Some("omc:CreativeWork"));
            assert_eq!(body.provenance.tool.as_deref(), Some("generate_image"));
            assert_eq!(body.provenance.span_id.as_deref(), Some("span-1"));
            assert_eq!(
                body.provenance.args,
                serde_json::json!({"prompt": "雪\n\""})
            );
            // Additional metadata stays in the original body handed to the renderer.
            let mut extended: Value = serde_json::from_str(&asset.body)?;
            extended["width"] = serde_json::json!(640);
            extended["height"] = serde_json::json!(480);
            extended["caption"] = serde_json::json!("雪\n\"");
            let extended_hint = self::hint(&extended.to_string());
            let asset = asset_from_hint(&extended_hint, &"generate_image".into())
                .ok_or_else(|| anyhow::anyhow!("extended body rejected"))?;
            assert_eq!(serde_json::from_str::<Value>(&asset.body)?, extended);
        }
        Ok(())
    }

    #[test]
    fn asset_from_hint_rejects_non_media_fences_and_bad_json() {
        assert!(asset_from_hint("```rust\nfn main() {}\n```", &"t".into()).is_none());
        assert!(asset_from_hint("```media\nnot json\n```", &"t".into()).is_none());
        assert!(asset_from_hint("plain text", &"t".into()).is_none());
    }

    fn indexed_asset(src: &str, index: usize) -> MediaAsset {
        MediaAsset {
            body: format!(r#"{{"kind":"image","src":"{src}"}}"#),
            src: src.into(),
            kind: "image".into(),
            tool: "gallery".into(),
            gallery_index: Some(index),
        }
    }

    fn surfaced_asset(src: &str) -> MediaAsset {
        MediaAsset {
            body: format!(r#"{{"kind":"image","src":"{src}"}}"#),
            src: src.into(),
            kind: "image".into(),
            tool: "generate_image".into(),
            gallery_index: None,
        }
    }

    /// The confirmed-delete bug this pins: deleting in the Library reloads
    /// the listing, but the merge used to only add and update — the deleted
    /// asset's row never left the list, so a confirmed removal appeared to
    /// do nothing. The merge must reconcile: in-page absences are
    /// deletions; unindexed (conversation-surfaced) and out-of-page assets
    /// stay; survivors' indexes follow the positional shift; a
    /// conversation-surfaced asset for an indexed file gains its index
    /// (listing indices are the gallery image_index contract — server:
    /// `index = offset + i`).
    #[test]
    fn merge_gallery_records_merges_by_src_and_reconciles_deletions() {
        let mut assets = vec![
            surfaced_asset("/gallery/keep-a.png"),
            indexed_asset("/gallery/deleted.png", 1),
            indexed_asset("/gallery/keep-b.png", 2),
            surfaced_asset("/tmp/chat-only.png"),
            indexed_asset("/gallery/out-of-page.png", 150),
        ];
        // The fresh page (offset 0): deleted.png is gone; keep-b shifted
        // from index 2 to 1.
        let records = serde_json::json!([
            {"index": 0, "path": "/gallery/keep-a.png", "media_type": "image"},
            {"index": 1, "path": "/gallery/keep-b.png", "media_type": "image"},
        ]);
        merge_gallery_records(&mut assets, records.as_array().unwrap(), 0);

        let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec![
                "/gallery/keep-a.png",
                "/gallery/keep-b.png",
                "/tmp/chat-only.png",
                "/gallery/out-of-page.png",
            ],
            "the deleted in-page asset must drop; unindexed and out-of-page assets stay"
        );
        assert_eq!(
            assets.first().expect("keep-a survives").gallery_index,
            Some(0),
            "a conversation-surfaced asset for an indexed file gains its index"
        );
        assert_eq!(
            assets
                .iter()
                .find(|a| a.src == "/gallery/keep-b.png")
                .expect("keep-b survives")
                .gallery_index,
            Some(1),
            "a survivor's index follows the positional shift"
        );
    }

    /// A page fetched at a non-zero offset vouches only for its own window:
    /// an asset on an earlier page absent from THIS page must survive —
    /// only an in-window absence is a deletion.
    #[test]
    fn merge_gallery_records_reconciles_only_the_fetched_window() {
        let mut assets = vec![
            indexed_asset("/gallery/early.png", 5),
            indexed_asset("/gallery/in-window-deleted.png", 100),
            indexed_asset("/gallery/in-window-kept.png", 101),
        ];
        // Page at offset 100: one record — in-window-kept shifted down to
        // 100 because in-window-deleted was removed.
        let records = serde_json::json!([
            {"index": 100, "path": "/gallery/in-window-kept.png", "media_type": "image"},
        ]);
        merge_gallery_records(&mut assets, records.as_array().unwrap(), 100);

        let srcs: Vec<&str> = assets.iter().map(|a| a.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec!["/gallery/early.png", "/gallery/in-window-kept.png"],
            "only in-window absences are deletions; earlier-page assets survive"
        );
    }

    /// The chat-driven reload trigger: gallery mutations and direct
    /// importers reload the Library listing; generation tools don't touch
    /// the index (their output surfaces via display hints) and neither do
    /// unrelated tools.
    #[test]
    fn gallery_mutation_classification() {
        assert!(tool_mutates_gallery("gallery_delete_image"));
        assert!(tool_mutates_gallery("gallery_organize"));
        assert!(tool_mutates_gallery("gallery_refresh"));
        assert!(tool_mutates_gallery("gallery_add_media"));
        assert!(tool_mutates_gallery("video_fetch"));
        assert!(tool_mutates_gallery("video_extract_frames"));
        assert!(!tool_mutates_gallery("generate_image"));
        assert!(!tool_mutates_gallery("video_clip"));
        assert!(!tool_mutates_gallery("transcribe"));
        assert!(!tool_mutates_gallery("job_list"));
    }

    /// Selection and delete-confirmation repair across a reconciling
    /// merge: gallery indexes are positional, so after a deletion the old
    /// indices point at different assets. The selection must follow its
    /// asset (or clear with its detail when the asset left), and a pending
    /// confirmation must follow its asset — a stale confirm index would
    /// delete the WRONG asset on confirm.
    #[gpui::test]
    fn merge_gallery_listing_repairs_selection_and_confirm(cx: &mut gpui::TestAppContext) {
        let viewer = cx.new(|_| MediaViewer::new());
        viewer.update(cx, |viewer, _| {
            viewer.assets = vec![
                indexed_asset("/gallery/doomed.png", 0),
                indexed_asset("/gallery/survivor.png", 1),
            ];
            viewer.selected = Some(0); // the asset about to be deleted
            viewer.confirm_delete = Some(1); // the survivor, pre-shift
        });
        let payload = serde_json::json!({
            "total": 1,
            "assets": [
                {"index": 0, "path": "/gallery/survivor.png", "media_type": "image"},
            ]
        });
        viewer.update(cx, |viewer, cx| {
            viewer.merge_gallery_listing(Some(payload), cx)
        });
        viewer.update(cx, |viewer, _| {
            assert_eq!(
                viewer.selected,
                Some(0),
                "the deleted selection falls back to the first asset"
            );
            assert!(
                viewer.detail.is_none(),
                "the deleted asset's detail is cleared"
            );
            assert_eq!(
                viewer.confirm_delete,
                Some(0),
                "the pending confirmation follows its asset to the shifted index"
            );
        });
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    /// The edit-dispatch ingestion path: a `video_clip` result (the exact
    /// envelope shape the server emits — content-wrapped, display_hint
    /// inside) must surface as a new selected asset, the same way thread
    /// tool results do. Ground truth for the shape: the server's
    /// `enrich_with_omc_and_provenance` (hkask-mcp-media/src/media_block.rs).
    #[test]
    fn merge_tool_result_surfaces_clip_as_new_selected_asset() {
        let mut viewer = MediaViewer::new();
        let output = serde_json::json!({
            "content": {
                "status": "clipped",
                "source": "/tmp/source.mp4",
                "start_sec": 10.0,
                "end_sec": 40.0,
                "duration": 30.0,
                "output": "/tmp/clip.mp4",
                "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
            }
        })
        .to_string();

        viewer.ingest_tool_result(&Value::String(output), "video_clip");

        assert_eq!(viewer.assets.len(), 1, "the clip must surface as an asset");
        let selected = viewer
            .selected
            .expect("the new asset must be auto-selected");
        assert_eq!(viewer.assets[selected].src, "/tmp/clip.mp4");
        assert_eq!(viewer.assets[selected].kind, "video");
        assert_eq!(viewer.assets[selected].tool, "video_clip");
    }

    /// Dedup: re-ingesting the same result must not duplicate the asset
    /// (the same discipline `ingest_thread` applies to thread results).
    #[test]
    fn merge_tool_result_deduplicates_by_body() {
        let mut viewer = MediaViewer::new();
        let output = serde_json::json!({
            "content": {
                "status": "clipped",
                "output": "/tmp/clip.mp4",
                "display_hint": "```media\n{\"kind\":\"video\",\"src\":\"/tmp/clip.mp4\"}\n```"
            }
        })
        .to_string();

        let output = Value::String(output);
        viewer.ingest_tool_result(&output, "video_clip");
        viewer.ingest_tool_result(&output, "video_clip");

        assert_eq!(
            viewer.assets.len(),
            1,
            "identical results must not duplicate"
        );
    }

    /// A result without a display_hint (or unparseable output) must not
    /// crash or add phantom assets — it is a no-op the status line already
    /// covers by clearing on success.
    #[test]
    fn merge_tool_result_ignores_output_without_display_hint() {
        let mut viewer = MediaViewer::new();
        viewer.ingest_tool_result(
            &serde_json::json!({"content": {"status": "clipped"}}),
            "video_clip",
        );
        assert!(viewer.assets.is_empty());
        viewer.ingest_tool_result(&Value::String("not json at all".into()), "video_clip");
        assert!(viewer.assets.is_empty());
    }
}

#[cfg(test)]
mod viewer_layout_tests {
    use super::*;
    use gpui::{DefiniteLength, TestAppContext, px, size};

    const FIXTURE: &str =
        "/home/mdz-axolotl/Documents/zk-data/media-mcp/generated/vonnegut-shape-of-stories.mp4";

    /// Layout ground truth at the VIEWER level — the full production chain
    /// (tab bar → tab content → toolbar → player), not the widget in
    /// isolation. The widget-level test passed while the app was broken,
    /// so the break is in this chain. The property: the video area's
    /// laid-out height tracks window height exactly.
    #[gpui::test]
    fn viewer_video_area_scales_with_window_size(cx: &mut TestAppContext) {
        if !std::path::Path::new(FIXTURE).exists() {
            return;
        }
        init_layout_test_globals(cx);

        let viewer = cx.new(|_| MediaViewer::new());
        let output = serde_json::json!({
            "content": {
                "status": "fetched",
                "display_hint": format!(
                    "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
                )
            }
        })
        .to_string();
        viewer.update(cx, |viewer, _| {
            viewer.ingest_tool_result(&Value::String(output), "video_fetch")
        });
        let asset_count = viewer.update(cx, |viewer, cx| {
            let count = viewer.assets.len();
            let _ = cx;
            count
        });
        assert_eq!(asset_count, 1, "fixture asset must merge");

        struct Host {
            viewer: Entity<MediaViewer>,
        }
        impl gpui::Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                // The production embedding: a pane-sized flex child, as the
                // media panel renders the viewer.
                div().size_full().child(self.viewer.clone())
            }
        }

        let (_, cx) = cx.add_window_view(|_window, _cx| Host {
            viewer: viewer.clone(),
        });
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        let short_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out in the viewer chain");

        // THE FIT assertion — the one this bug class evaded: the whole
        // widget (video area + transport) must fit INSIDE the window. The
        // broken layout (h_full on the tab content, a flex-column main-axis
        // child) made the widget extend past the window bottom by the tab
        // bar's height while every scaling assertion passed.
        let widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out with debug bounds");
        assert!(
            widget_bounds.bottom() <= px(600.) + px(1.),
            "the player must fit inside a 600px window: widget bottom {:?}",
            widget_bounds.bottom()
        );

        cx.simulate_resize(size(px(800.), px(900.)));
        cx.run_until_parked();
        let tall_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out after resize");

        // 300px window delta minus sub-pixel rounding — the property is
        // "tracks the window", not exact pixel equality.
        let height_delta = tall_bounds.size.height - short_bounds.size.height;
        assert!(
            height_delta > px(295.) && height_delta < px(305.),
            "video area height must track window height through the full viewer chain: \
             {:?} at 600px vs {:?} at 900px (delta {height_delta:?})",
            short_bounds.size.height,
            tall_bounds.size.height
        );

        let tall_widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out after resize");
        assert!(
            tall_widget_bounds.bottom() <= px(900.) + px(1.),
            "the player must fit inside a 900px window: widget bottom {:?}",
            tall_widget_bounds.bottom()
        );
    }

    /// The production panel embedding for the narrow-pane probes: the
    /// viewer pane on top (`flex_1`, `min_h_0`, `min_w_0`), the 1px split
    /// divider, and the director below at a fractional height — exactly as
    /// `media_panel.rs` renders them (minus the drag wiring, which needs a
    /// real pointer drag the harness doesn't synthesize).
    struct NarrowPaneHost {
        viewer: Entity<MediaViewer>,
    }

    impl gpui::Render for NarrowPaneHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .child(
                    div()
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .child(self.viewer.clone()),
                )
                .child(div().w_full().flex_shrink_0().h(px(1.)))
                .child(
                    div()
                        .w_full()
                        .h(DefiniteLength::Fraction(0.5))
                        .min_h_0()
                        .flex()
                        .flex_col(),
                )
        }
    }

    /// The theme/settings globals the viewer's render reads — shared by
    /// this module's layout tests.
    fn init_layout_test_globals(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                settings::init(cx);
            }
            if !cx.has_global::<theme::GlobalTheme>() {
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            }
        });
    }

    /// THE horizontal-fit invariant — the one the scaling test above does
    /// not encode. No part of the viewer — media content, header, toolbar,
    /// or tab bar — may exceed the pane's available width at any pane
    /// width, with the video's natural width (640px) larger than the pane.
    /// Historically the pane div lacked `min_w_0`, so the viewer's
    /// min-content width (untruncated header src + seven inline toolbar
    /// buttons ≈ 685px) inflated the pane and the video rendered clipped
    /// at the dock edge, unviewable. This test pins the fix in the current
    /// production embedding (top/bottom split): the pane is a flex-column
    /// child and still carries `min_w_0`. The final iteration resizes back
    /// — the no-thrash probe.
    #[gpui::test]
    fn viewer_content_fits_narrow_pane(cx: &mut TestAppContext) {
        if !std::path::Path::new(FIXTURE).exists() {
            return;
        }
        init_layout_test_globals(cx);

        let viewer = cx.new(|_| MediaViewer::new());
        let output = serde_json::json!({
            "content": {
                "status": "fetched",
                "display_hint": format!(
                    "```media\n{{\"kind\":\"video\",\"src\":\"{FIXTURE}\"}}\n```"
                )
            }
        })
        .to_string();
        viewer.update(cx, |viewer, _| {
            viewer.ingest_tool_result(&Value::String(output), "video_fetch")
        });

        let (_, cx) = cx.add_window_view(|_window, _cx| NarrowPaneHost {
            viewer: viewer.clone(),
        });

        // (dock width, pane available) — the split is horizontal
        // (top/bottom panes), so the viewer spans the dock's full width;
        // the director no longer takes a horizontal share. 480px is the
        // adversarial dock: far narrower than the fixture's natural 640px
        // video.
        for (dock_width, pane_available) in [
            (px(700.), px(700.)),
            (px(480.), px(480.)),
            (px(700.), px(700.)),
        ] {
            cx.simulate_resize(size(dock_width, px(600.)));
            cx.run_until_parked();
            assert_viewer_fits_narrow_pane(cx, dock_width, pane_available);
        }
    }

    /// The shared fit assertions: the video area and the whole player
    /// widget must stay within the dock's right edge, and the video area's
    /// width must stay within the pane's available width (+1px sub-pixel
    /// tolerance).
    fn assert_viewer_fits_narrow_pane(
        cx: &mut gpui::VisualTestContext,
        dock_width: gpui::Pixels,
        pane_available: gpui::Pixels,
    ) {
        let widget_bounds = cx
            .debug_bounds("media-widget")
            .expect("widget laid out with debug bounds");
        let video_bounds = cx
            .debug_bounds("media-video-area")
            .expect("video area laid out with debug bounds");
        assert!(
            widget_bounds.right() <= dock_width + px(1.),
            "the player must fit inside the {dock_width:?} dock: widget right {:?}",
            widget_bounds.right()
        );
        assert!(
            video_bounds.right() <= dock_width + px(1.),
            "the video area must fit inside the {dock_width:?} dock: right {:?}",
            video_bounds.right()
        );
        assert!(
            video_bounds.size.width <= pane_available + px(1.),
            "the video area must stay within the pane's ~{pane_available:?} available \
             width, got {:?} — the pane inflated to its content's min-width",
            video_bounds.size.width
        );
    }
}
