use super::*;

impl MediaViewer {
    /// Dispatch a media-server tool and merge its display-hint assets on
    /// success; surface failures in the status line. The governed invoker
    /// path — identical to `load_gallery`/`load_jobs`.
    pub(super) fn dispatch_edit(
        &mut self,
        tool: &'static str,
        params: serde_json::Value,
        describe: String,
        cx: &mut Context<Self>,
    ) {
        let epoch = match self.begin_edit(tool) {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.status = None;
        self.edit_progress = Some(format!("{tool}: {describe}…"));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_edit_result(
                epoch,
                tool,
                Err("Tool invoker not wired — edit actions unavailable.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(MEDIA_SERVER, tool, params);
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|error| error.message());
            this.update(cx, |this, cx| {
                this.apply_edit_result(epoch, tool, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// Admit exactly one direct side-effecting edit at a time.
    pub(super) fn begin_edit(&mut self, tool: &'static str) -> Result<u64, String> {
        if self.edit_request.state == ResourceState::Loading {
            return Err("Another media edit is already in progress; wait for it to finish.".into());
        }
        self.edit_request
            .begin(RequestOwner::Edit(tool))
            .ok_or_else(|| "This media edit is already in progress.".to_string())
    }

    /// expect: The admitted edit remains visible until its result or failure arrives.
    /// [P1] Motivating: superseded edits cannot overwrite the current operation state.
    /// pre: epoch was returned by edit_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    pub(super) fn apply_edit_result(
        &mut self,
        epoch: u64,
        tool: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        match result {
            Ok(text) => {
                self.ingest_tool_result(&Value::String(text), tool, cx);
                self.edit_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.edit_request
                    .complete(epoch, Err(format!("{tool} failed: {error}")));
            }
        }
        self.edit_progress = None;
        cx.notify();
    }

    /// Trim the selected asset to the widget's transport marks via
    /// `video_clip`. The result surfaces as a new playable asset.
    pub(super) fn dispatch_trim(&mut self, cx: &mut Context<Self>) {
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

    /// expect: Concat admission accepts only videos and never truncates an over-cap queue.
    /// [P1] Motivating: rejected media work is visible before dispatch.
    /// pre: selected may identify any viewer asset.
    /// post: valid unique videos append up to MAX_CONCAT_ITEMS; every rejection preserves the queue.
    pub(super) fn queue_selected_for_concat(&mut self) -> Result<(), String> {
        let asset = self
            .selected
            .and_then(|position| self.assets.get(position))
            .ok_or_else(|| "Select a video before queueing it for concat.".to_string())?;
        if asset.kind != "video" {
            return Err("Only video assets can be queued for concat.".into());
        }
        if self.concat_queue.contains(&asset.src) {
            return Ok(());
        }
        if self.concat_queue.len() >= hkask_media::media_limits::MAX_CONCAT_ITEMS {
            return Err(format!(
                "Concat queue limit is {} videos; remove one before adding another.",
                hkask_media::media_limits::MAX_CONCAT_ITEMS
            ));
        }
        self.concat_queue.push(asset.src.clone());
        Ok(())
    }

    /// Concatenate the queued assets via `video_concat`. The result surfaces
    /// as a new playable asset.
    pub(super) fn dispatch_concat(&mut self, cx: &mut Context<Self>) {
        if self.concat_queue.len() < 2 {
            self.status = Some("Queue at least two clips to concatenate.".into());
            cx.notify();
            return;
        }
        if self.concat_queue.len() > hkask_media::media_limits::MAX_CONCAT_ITEMS {
            self.status = Some(format!(
                "Concat queue exceeds the {}-video limit.",
                hkask_media::media_limits::MAX_CONCAT_ITEMS
            ));
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

    /// Load the inspector data for the selected asset (spec: metadata, tags,
    /// lineage, versions). Addresses the asset by its stable gallery id —
    /// the positional index is display data and cannot survive a root
    /// switch or reordering.
    pub(super) fn load_detail(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail = None;
            self.detail_request.invalidate();
            cx.notify();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail = None;
            self.detail_request.invalidate();
            self.status = Some(
                "Asset is not in the gallery index yet — run gallery_refresh \
                 from the curator to index it."
                    .into(),
            );
            cx.notify();
            return;
        };
        let Some(epoch) = self
            .detail_request
            .begin(RequestOwner::Detail(asset_id.clone()))
        else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load detail.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_asset_detail",
            serde_json::json!({ "image_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => hkask_types::tool_response::parse_tool_response(&text)
                    .filter(Value::is_object)
                    .ok_or_else(|| {
                        "gallery_asset_detail returned malformed object data.".to_string()
                    }),
                Err(error) => Err(format!("gallery_asset_detail failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_detail_result(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Detail preserves its last-good object unless the latest selected-asset request succeeds.
    /// [P1] Motivating: superseded detail errors cannot blank or degrade newer detail.
    /// pre: epoch was returned by detail_request.begin.
    /// post: stale outcomes are ignored and owned failures retain detail.
    pub(super) fn apply_detail_result(
        &mut self,
        epoch: u64,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_request.owns(epoch) {
            return;
        }
        match result {
            Ok(detail) => {
                self.detail = Some(detail);
                self.detail_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the selected asset's transcript summaries for the inspector's
    /// Transcripts tab via
    /// `educt_list_transcripts`. A read beside `load_detail`, not chained to
    /// it: both are keyed by the same stable asset id and protected by their
    /// own request lifecycles, so a slow transcripts response cannot blank a
    /// loaded detail and vice versa. Guard failures stay silent here —
    /// `load_detail` already surfaces the no-selection and unindexed-asset
    /// statuses for the same selection.
    pub(super) fn load_detail_transcripts(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            return;
        };
        let Some(epoch) = self
            .detail_transcripts_request
            .begin(RequestOwner::DetailTranscripts(asset_id.clone()))
        else {
            return;
        };
        self.detail_transcripts = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_transcripts_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_list_transcripts",
            serde_json::json!({ "gallery_asset_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_transcript_listing(&value),
                    None => Err("educt_list_transcripts returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_list_transcripts failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_detail_transcripts_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer transcripts request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress visible detail state.
    /// pre: epoch was returned by detail_transcripts_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    pub(super) fn apply_detail_transcripts_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<TranscriptSummaryRow>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_transcripts_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.detail_transcripts = Some(rows);
                self.detail_transcripts_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_transcripts_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the selected asset's researcher tags/notes via
    /// `gallery_annotation_list` — the inspector's Annotations tab
    /// (2026-10-27). A read beside `load_detail`, keyed by the same stable
    /// asset id and protected by its own request lifecycle; guard failures
    /// stay silent (load_detail surfaces the no-selection and
    /// unindexed-asset statuses for the same selection).
    pub(super) fn load_detail_annotations(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected.filter(|ix| *ix < self.assets.len()) else {
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            return;
        };
        let Some(asset_id) = self.assets[ix].gallery_asset_id.clone() else {
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            return;
        };
        let Some(epoch) = self
            .detail_annotations_request
            .begin(RequestOwner::DetailAnnotations(asset_id.clone()))
        else {
            return;
        };
        self.detail_annotations = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_detail_annotations_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load annotations.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_annotation_list",
            serde_json::json!({ "gallery_asset_id": asset_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_annotation_listing(&value),
                    None => Err("gallery_annotation_list returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "gallery_annotation_list failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_detail_annotations_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer annotations request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping annotation loads cannot regress visible detail state.
    /// pre: epoch was returned by detail_annotations_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    pub(super) fn apply_detail_annotations_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<PaneAnnotation>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_annotations_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.detail_annotations = Some(rows);
                self.detail_annotations_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.detail_annotations_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Dispatch a researcher-annotation edit (create/update/delete) under
    /// the single edit admission, then reload the annotation list so the
    /// section reflects it.
    pub(super) fn dispatch_annotation_edit(
        &mut self,
        tool: &'static str,
        params: serde_json::Value,
        done: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .and_then(|asset| asset.gallery_asset_id.as_deref())
            .is_none()
        {
            self.status = Some(
                "Asset is not in the gallery index yet — annotations need an indexed asset.".into(),
            );
            cx.notify();
            return;
        }
        let epoch = match self.begin_edit(tool) {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(format!("{tool}…"));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_annotation_edit_result(
                epoch,
                done,
                Err("Tool invoker not wired — panel cannot edit annotations.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(MEDIA_SERVER, tool, params);
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|error| error.message());
            this.update(cx, |this, cx| {
                this.apply_annotation_edit_result(epoch, done, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The owned annotation edit commits once and the list reloads
    /// with it; failures surface visibly and clear progress.
    pub(super) fn apply_annotation_edit_result(
        &mut self,
        epoch: u64,
        done: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status = Some(format!("Annotation {done} — the list reloads."));
                self.edit_request.complete(epoch, Ok(()));
                self.load_detail_annotations(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err(format!("annotation {done} failed")));
            }
        }
        cx.notify();
    }

    /// Open the annotation editor for a new tag/note or an existing row's
    /// value, focus its input, and hint the keys (the transcript label
    /// editor's modal pattern).
    pub(super) fn open_annotation_editor(
        &mut self,
        edit: AnnotationEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .annotation_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let replacing = edit.annotation_id.is_some();
        self.status = Some(if replacing {
            "Replace the value — Enter saves; Escape cancels.".into()
        } else {
            "Write the tag or note — Enter saves; Escape cancels.".into()
        });
        self.annotation_edit = Some(edit);
        focus.focus(window, cx);
        cx.notify();
    }

    /// Confirm the open annotation editor: create dispatches with the typed
    /// value, replace dispatches over the row's identity. Blank values
    /// refuse visibly and keep the buffer; an in-flight edit refuses and
    /// keeps the buffer for retry.
    pub(super) fn confirm_annotation_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.annotation_edit.take() else {
            return;
        };
        if self.edit_request.state == ResourceState::Loading {
            self.annotation_edit = Some(edit);
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        let value = edit.buffer.label();
        if value.is_empty() {
            self.annotation_edit = Some(edit);
            self.status = Some("A tag or note needs text — Escape cancels.".into());
            cx.notify();
            return;
        }
        let Some(asset_id) = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .and_then(|asset| asset.gallery_asset_id.clone())
        else {
            self.annotation_edit = Some(edit);
            self.status = Some(
                "Asset is not in the gallery index yet — annotations need an indexed asset.".into(),
            );
            cx.notify();
            return;
        };
        match edit.annotation_id.clone() {
            None => self.dispatch_annotation_edit(
                "gallery_annotation_create",
                annotation_create_params(&asset_id, edit.kind, &value),
                "created",
                cx,
            ),
            Some(annotation_id) => self.dispatch_annotation_edit(
                "gallery_annotation_update",
                annotation_update_params(&asset_id, &annotation_id, &value),
                "updated",
                cx,
            ),
        }
    }

    /// Cancel the open annotation editor without dispatching.
    pub(super) fn cancel_annotation_edit(&mut self, cx: &mut Context<Self>) {
        if self.annotation_edit.take().is_some() {
            self.status = Some("Annotation edit cancelled.".into());
            cx.notify();
        }
    }

    /// Route the annotation editor's keystrokes — the label editor's key
    /// handling over the annotation confirm/cancel pair.
    pub(super) fn handle_annotation_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match keystroke.key.as_str() {
            "escape" => self.cancel_annotation_edit(cx),
            "enter" => self.confirm_annotation_edit(cx),
            _ => {
                if let Some(edit) = self.annotation_edit.as_mut()
                    && edit.buffer.apply_keystroke(keystroke)
                {
                    cx.notify();
                }
            }
        }
    }

    /// Paste clipboard text into the open annotation editor (control
    /// characters dropped — single-line values).
    pub(super) fn paste_into_annotation_edit(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(edit) = self.annotation_edit.as_mut() {
            edit.buffer.insert_str(&text);
            cx.notify();
        }
    }

    /// The inspector's annotation editor row — the label editor's input,
    /// parameterized for a tag/note value. An associated fn so the caller
    /// can hold the detail state's immutable borrow.
    pub(super) fn render_annotation_editor(
        edit: &AnnotationEdit,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let context = if edit.annotation_id.is_some() {
            format!("Replace {}", edit.kind)
        } else {
            format!("New {}", edit.kind)
        };
        let typed = edit.buffer.text();
        let cursor = edit.buffer.cursor();
        let before: String = typed.chars().take(cursor).collect();
        let after: String = typed.chars().skip(cursor).collect();
        let mut input = div()
            .id("media-inspector-annotation-input")
            .track_focus(focus)
            .flex()
            .items_center()
            .min_w_0()
            .w(px(160.))
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border)
            .px_1()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let keystroke = &event.keystroke;
                if (keystroke.modifiers.control || keystroke.modifiers.platform)
                    && keystroke.key == "v"
                {
                    this.paste_into_annotation_edit(cx);
                } else {
                    this.handle_annotation_key(&event.keystroke, cx);
                }
                cx.stop_propagation();
            }))
            .on_click({
                let focus = focus.clone();
                cx.listener(move |_, _, window, cx| {
                    window.focus(&focus, cx);
                })
            });
        if typed.is_empty() {
            input = input.child(
                Label::new(SharedString::from("(value)"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        } else {
            input = input
                .child(
                    Label::new(SharedString::from(before))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                )
                .child(
                    div()
                        .id("media-inspector-annotation-caret")
                        .w(px(1.))
                        .h(px(12.))
                        .bg(cx.theme().colors().text),
                )
                .child(
                    Label::new(SharedString::from(after))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
        }
        div()
            .id("media-inspector-annotation-editor")
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                Label::new(SharedString::from(context))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(input)
            .child(
                div()
                    .id("media-inspector-annotation-confirm")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("save"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_annotation_edit(cx);
                    })),
            )
            .child(
                div()
                    .id("media-inspector-annotation-cancel")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("cancel"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_annotation_edit(cx);
                    })),
            )
            .into_any_element()
    }

    /// The edit toolbar (trim marks + concatenation queue) — shared by the
    /// plain workspace and the transcript sub-surface (T3): both bind the
    /// same shared player, so the toolbar serves the recording either way.
    /// Width discipline: the pane is a flex item whose min-content is this
    /// row's content width, and seven buttons + two readout labels ≈ 800px —
    /// wider than any dock pane. The row wraps (the same overflow policy as
    /// the inspector's kv rows) so every action stays reachable at ~320px
    /// panes; min_w_0 keeps the row's min-content from inflating the pane;
    /// the readout labels truncate.
    pub(super) fn render_edit_toolbar(
        &self,
        widget: &Entity<hkask_media_widget::MediaWidget>,
        asset: &MediaAsset,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (position_label, marks_label, trim_ready) = widget.read(cx).edit_state_labels();
        let edit_active = self.edit_request.state == ResourceState::Loading;
        let concat_count = self.concat_queue.len();
        let queued_current = self.concat_queue.contains(&asset.src);
        h_flex()
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
                    .disabled(!trim_ready || edit_active)
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
                    match this.queue_selected_for_concat() {
                        Ok(()) => this.status = None,
                        Err(error) => this.status = Some(error),
                    }
                    cx.notify();
                })),
            )
            .child(
                ui::Button::new("concat", format!("Concat ({concat_count})"))
                    .label_size(LabelSize::XSmall)
                    .disabled(concat_count < 2 || edit_active)
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
            })
    }

    /// The workspace's inspector toggle chip (T1): the supporting pane is
    /// part of the workspace, not a navigation destination — one click
    /// hides or returns it without touching the player or selection.
    /// Shared by the plain workspace header and the transcript
    /// sub-surface's header (T3), so the pane stays reachable in both.
    pub(super) fn workspace_inspector_toggle(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id("media-workspace-inspector-toggle")
            .flex_shrink_0()
            .px_2()
            .h_5()
            .flex()
            .items_center()
            .rounded_sm()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .cursor_pointer()
            .child(
                Label::new(if self.inspector_open {
                    "Hide inspector"
                } else {
                    "Inspector"
                })
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_inspector(cx)))
    }

    /// The workspace's inspector pane (T1): the seven Detail sections as
    /// pane-internal tabs — alternate views of the same selected object
    /// (HIG "Tab views": closely related areas of content). A supporting
    /// pane, not a destination: it renders under the player, toggles
    /// without touching the selection or the player, and every tab
    /// renders the same selected asset.
    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tabs = h_flex()
            .id("media-inspector-tabs")
            .gap_1()
            .px_2()
            .py_1()
            .flex_wrap()
            .min_w_0()
            .overflow_hidden()
            .border_t_1()
            .border_color(cx.theme().colors().border_variant)
            .children(InspectorTab::ALL.map(|tab| self.inspector_tab_button(tab, cx)));
        let Some(detail) = self.detail.as_ref() else {
            // The pane-level data states (the old Detail tab's empty view):
            // the inspector's reads follow the selection, so every tab
            // shares one loading/failed/idle state. An unindexed selection
            // states its own condition instead of pointing elsewhere.
            let unindexed_selection = self
                .selected
                .and_then(|ix| self.assets.get(ix))
                .is_some_and(|asset| asset.gallery_asset_id.is_none());
            let (title, hint) = match &self.detail_request.state {
                ResourceState::Loading => (
                    "Loading inspector",
                    "Fetching the selected asset's metadata…",
                ),
                ResourceState::Failed(error) => ("Inspector unavailable", error.as_str()),
                ResourceState::Ready => {
                    ("Inspector ready — empty", "The asset returned no detail.")
                }
                ResourceState::Idle if unindexed_selection => (
                    "No inspector data",
                    "The selected asset is not in the gallery index — run gallery_refresh to index it.",
                ),
                ResourceState::Idle => (
                    "No inspector data",
                    "Select an indexed gallery asset in the Library — the inspector follows the selection.",
                ),
            };
            return v_flex()
                .id("media-inspector")
                .h(gpui::relative(INSPECTOR_PANE_FRACTION))
                .min_h_0()
                .flex_shrink_0()
                .debug_selector(|| "media-inspector".into())
                .child(tabs)
                .child(self.render_empty(title, hint).into_any_element())
                .into_any_element();
        };
        v_flex()
            .id("media-inspector")
            .h(gpui::relative(INSPECTOR_PANE_FRACTION))
            .min_h_0()
            .flex_shrink_0()
            .debug_selector(|| "media-inspector".into())
            .child(tabs)
            .child(
                v_flex()
                    .id("media-inspector-section")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.render_inspector_section(detail, cx)),
            )
            .into_any_element()
    }

    /// One pane-internal inspector tab chip (T1) — the tab bar's tab
    /// button at pane scale: the active tab carries the selected
    /// treatment; the row wraps so all seven stay reachable in a narrow
    /// dock.
    pub(super) fn inspector_tab_button(
        &self,
        tab: InspectorTab,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.inspector_tab == tab;
        div()
            .id(tab.element_id())
            .debug_selector(move || tab.element_id().to_string())
            .px_2()
            .h_6()
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
                Label::new(tab.label())
                    .size(LabelSize::Small)
                    .truncate()
                    .color(if active {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.inspector_tab = tab;
                cx.notify();
            }))
    }

    /// The active inspector tab's body (T1) — the old Detail sections, one
    /// per pane-internal tab, with honest absence copy where a section's
    /// data is missing (the stacked view could hide an empty section; a
    /// tab must always render something).
    pub(super) fn render_inspector_section(
        &self,
        detail: &Value,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match self.inspector_tab {
            InspectorTab::Record => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(image) = detail.get("image").and_then(|i| i.as_object()) {
                    for (key, value) in image {
                        if let Some(text) = scalar_text(value) {
                            body = body.child(Self::kv_row(key, &text, cx));
                        }
                    }
                } else {
                    body = body.child(Self::inspector_absent("No record in the asset detail."));
                }
                body.into_any_element()
            }
            InspectorTab::Tags => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(tags) = detail.get("tags").and_then(|t| t.as_array()) {
                    for tag in tags {
                        let tag_type = tag.get("tag_type").and_then(|t| t.as_str()).unwrap_or("?");
                        let value = tag.get("value").and_then(|v| v.as_str()).unwrap_or("?");
                        body = body.child(Self::kv_row(tag_type, value, cx));
                    }
                } else {
                    body = body.child(Self::inspector_absent("No tags."));
                }
                body.into_any_element()
            }
            InspectorTab::Lineage => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(map) = detail
                    .get("lineage")
                    .filter(|lineage| !lineage.is_null())
                    .and_then(|lineage| lineage.as_object())
                {
                    for (key, value) in map {
                        if let Some(text) = scalar_text(value) {
                            body = body.child(Self::kv_row(key, &text, cx));
                        }
                    }
                } else {
                    body = body.child(Self::inspector_absent("No lineage."));
                }
                body.into_any_element()
            }
            InspectorTab::Faces => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(faces) = detail.get("faces").and_then(|f| f.as_array())
                    && !faces.is_empty()
                {
                    body = body.child(Self::kv_row("count", &faces.len().to_string(), cx));
                } else {
                    body = body.child(Self::inspector_absent("No faces."));
                }
                body.into_any_element()
            }
            InspectorTab::Transcripts => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(transcripts) = &self.detail_transcripts {
                    if transcripts.is_empty() {
                        body = body.child(Self::inspector_absent(
                            "No transcripts yet — transcribe the recording and one appears here.",
                        ));
                    }
                    for transcript in transcripts {
                        let timings = if transcript.has_word_timings {
                            "word timings"
                        } else {
                            "no word timings"
                        };
                        let transcript_id = transcript.id.clone();
                        let mut row = div()
                            .id(SharedString::from(format!(
                                "media-inspector-open-{}",
                                transcript.id
                            )))
                            .cursor_pointer()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .child(
                                Label::new(SharedString::from(transcript.id.clone()))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Muted),
                            )
                            .child(
                                Label::new(SharedString::from(format!(
                                    "{} words, {timings}",
                                    transcript.words_count
                                )))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                            );
                        if let Some(note) = transcript_alignment_note(&transcript.alignment) {
                            row = row.child(
                                Label::new(SharedString::from(note))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Error),
                            );
                        }
                        body = body.child(row.on_click(cx.listener(move |this, _, _, cx| {
                            this.open_transcript(transcript_id.clone(), cx);
                        })));
                    }
                }
                // A failed reload keeps the last-good rows visible and
                // states the failure beside them (the request-lifecycle
                // contract: failures surface, never silence); an in-flight
                // first load shows its own copy.
                if let ResourceState::Failed(error) = &self.detail_transcripts_request.state {
                    body = body.child(Self::inspector_absent(&format!(
                        "Transcripts unavailable: {error}"
                    )));
                }
                if self.detail_transcripts.is_none()
                    && self.detail_transcripts_request.state == ResourceState::Loading
                {
                    body = body.child(Self::inspector_absent("Loading transcripts…"));
                }
                body.into_any_element()
            }
            InspectorTab::Annotations => {
                let mut body = v_flex().gap_2().p_3();
                // The annotation surface (2026-10-27): researcher tags/notes
                // with direct create/replace/delete — the same
                // direct-manipulation rationale as the transcript
                // highlights, on the inspector's home surface. The whole
                // tab needs an indexed asset.
                let Some(asset_id) = self
                    .selected
                    .and_then(|ix| self.assets.get(ix))
                    .and_then(|asset| asset.gallery_asset_id.clone())
                else {
                    return body
                        .child(Self::inspector_absent(
                            "Annotations need an indexed asset — surface this asset through the gallery first.",
                        ))
                        .into_any_element();
                };
                if let Some(edit) = self.annotation_edit.clone()
                    && let Some(focus) = self.annotation_input_focus.clone()
                {
                    body = body.child(Self::render_annotation_editor(&edit, &focus, cx));
                } else {
                    for kind in ["tag", "note"] {
                        body = body.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-inspector-annotation-add-{kind}"
                                )))
                                .cursor_pointer()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .child(
                                    Label::new(SharedString::from(format!("add {kind}")))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Muted),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_annotation_editor(
                                        AnnotationEdit {
                                            kind,
                                            annotation_id: None,
                                            buffer: LabelBuffer::default(),
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        );
                    }
                }
                if let Some(annotations) = &self.detail_annotations {
                    for annotation in annotations {
                        let annotation_id = annotation.annotation_id.clone();
                        let row_label = format!("{}: {}", annotation.kind, annotation.value);
                        let edit_annotation_id = annotation.annotation_id.clone();
                        let edit_value = annotation.value.clone();
                        let kind_static: &'static str = if annotation.kind == "note" {
                            "note"
                        } else {
                            "tag"
                        };
                        let delete_asset_id = asset_id.clone();
                        body = body.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-inspector-annotation-{annotation_id}"
                                )))
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    Label::new(SharedString::from(row_label))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Default),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "media-inspector-annotation-edit-{annotation_id}"
                                        )))
                                        .cursor_pointer()
                                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                                        .child(
                                            Label::new(SharedString::from("edit"))
                                                .size(LabelSize::XSmall)
                                                .color(ui::Color::Default),
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_annotation_editor(
                                                AnnotationEdit {
                                                    kind: kind_static,
                                                    annotation_id: Some(edit_annotation_id.clone()),
                                                    buffer: LabelBuffer::from_str(&edit_value),
                                                },
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "media-inspector-annotation-delete-{annotation_id}"
                                        )))
                                        .cursor_pointer()
                                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                                        .child(
                                            Label::new(SharedString::from("delete"))
                                                .size(LabelSize::XSmall)
                                                .color(ui::Color::Error),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.dispatch_annotation_edit(
                                                "gallery_annotation_delete",
                                                annotation_delete_params(
                                                    &delete_asset_id,
                                                    &annotation_id,
                                                ),
                                                "deleted",
                                                cx,
                                            );
                                        })),
                                ),
                        );
                    }
                }
                if let ResourceState::Failed(error) = &self.detail_annotations_request.state {
                    body = body.child(Self::inspector_absent(&format!(
                        "Annotations unavailable: {error}"
                    )));
                }
                if self.detail_annotations.is_none()
                    && self.detail_annotations_request.state == ResourceState::Loading
                {
                    body = body.child(Self::inspector_absent("Loading annotations…"));
                }
                body.into_any_element()
            }
            InspectorTab::Reel => {
                let mut body = v_flex().gap_2().p_3();
                if let Some(provenance) = parse_reel_provenance(detail) {
                    body = body.child(Self::kv_row("rendered from transcript", &provenance, cx));
                } else {
                    body = body.child(Self::inspector_absent(
                        "No reel provenance — the asset was not rendered from a transcript.",
                    ));
                }
                body.into_any_element()
            }
        }
    }
}
