use super::*;

impl MediaViewer {
    /// Open a transcript in the workspace's sub-surface (T3, S3a): clear
    /// prior state, select the linked recording so the workspace stays with
    /// the asset (the ruling — header, edit toolbar, and inspector pane all
    /// serve the recording the transcript belongs to), switch to the
    /// Workspace mode, and dispatch `educt_get_transcript` for the id — the
    /// entry point of the Library's transcript rows and the inspector's
    /// Transcripts tab.
    pub(super) fn open_transcript(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_mode = false;
        self.selection_anchor = None;
        self.word_drag_anchor = None;
        self.drag_last_word = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.reset_transcript_search();
        self.highlight_edit = None;
        self.reel_clips.clear();
        self.transcript_follow_task.take();
        self.stop_reel_preview(cx);
        // A fresh workspace starts on the word surface with no visual-event
        // or redaction state carried over (S5).
        self.transcript_surface = TranscriptSurface::Words;
        self.visual_events = None;
        self.visual_events_request.invalidate();
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.redaction_target = None;
        self.redaction_render = None;
        // A fresh open starts the virtualized list from the top — a
        // previous workspace's reading position must not survive a
        // reopen (S3d).
        self.transcript_list.reset(0);
        self.transcript_list_count = 0;
        self.follow_reveals_in_flight = 0;
        self.follow_viewport = true;
        // The reading surface is the sub-surface's primary area (T3): the
        // inspector pane yields its screen share on open — the header's
        // toggle restores it in one click, serving the same recording
        // (its reads re-fire on reopen). With the pane open in a narrow
        // dock, purpose + header + pane + the pinned 140px player row
        // starve the reading list below usability (the S3d starvation the
        // header-items design exists to avoid).
        self.inspector_open = false;
        // R3 selection coherence (T3): when the Library's listing links the
        // transcript to a recording on the current page, select that
        // recording so the workspace chrome serves the transcript's own
        // asset. An unlinked transcript (or one whose recording is on
        // another page) keeps the current selection — the sub-surface
        // degrades to its own header copy, never hides.
        let recording_index = self
            .transcript_listing
            .as_ref()
            .and_then(|listing| {
                listing
                    .iter()
                    .find(|row| row.id == transcript_id)
                    .and_then(|row| row.gallery_asset_id.clone())
            })
            .and_then(|asset_id| {
                self.assets
                    .iter()
                    .position(|asset| asset.gallery_asset_id.as_deref() == Some(asset_id.as_str()))
            });
        if let Some(ix) = recording_index
            && self.selected != Some(ix)
        {
            self.set_selection(Some(ix), cx);
        }
        self.activate_tab(ViewerTab::Workspace, cx);
        self.load_transcript_with(transcript_id, cx);
    }

    /// Close the transcript sub-surface back to the plain asset workspace
    /// (S6, T3): the transcript state clears, the follow task cancels, and
    /// the operator returns to the selected recording's plain workspace —
    /// the same mode, the surface they left. The recordings browser is the
    /// Library's recordings section now, reachable by its mode at any time,
    /// so the 2026-10-26 reachability concern (the browser disappearing
    /// after the first open) is structural: a mode can never be unreachable.
    /// The shared player stays bound in the registry; reopening any
    /// workspace rebinds through the same path.
    pub(super) fn close_transcript_workspace(&mut self, cx: &mut Context<Self>) {
        self.transcript = None;
        self.transcript_request.invalidate();
        self.current_word = None;
        self.selection_mode = false;
        self.selection_anchor = None;
        self.word_drag_anchor = None;
        self.drag_last_word = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.reset_transcript_search();
        self.highlight_edit = None;
        self.reel_clips.clear();
        self.transcript_follow_task.take();
        self.stop_reel_preview(cx);
        self.transcript_surface = TranscriptSurface::Words;
        self.visual_events = None;
        self.visual_events_request.invalidate();
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.redaction_target = None;
        self.redaction_render = None;
        self.status = Some("Back to the asset workspace.".into());
        cx.notify();
    }

    // ── Visual-transcript surface (S5: the events reading surface) ───────

    /// Switch the workspace's reading surface (operator refinement
    /// 2026-10-30): word-transcript pane ⇄ visual-transcript event log —
    /// parallel annotations over the same recording, one seek discipline.
    /// The pending surface-local selections and any open label editor belong
    /// to the surface being left. The first switch to Events loads the
    /// event log for the open recording.
    pub(super) fn toggle_transcript_surface(
        &mut self,
        surface: TranscriptSurface,
        cx: &mut Context<Self>,
    ) {
        if self.transcript_surface == surface {
            return;
        }
        self.transcript_surface = surface;
        self.selection_anchor = None;
        self.selection_popover = None;
        self.context_menu = None;
        self.highlight_edit = None;
        self.event_selection_anchor = None;
        self.event_selection_span = None;
        self.status = Some(match surface {
            TranscriptSurface::Words => {
                "Word transcript — click a word to seek; Select text to annotate.".into()
            }
            TranscriptSurface::Events => {
                "Visual transcript — click an event to seek; Select span to annotate.".into()
            }
        });
        if surface == TranscriptSurface::Events && self.visual_events.is_none() {
            self.load_visual_events(cx);
        }
        cx.notify();
    }

    /// Load the open recording's visual transcript via `layers_transcript`
    /// (S1's reading surface): one time-ordered event list across layer
    /// kinds. The last-good log stays visible while a reload is in flight;
    /// a malformed payload rejects visibly and never degrades to an empty
    /// success.
    pub(super) fn load_visual_events(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let media_path = state.media_path.clone();
        let Some(epoch) = self
            .visual_events_request
            .begin(RequestOwner::VisualEvents(media_path.clone()))
        else {
            return;
        };
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_visual_events_result(
                epoch,
                Err("Tool invoker not wired — panel cannot read the visual transcript.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        // The dispatch pins the maximum page (limit 5000 — the server's
        // cap, clamped 1..=5000): the event log never reads a silent
        // 500-event default prefix. Longer logs still page — the list's
        // bottom truncation note names what the page omits.
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_transcript",
            serde_json::json!({ "media_path": media_path, "limit": 5000 }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_visual_events(&value),
                    None => Err("layers_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("layers_transcript failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_visual_events_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The event-log load's result gate: owned epochs only. A committed
    /// reload clears the index-keyed span selection — the operator
    /// re-clicks rather than selecting against shifted indexes.
    pub(super) fn apply_visual_events_result(
        &mut self,
        epoch: u64,
        result: Result<VisualEventsState, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.visual_events_request.owns(epoch) {
            return;
        }
        match result {
            Ok(state) => {
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.visual_events = Some(state);
                self.visual_events_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.visual_events_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// The event-click state machine under the same two-mode interaction
    /// model as the word pane (S4 part-2): seek-only when selection mode is
    /// off; anchor → span when on; a repeated click on the anchor clears
    /// the pending selection; while the label editor is open, clicks are
    /// inert. Pure so the state machine is pin-testable.
    pub(super) fn handle_event_click(&self, clicked: usize) -> EventClickOutcome {
        if self.highlight_edit.is_some() {
            return EventClickOutcome::EditorOpen;
        }
        if !self.selection_mode {
            return EventClickOutcome::SeekOnly;
        }
        let Some(events) = self
            .visual_events
            .as_ref()
            .map(|state| state.events.as_slice())
        else {
            return EventClickOutcome::SeekOnly;
        };
        match self.event_selection_anchor {
            None => match events.get(clicked) {
                Some(_) => EventClickOutcome::AnchorSet(clicked),
                // A stale anchor index against a reloaded list re-anchors
                // on the clicked row rather than guessing.
                None => EventClickOutcome::SpanRejected,
            },
            Some(anchor) if anchor == clicked => EventClickOutcome::AnchorCleared,
            Some(anchor) => match (events.get(anchor), events.get(clicked)) {
                (Some(first), Some(last)) => {
                    let start = first.start_sec.min(last.start_sec);
                    let end = first.end_or_start().max(last.end_or_start());
                    if end <= start {
                        EventClickOutcome::SpanRejected
                    } else {
                        EventClickOutcome::SpanSelected {
                            start_sec: start,
                            end_sec: end,
                        }
                    }
                }
                _ => EventClickOutcome::SpanRejected,
            },
        }
    }

    /// An event row was clicked in the visual-transcript surface: dispatch
    /// by the selection state machine — seek outside select mode (the
    /// word pane's click-to-seek discipline over the same bound player),
    /// build the span inside it, ignore clicks while the label editor is
    /// open.
    pub(super) fn event_clicked(&mut self, index: usize, start_sec: f64, cx: &mut Context<Self>) {
        match self.handle_event_click(index) {
            EventClickOutcome::SeekOnly => {
                self.seek_workspace_transcript((start_sec * 1000.0) as u64, cx);
            }
            EventClickOutcome::AnchorSet(event) => {
                self.event_selection_anchor = Some(event);
                self.status = Some(
                    "Click the last event to finish the span, then tag it or redact it.".into(),
                );
                cx.notify();
            }
            EventClickOutcome::AnchorCleared => {
                self.event_selection_anchor = None;
                self.status = Some("Span selection cleared.".into());
                cx.notify();
            }
            EventClickOutcome::EditorOpen => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            EventClickOutcome::SpanSelected { start_sec, end_sec } => {
                self.event_selection_anchor = None;
                self.event_selection_span = Some((start_sec, end_sec));
                self.status = Some(
                    "Span selected — Tag span names it; arm a Target and Redact span to redact."
                        .into(),
                );
                cx.notify();
            }
            EventClickOutcome::SpanRejected => {
                self.event_selection_anchor = None;
                self.status =
                    Some("That span is empty — pick two events whose ranges cover time.".into());
                cx.notify();
            }
        }
    }

    /// Arm (or re-arm) the redaction target from one box-carrying event row
    /// — criterion 3's "select target". Only events carrying a normalized
    /// box can arm; the refusal names the requirement.
    pub(super) fn arm_redaction_target(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self
            .visual_events
            .as_ref()
            .and_then(|state| state.events.get(index))
        else {
            return;
        };
        let Some(box_norm) = row.box_norm else {
            self.status = Some("Only events carrying a box can arm a redaction target.".into());
            cx.notify();
            return;
        };
        let label = row
            .object_label
            .clone()
            .unwrap_or_else(|| row.searchable_text.clone());
        self.redaction_target = Some(RedactionTarget {
            label: label.clone(),
            box_norm,
            source_ref: row.source_ref(),
        });
        self.status = Some(format!(
            "Target armed: {label} — select a span, then Redact span."
        ));
        cx.notify();
    }

    /// Open the label editor over the pending event span (criterion 2's
    /// authoring arm) — the same modal editor as the word pane's create
    /// flow; Enter confirms, Escape cancels.
    pub(super) fn open_event_span_tag_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_sec, end_sec)) = self.event_selection_span else {
            return;
        };
        self.open_label_editor(
            HighlightEdit {
                kind: HighlightEditKind::EventTag { start_sec, end_sec },
                buffer: LabelBuffer::default(),
            },
            window,
            cx,
        );
    }

    /// Author a researcher tag over the selected span through
    /// `layers_store` (criterion 2): kind `researcher`, provenance model
    /// `"user"` — a human act, never machine-authored (design §3) — one
    /// item over the span whose searchable text keys the label. The event
    /// log reloads with the stored tag on success.
    pub(super) fn create_event_tag(
        &mut self,
        start_sec: f64,
        end_sec: f64,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let (media_path, gallery_asset_id) =
            (state.media_path.clone(), state.gallery_asset_id.clone());
        let epoch = match self.begin_edit("layers_store") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Storing researcher tag…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_event_authoring_result(
                epoch,
                Err("Tool invoker not wired — panel cannot author tags.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_store",
            researcher_tag_params(
                &media_path,
                gallery_asset_id.as_deref(),
                start_sec,
                end_sec,
                &label,
            ),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "layers_store returned no layer id for the tag.".to_string()
                        }),
                    None => Err("layers_store returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("layers_store failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_event_authoring_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The tag authoring's result gate: owned epochs only; success clears
    /// the consumed span and reloads the event log so the stored tag lands
    /// in it (a researcher layer over the same recording).
    pub(super) fn apply_event_authoring_result(
        &mut self,
        epoch: u64,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status =
                    Some("Researcher tag stored — the visual transcript reloads with it.".into());
                self.edit_request.complete(epoch, Ok(()));
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.load_visual_events(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("tag authoring failed".to_string()));
            }
        }
        cx.notify();
    }

    /// Author an approved redaction decision over the selected span and
    /// dispatch the render (criterion 3): a two-invoke chain under the
    /// single edit admission, exactly the reel-compose precedent — store
    /// the decision layer via `layers_store` (kind `researcher`, model
    /// `"user"`, one approved item carrying the S3 vocabulary: op redact,
    /// status approved, effect, box target, source-tag lineage), then
    /// render via `layers_render_redactions` with the STORED LAYER ID
    /// (required — the render consumes an approved decision layer, never
    /// free parameters). The armed target names the box; the pending span
    /// names the range.
    pub(super) fn dispatch_redaction_render(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(target) = self.redaction_target.clone() else {
            self.status = Some("Arm a target first — click Target on an event with a box.".into());
            cx.notify();
            return;
        };
        let Some(span) = self.event_selection_span else {
            self.status = Some("Select a span first — Select span, then click two events.".into());
            cx.notify();
            return;
        };
        let (media_path, gallery_asset_id) =
            (state.media_path.clone(), state.gallery_asset_id.clone());
        let effect = self.redaction_effect;
        let epoch = match self.begin_edit("layers_store") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Authoring redaction decision & rendering…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_redaction_render_result(
                epoch,
                Err("Tool invoker not wired — panel cannot redact.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let store_task = invoker.invoke_tool(
            MEDIA_SERVER,
            "layers_store",
            redaction_decision_params(
                &media_path,
                gallery_asset_id.as_deref(),
                &target,
                span,
                effect,
            ),
        );
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                result: Result<RedactionRenderOutcome, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_redaction_render_result(epoch, result, cx)
                })
                .log_err();
            }
            let store_text = match store_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        Err(format!("layers_store failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let Some(layer_id) = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(str::to_string)
                })
            else {
                finish(
                    &this,
                    epoch,
                    Err("layers_store returned no layer id for the redaction decision.".into()),
                    cx,
                );
                return;
            };
            let render_task = invoker.invoke_tool(
                MEDIA_SERVER,
                "layers_render_redactions",
                serde_json::json!({
                    "media_path": media_path,
                    "layer_id": layer_id,
                }),
            );
            let render_text = match render_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        Err(format!(
                            "layers_render_redactions failed: {}",
                            error.message()
                        )),
                        cx,
                    );
                    return;
                }
            };
            let outcome = match hkask_types::tool_response::parse_tool_response(&render_text) {
                Some(value) => {
                    parse_redaction_render(&value).map(|summary| RedactionRenderOutcome {
                        // The raw render text rides along: the applied result
                        // routes it through the display-hint ingestion so the
                        // rendered asset lands as a playable panel asset,
                        // exactly as a composed reel does.
                        render_text,
                        summary,
                    })
                }
                None => Err("layers_render_redactions returned no parsable data.".to_string()),
            };
            finish(&this, epoch, outcome, cx);
        })
        .detach();
    }

    /// The redaction chain's result gate: owned epochs only. Success
    /// commits the review/lineage summary, ingests the render's display
    /// hints (landing the rendered asset), surfaces the render counts,
    /// clears the consumed span, and reloads the event log (the decision
    /// layer is a researcher layer over the same recording — its items
    /// appear in the log).
    pub(super) fn apply_redaction_render_result(
        &mut self,
        epoch: u64,
        result: Result<RedactionRenderOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(outcome) => {
                let RedactionRenderOutcome {
                    render_text,
                    summary,
                } = outcome;
                self.ingest_tool_result(
                    &Value::String(render_text),
                    "layers_render_redactions",
                    cx,
                );
                self.status = Some(format!(
                    "Redaction rendered — {} decision(s), {} segment(s) re-encoded, \
                     {} stream-copied. The visual transcript reloads.",
                    summary.decisions_rendered,
                    summary.reencoded_segments,
                    summary.stream_copied_segments
                ));
                self.redaction_render = Some(summary);
                self.edit_request.complete(epoch, Ok(()));
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                self.load_visual_events(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("redaction render failed".to_string()));
            }
        }
        cx.notify();
    }

    /// Load the workspace transcript via `educt_get_transcript` — the
    /// single-dispatch data contract (D2): summary + working view + layers
    /// arrive together; the strict parse rejects a malformed payload rather
    /// than rendering a half view.
    pub(super) fn load_transcript_with(&mut self, transcript_id: String, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .transcript_request
            .begin(RequestOwner::Transcript(transcript_id.clone()))
        else {
            return;
        };
        // The last-good workspace stays visible while a reload is in
        // flight (2026-10-26 review: eagerly nulling it flashed the idle
        // browser after every highlight edit; open_transcript nulls
        // explicitly when the operator leaves one workspace for another).
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_result(
                epoch,
                Err("Tool invoker not wired — panel cannot load the transcript.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_get_transcript",
            serde_json::json!({ "transcript_id": transcript_id }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => parse_transcript_payload(&value),
                    None => Err("educt_get_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!("educt_get_transcript failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                this.apply_transcript_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer transcript request owns the workspace even if an older outcome arrives later.
    /// [P1] Motivating: overlapping transcript loads cannot regress the visible workspace.
    /// pre: epoch was returned by transcript_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    pub(super) fn apply_transcript_result(
        &mut self,
        epoch: u64,
        result: Result<TranscriptPaneState, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_request.owns(epoch) {
            return;
        }
        match result {
            Ok(state) => {
                // S6: every annotation CRUD reloads the workspace. The preview
                // task tears down with the reload (its span plan derives from
                // state that just changed), while the composer state
                // RE-MATCHES to highlight identity and survives — order,
                // inclusion, and in-reel strikes keep their deltas; new
                // researcher highlights join included, deleted ones drop.
                self.stop_reel_preview(cx);
                self.rematch_reel_clips(&state);
                // A fresh workspace invalidates any open label editor and
                // selection popover — their annotation ids and word ranges
                // belong to the previous view — and the transcript search
                // (the pending matches are stale in the reloaded view).
                self.highlight_edit = None;
                self.selection_popover = None;
                self.context_menu = None;
                self.reset_transcript_search();
                self.transcript = Some(state);
                self.current_word = None;
                self.transcript_request.complete(epoch, Ok(()));
                // A fresh workspace state re-reads the recording's layers:
                // the visual-event log and its index-keyed selections do not
                // survive it. While the events surface is live, the reload
                // is dispatched immediately so the log never blanks.
                self.visual_events = None;
                self.visual_events_request.invalidate();
                self.event_selection_anchor = None;
                self.event_selection_span = None;
                if self.transcript_surface == TranscriptSurface::Events {
                    self.load_visual_events(cx);
                }
                self.start_transcript_follow(cx);
                // A fresh workspace follows the mark: tracking on, and the
                // scroll baseline re-anchored so the first tick cannot
                // misread the load-time layout as a manual scroll (S3d).
                self.follow_viewport = true;
                self.follow_reveals_in_flight = 0;
                self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
            }
            Err(error) => {
                self.transcript_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// Load the stored-transcript listing via `educt_list_transcripts` — the
    /// Library's recordings section reads it (T3): transcription states and
    /// nested transcript rows (operator report 2026-10-26: the section must
    /// list what's available, not dead-end).
    /// Delete one stored recording through `educt_delete_transcript` — the
    /// CRUD delete the recordings listing owed the reference model
    /// (recordings are project-scoped assets the operator manages, not
    /// read-only fixtures). Durable exports and rendered reels are preserved
    /// by the server, detached from the removed record.
    pub(super) fn delete_transcript_recording(
        &mut self,
        transcript_id: String,
        cx: &mut Context<Self>,
    ) {
        let epoch = match self.begin_edit("educt_delete_transcript") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Deleting recording…".into());
        self.confirm_delete_transcript.disarm();
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_deletion_result(
                epoch,
                Err("Tool invoker not wired — panel cannot delete recordings.".to_string()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_delete_transcript",
            serde_json::json!({
                "transcript_id": transcript_id,
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("transcripts_removed")
                        .and_then(serde_json::Value::as_u64)
                        .filter(|removed| *removed > 0)
                        .map(|removed| removed as usize)
                        .ok_or_else(|| {
                            "educt_delete_transcript returned no removal acknowledgement."
                                .to_string()
                        }),
                    None => Err("educt_delete_transcript returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_delete_transcript failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_transcript_deletion_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// The deletion's result gate: owned epochs only, the edit lifecycle
    /// closes either way, and a successful deletion reloads the recordings
    /// listing so the removed row disappears from the browser.
    pub(super) fn apply_transcript_deletion_result(
        &mut self,
        epoch: u64,
        result: Result<usize, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(removed) => {
                self.status = Some(format!(
                    "Deleted {removed} recording{} — exports and rendered reels are preserved.",
                    if removed == 1 { "" } else { "s" }
                ));
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_listing(cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("recording deletion failed".to_string()));
            }
        }
        cx.notify();
    }

    pub(super) fn load_transcript_listing(&mut self, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .transcript_listing_request
            .begin(RequestOwner::TranscriptListing)
        else {
            return;
        };
        self.transcript_listing = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_transcript_listing_result(
                epoch,
                Err("Tool invoker not wired — panel cannot list transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_list_transcripts",
            serde_json::json!({ "limit": 500 }),
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
                this.apply_transcript_listing_result(epoch, result, cx)
            })
            .log_err();
        })
        .detach();
    }

    /// expect: A newer listing request owns the rows even if an older outcome arrives later.
    /// [P1] Motivating: overlapping listing loads cannot regress the visible browse surface.
    /// pre: epoch was returned by transcript_listing_request.begin.
    /// post: stale outcomes are ignored; the owned outcome commits exactly once.
    pub(super) fn apply_transcript_listing_result(
        &mut self,
        epoch: u64,
        result: Result<Vec<TranscriptSummaryRow>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_listing_request.owns(epoch) {
            return;
        }
        match result {
            Ok(rows) => {
                self.transcript_listing = Some(rows);
                // A deletion or concurrent listing change may have removed the
                // recording whose Confirm delete is armed — never leave a confirm
                // aimed at a row that no longer exists.
                if let Some(id) = self.confirm_delete_transcript.armed.as_ref()
                    && !self
                        .transcript_listing
                        .as_ref()
                        .is_some_and(|listing| listing.iter().any(|row| row.id == *id))
                {
                    self.confirm_delete_transcript.disarm();
                }
                self.transcript_listing_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.transcript_listing_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    /// The word-click outcome under the two-mode interaction model (S4
    /// part-2): seek-only when selection mode is off; anchor → range when
    /// on; a repeated click on the anchor clears the pending selection;
    /// while the label editor is open, clicks are inert.
    /// Pure so the state machine is pin-testable.
    pub(super) fn handle_word_selection(&self, clicked: usize) -> WordClickOutcome {
        if self.highlight_edit.is_some() {
            // The label editor is modal to its range: stray clicks neither
            // seek the player nor start a new selection (Enter/Escape only).
            return WordClickOutcome::EditorOpen;
        }
        if !self.selection_mode {
            return WordClickOutcome::SeekOnly;
        }
        match self.selection_anchor {
            None => WordClickOutcome::AnchorSet(clicked),
            Some(anchor) if anchor == clicked => WordClickOutcome::AnchorCleared,
            Some(anchor) => WordClickOutcome::CompleteRange {
                start_word: anchor.min(clicked),
                end_word: anchor.max(clicked),
            },
        }
    }

    /// A word was clicked in the workspace: dispatch by the selection
    /// state machine — seek outside select mode, build ranges inside it,
    /// ignore clicks while the label editor is open. Range completion
    /// opens the selection popover (whose Highlight action opens the label
    /// editor), gated on index-identical views (D5: the
    /// panel never re-implements the server's projection; re-anchored
    /// views cannot source-map a hand highlight in v1).
    pub(super) fn word_clicked(
        &mut self,
        global_word: usize,
        start_ms: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.handle_word_selection(global_word) {
            WordClickOutcome::SeekOnly => {
                self.seek_workspace_transcript(start_ms, cx);
            }
            WordClickOutcome::AnchorSet(word) => {
                // A fresh anchor dismisses the tray — the operator is
                // building a new selection, so the pending range's actions
                // will not run. (Seek clicks keep it: seeking is preview,
                // the pending range still stands.)
                self.selection_popover = None;
                self.context_menu = None;
                self.selection_anchor = Some(word);
                self.status = Some(
                    "Click the last word to finish selecting, then create a highlight or cut."
                        .into(),
                );
                cx.notify();
            }
            WordClickOutcome::AnchorCleared => {
                self.selection_anchor = None;
                self.status = Some("Selection cleared.".into());
                cx.notify();
            }
            WordClickOutcome::EditorOpen => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            WordClickOutcome::CompleteRange {
                start_word,
                end_word,
            } => {
                self.selection_anchor = None;
                let alignment = self
                    .transcript
                    .as_ref()
                    .map(|state| state.alignment.as_str())
                    .unwrap_or_default();
                if let Err(reason) = selection_tray_mode(alignment) {
                    self.status = Some(reason.into());
                    cx.notify();
                    return;
                }
                self.open_selection_popover(start_word, end_word, window, cx);
            }
        }
    }

    /// The press-and-release drag outcome (S2) — pure so the state machine
    /// is pin-testable, mirroring [`handle_word_selection`]: the alignment
    /// gate applies at the opening site (`word_drag_released`), not here.
    pub(super) fn handle_word_drag(&self, anchor: usize, release: usize) -> WordDragOutcome {
        if self.highlight_edit.is_some() {
            return WordDragOutcome::Inert;
        }
        if anchor == release {
            return WordDragOutcome::Click;
        }
        WordDragOutcome::CompleteRange {
            start_word: anchor.min(release),
            end_word: anchor.max(release),
        }
    }

    /// A press-and-release completed over the transcript surface (S2):
    /// consume the pending drag, then dispatch by [`WordDragOutcome`] —
    /// same-word releases and editor-open presses are inert (the click
    /// path seeks; the editor is modal to its range), and a cross-word
    /// release opens the selection popover over the ordered union range
    /// under the same index-identical gate as the two-click flow (D5). A
    /// release with no pending anchor or no hovered word is a no-op: the
    /// press was not over a word, or the release landed on a word gap.
    pub(super) fn word_drag_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (anchor, release) = (self.word_drag_anchor.take(), self.drag_last_word);
        let (Some(anchor), Some(release)) = (anchor, release) else {
            return;
        };
        match self.handle_word_drag(anchor, release) {
            WordDragOutcome::Click => {}
            WordDragOutcome::Inert => {
                self.status = Some("Label editor open — Enter confirms, Escape cancels.".into());
                cx.notify();
            }
            WordDragOutcome::CompleteRange {
                start_word,
                end_word,
            } => {
                let alignment = self
                    .transcript
                    .as_ref()
                    .map(|state| state.alignment.as_str())
                    .unwrap_or_default();
                if let Err(reason) = selection_tray_mode(alignment) {
                    self.status = Some(reason.into());
                    cx.notify();
                    return;
                }
                self.open_selection_popover(start_word, end_word, window, cx);
            }
        }
    }

    /// Create a researcher highlight over the selected source range via
    /// `educt_annotation_create` — the label editor's confirm arm — then
    /// reload the workspace so the new highlight lands in the overlay and
    /// list (single-flight via the edit admission).
    pub(super) fn create_workspace_highlight(
        &mut self,
        start_word: usize,
        end_word: usize,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_create") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Creating highlight…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Highlight",
                "created",
                "creation",
                Err("Tool invoker not wired — panel cannot create highlights.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_create",
            highlight_create_params(&transcript_id, start_word, end_word, &label),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|id| id.to_string())
                        .ok_or_else(|| {
                            "educt_annotation_create returned no annotation_id.".to_string()
                        }),
                    None => Err("educt_annotation_create returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_create failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Highlight",
                    "created",
                    "creation",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Apply a researcher correction over the selected working range (the
    /// transcript-layer S5): resolve the selection's SOURCE range (identity
    /// on original/aligned views; `educt_locate` basis "source" on a
    /// re_anchored view — the server-side translation through the
    /// re-anchored index map, the D5 v2 consumer), merge the new edit over
    /// the live correction layer's edits (disjoint priors carried verbatim,
    /// overlaps superseded and surfaced — the silent-revert guard), store
    /// the merged layer, chain `educt_realign_transcript` naming it (an
    /// honest no-op when the layer is one-for-one), and reload the
    /// workspace — one edit admission over the whole chain.
    pub(super) fn dispatch_workspace_correction(
        &mut self,
        start_word: usize,
        end_word: usize,
        replacement: String,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let transcript_id = state.transcript_id.clone();
        let alignment = state.alignment.clone();
        let prior_edits = state.correction_edits.clone();
        // The selection's rendered form — the locate contract quotes the
        // rendered words exactly, and both sides join word text with single
        // spaces (`rendered_transcript`).
        let quote = state
            .words
            .as_deref()
            .and_then(|words| words.get(start_word..=end_word))
            .map(|slice| {
                slice
                    .iter()
                    .map(|word| word.word.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        let epoch = match self.begin_edit("educt_store_layer") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Applying correction…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_correction_result(
                epoch,
                transcript_id,
                Err("Tool invoker not wired — panel cannot correct transcripts.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let locate_needed = alignment == "re_anchored";
        let locate_params =
            correction_locate_params(&transcript_id, quote.as_deref().unwrap_or_default());
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                transcript_id: &str,
                result: Result<usize, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_workspace_correction_result(
                        epoch,
                        transcript_id.to_string(),
                        result,
                        cx,
                    )
                })
                .log_err();
            }
            // 1. The source range: identity on index-identical views; the
            //    server's own translation on re_anchored views.
            let (source_start, source_end) = if locate_needed {
                let locate_text = match invoker
                    .invoke_tool(MEDIA_SERVER, "educt_locate", locate_params)
                    .await
                {
                    Ok(text) => text,
                    Err(error) => {
                        finish(
                            &this,
                            epoch,
                            &transcript_id,
                            Err(format!("educt_locate failed: {}", error.message())),
                            cx,
                        );
                        return;
                    }
                };
                match parse_correction_source_range(&locate_text, start_word, end_word) {
                    Ok(range) => range,
                    Err(error) => {
                        finish(&this, epoch, &transcript_id, Err(error), cx);
                        return;
                    }
                }
            } else {
                (start_word, end_word)
            };
            // 2. Merge over the live layer's edits — the server's own
            //    supersession semantics, never a fork.
            let new_edit = PaneCorrectionEdit {
                start_word: source_start,
                end_word: source_end,
                replacement,
                reason: CORRECTION_REASON.to_string(),
            };
            let prior: Vec<hkask_media::transcript_layers::CorrectionEdit> =
                prior_edits.iter().map(server_correction_edit).collect();
            let (merged, superseded) = hkask_media::transcript_retranscribe::merge_over_prior(
                &prior,
                server_correction_edit(&new_edit),
            );
            let merged_pane: Vec<PaneCorrectionEdit> =
                merged.iter().map(pane_correction_edit).collect();
            let superseded = superseded.len();
            // 3. Store the merged correction layer.
            let store_params = match correction_store_params(
                &transcript_id,
                &merged_pane,
                &hkask_types::time::now_rfc3339(),
            ) {
                Ok(params) => params,
                Err(error) => {
                    finish(&this, epoch, &transcript_id, Err(error), cx);
                    return;
                }
            };
            let store_text = match invoker
                .invoke_tool(MEDIA_SERVER, "educt_store_layer", store_params)
                .await
            {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        &transcript_id,
                        Err(format!("educt_store_layer failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let stored_id = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .pointer("/stored/id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .ok_or_else(|| "educt_store_layer returned no stored layer id.".to_string());
            let stored_id = match stored_id {
                Ok(id) => id,
                Err(error) => {
                    finish(&this, epoch, &transcript_id, Err(error), cx);
                    return;
                }
            };
            // 4. Realign names the stored layer — an honest no-op when the
            //    merged layer is one-for-one; the re-anchoring decision
            //    otherwise (the reload reads the true state).
            let realign_params = correction_realign_params(&transcript_id, &stored_id);
            if let Err(error) = invoker
                .invoke_tool(MEDIA_SERVER, "educt_realign_transcript", realign_params)
                .await
            {
                finish(
                    &this,
                    epoch,
                    &transcript_id,
                    Err(format!(
                        "educt_realign_transcript failed: {}",
                        error.message()
                    )),
                    cx,
                );
                return;
            }
            finish(&this, epoch, &transcript_id, Ok(superseded), cx);
        })
        .detach();
    }

    /// The correction chain's completion (S5): commit once and reload the
    /// workspace so the corrected working view lands; failures surface
    /// visibly and clear progress. The superseded count rides the success
    /// status — overlapping prior corrections are superseded visibly,
    /// never silently (the merge's contract).
    pub(super) fn apply_workspace_correction_result(
        &mut self,
        epoch: u64,
        transcript_id: String,
        result: Result<usize, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(superseded) => {
                self.status = Some(if superseded == 0 {
                    "Correction applied — the transcript reloads with it.".to_string()
                } else {
                    format!(
                        "Correction applied — superseded {superseded} overlapping prior \
                         correction(s) — the transcript reloads with it."
                    )
                });
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_with(transcript_id, cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request
                    .complete(epoch, Err("transcript correction failed".to_string()));
            }
        }
        cx.notify();
    }

    /// Strike the pending range instead of labeling it (R2): close the
    /// editor and dispatch the cut — a strike carries no label, so the
    /// label flow ends here. The range already passed the selection's
    /// alignment gate when the editor opened; an in-flight edit keeps the
    /// editor open for retry (the confirm path's admission contract).
    pub(super) fn strike_selected_range(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        if self.edit_request.state == ResourceState::Loading {
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        self.highlight_edit = None;
        self.create_workspace_strike(start_word, end_word, cx);
    }

    /// Create a researcher strike over the word range via
    /// `educt_annotation_create` (kind strike — a single-Cut EDL with blank
    /// label/note), then reload the workspace so the line-through overlay
    /// and the Strikes list reflect it (single-flight via the edit
    /// admission).
    pub(super) fn create_workspace_strike(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_create") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Cutting selection…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Strike",
                "struck",
                "creation",
                Err("Tool invoker not wired — panel cannot strike words.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_create",
            strike_create_params(&transcript_id, start_word, end_word),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .map(|id| id.to_string())
                        .ok_or_else(|| {
                            "educt_annotation_create returned no annotation_id.".to_string()
                        }),
                    None => Err("educt_annotation_create returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_create failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Strike",
                    "struck",
                    "creation",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// expect: The owned annotation edit (create, rename, delete, or
    /// strike) commits once and the workspace reloads with it; failures
    /// surface visibly and clear progress.
    pub(super) fn apply_workspace_annotation_result(
        &mut self,
        epoch: u64,
        transcript_id: String,
        subject: &'static str,
        done: &'static str,
        failed: &'static str,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.edit_request.owns(epoch) {
            return;
        }
        self.edit_progress = None;
        match result {
            Ok(_acknowledgement) => {
                self.status = Some(format!(
                    "{subject} {done} — the transcript reloads with it."
                ));
                self.edit_request.complete(epoch, Ok(()));
                self.load_transcript_with(transcript_id, cx);
            }
            Err(error) => {
                self.status = Some(error);
                self.edit_request.complete(
                    epoch,
                    Err(format!("{} {failed} failed", subject.to_lowercase())),
                );
            }
        }
        cx.notify();
    }

    /// The pane's compose flow (R3): build the reel EDL directly from the
    /// pane's annotations — Keep ops from the toggled-in researcher
    /// highlights and Cut ops from the strikes, per the mode — store it via
    /// `educt_store_layer`, and render via `educt_render_edl` with the
    /// stored layer id: a two-invoke chain under the single edit admission,
    /// ending in the render's display hint (which lands the reel as a
    /// playable asset through the existing ingestion).
    pub(super) fn compose_workspace_reel(&mut self, mode: ReelComposeMode, cx: &mut Context<Self>) {
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let selection = self.reel_clips.clone();
        let Some(compose_params) = compose_reel_params(state, &selection, mode) else {
            self.status = Some(mode.empty_status().into());
            cx.notify();
            return;
        };
        let transcript_id = state.transcript_id.clone();
        let epoch = match self.begin_edit("educt_store_layer") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(mode.progress().into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_edit_result(
                epoch,
                "educt_store_layer",
                Err("Tool invoker not wired — panel cannot compose reels.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let store_task = invoker.invoke_tool(MEDIA_SERVER, "educt_store_layer", compose_params);
        cx.spawn(async move |this, cx| {
            fn finish(
                this: &gpui::WeakEntity<MediaViewer>,
                epoch: u64,
                tool: &'static str,
                result: Result<String, String>,
                cx: &mut gpui::AsyncApp,
            ) {
                this.update(cx, |viewer, cx| {
                    viewer.apply_edit_result(epoch, tool, result, cx)
                })
                .log_err();
            }
            let store_text = match store_task.await {
                Ok(text) => text,
                Err(error) => {
                    finish(
                        &this,
                        epoch,
                        "educt_store_layer",
                        Err(format!("educt_store_layer failed: {}", error.message())),
                        cx,
                    );
                    return;
                }
            };
            let Some(edl_id) = hkask_types::tool_response::parse_tool_response(&store_text)
                .and_then(|value| {
                    value
                        .pointer("/stored/id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
            else {
                finish(
                    &this,
                    epoch,
                    "educt_store_layer",
                    Err("educt_store_layer returned no EDL layer id.".into()),
                    cx,
                );
                return;
            };
            let render_task = invoker.invoke_tool(
                MEDIA_SERVER,
                "educt_render_edl",
                serde_json::json!({
                    "transcript_id": transcript_id,
                    "layer_id": edl_id,
                }),
            );
            match render_task.await {
                Ok(text) => finish(&this, epoch, "educt_render_edl", Ok(text), cx),
                Err(error) => finish(
                    &this,
                    epoch,
                    "educt_render_edl",
                    Err(format!("educt_render_edl failed: {}", error.message())),
                    cx,
                ),
            }
        })
        .detach();
    }

    /// Delete a researcher-authored annotation (highlight or strike) via
    /// `educt_annotation_delete`, then reload the workspace so the row,
    /// overlay, and any reel selection reflect the removal (single-flight
    /// via the edit admission; the completion reuses the create path's
    /// reload semantics). The subject names the kind for the progress and
    /// failure messages; the server dispatches on the stored layer kind.
    pub(super) fn delete_workspace_annotation(
        &mut self,
        annotation_id: String,
        subject: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_delete") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some(format!("Deleting {}…", subject.to_lowercase()));
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                subject,
                "deleted",
                "deletion",
                Err(format!(
                    "Tool invoker not wired — panel cannot delete {}s.",
                    subject.to_lowercase()
                )),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_delete",
            serde_json::json!({
                "transcript_id": transcript_id,
                "annotation_id": annotation_id,
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("deleted")
                        .and_then(serde_json::Value::as_bool)
                        .filter(|deleted| *deleted)
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "educt_annotation_delete returned no deletion acknowledgement."
                                .to_string()
                        }),
                    None => Err("educt_annotation_delete returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_delete failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    subject,
                    "deleted",
                    "deletion",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Rename a researcher highlight via `educt_annotation_update` — the
    /// label editor's confirm arm for an existing row — preserving its
    /// range and note (only the label changes), then reload the workspace so
    /// the row, overlay, and label color reflect it (single-flight via the
    /// edit admission).
    pub(super) fn rename_workspace_highlight(
        &mut self,
        annotation_id: String,
        start_word: usize,
        end_word: usize,
        label: String,
        note: String,
        cx: &mut Context<Self>,
    ) {
        let Some(transcript_id) = self
            .transcript
            .as_ref()
            .map(|state| state.transcript_id.clone())
        else {
            return;
        };
        let epoch = match self.begin_edit("educt_annotation_update") {
            Ok(epoch) => epoch,
            Err(error) => {
                self.status = Some(error);
                cx.notify();
                return;
            }
        };
        self.edit_progress = Some("Renaming highlight…".into());
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_workspace_annotation_result(
                epoch,
                transcript_id,
                "Highlight",
                "renamed",
                "rename",
                Err("Tool invoker not wired — panel cannot rename highlights.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "educt_annotation_update",
            highlight_update_params(
                &transcript_id,
                &annotation_id,
                start_word,
                end_word,
                &label,
                &note,
            ),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => match hkask_types::tool_response::parse_tool_response(&text) {
                    Some(value) => value
                        .get("annotation_id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| *id == annotation_id)
                        .map(|_| String::new())
                        .ok_or_else(|| {
                            "educt_annotation_update returned no matching annotation_id."
                                .to_string()
                        }),
                    None => Err("educt_annotation_update returned no parsable data.".to_string()),
                },
                Err(error) => Err(format!(
                    "educt_annotation_update failed: {}",
                    error.message()
                )),
            };
            this.update(cx, |this, cx| {
                this.apply_workspace_annotation_result(
                    epoch,
                    transcript_id,
                    "Highlight",
                    "renamed",
                    "rename",
                    result,
                    cx,
                )
            })
            .log_err();
        })
        .detach();
    }

    /// Open the label editor (S4-3) for a completed range (create) or an
    /// existing researcher row (rename), focus its input, and hint the
    /// keys. The editor is modal to the pane's word clicks.
    pub(super) fn open_label_editor(
        &mut self,
        edit: HighlightEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        self.status = Some(match &edit.kind {
            HighlightEditKind::Create { .. } => {
                "Name this highlight, or choose Cut selection. Enter confirms; Escape cancels."
                    .into()
            }
            HighlightEditKind::Correction { .. } => {
                "Type the corrected text for the selection — Enter applies it as a \
                 correction layer; Escape cancels."
                    .into()
            }
            HighlightEditKind::EventTag { .. } => {
                "Name this event tag. Enter confirms; Escape cancels.".into()
            }
            HighlightEditKind::Rename { .. } => {
                "Rename the highlight — Enter saves; Escape cancels.".into()
            }
        });
        self.highlight_edit = Some(edit);
        // The editor supersedes any open selection popover (S2): opening it
        // from the tray's Highlight action or a row's Rename never leaves a
        // stale tray rendered beside the modal editor.
        self.selection_popover = None;
        self.context_menu = None;
        focus.focus(window, cx);
        cx.notify();
    }

    /// Confirm the open label editor: create dispatches with the typed
    /// label (empty falls back to the default), rename dispatches with the
    /// typed label over the unchanged range and note. Admission refusals
    /// (edit in flight, blank rename label) surface in the status line and
    /// keep the typed buffer for retry.
    pub(super) fn confirm_label_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.highlight_edit.take() else {
            return;
        };
        if self.edit_request.state == ResourceState::Loading {
            self.highlight_edit = Some(edit);
            self.status =
                Some("Another media edit is already in progress; wait for it to finish.".into());
            cx.notify();
            return;
        }
        match &edit.kind {
            HighlightEditKind::Create {
                start_word,
                end_word,
            } => {
                let label = resolve_create_label(edit.buffer.text().as_str());
                self.create_workspace_highlight(*start_word, *end_word, label, cx);
            }
            HighlightEditKind::Correction {
                start_word,
                end_word,
            } => {
                // A correction replaces text; an empty or blank
                // replacement is a deletion, which belongs to Cut — the
                // panel-side gate the server's correction-pass guard
                // mirrors (store validation checks bounds and
                // disjointness only).
                let replacement = edit.buffer.text();
                if replacement.trim().is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status = Some(
                        "A correction needs replacement text — deletions belong to Cut; \
                         Escape cancels."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                self.dispatch_workspace_correction(
                    *start_word,
                    *end_word,
                    replacement.trim().to_string(),
                    cx,
                );
            }
            HighlightEditKind::EventTag { start_sec, end_sec } => {
                let label = edit.buffer.label();
                if label.is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status = Some("A tag needs a nonblank label — Escape cancels.".into());
                    cx.notify();
                    return;
                }
                self.create_event_tag(*start_sec, *end_sec, label, cx);
            }
            HighlightEditKind::Rename {
                annotation_id,
                start_word,
                end_word,
                note,
            } => {
                let label = edit.buffer.label();
                if label.is_empty() {
                    self.highlight_edit = Some(edit);
                    self.status =
                        Some("A renamed highlight needs a nonblank label — Escape cancels.".into());
                    cx.notify();
                    return;
                }
                self.rename_workspace_highlight(
                    annotation_id.clone(),
                    *start_word,
                    *end_word,
                    label,
                    note.clone(),
                    cx,
                );
            }
        }
    }

    /// Cancel the open label editor without dispatching anything.
    pub(super) fn cancel_label_edit(&mut self, cx: &mut Context<Self>) {
        if self.highlight_edit.take().is_some() {
            self.status = Some("Label edit cancelled.".into());
            cx.notify();
        }
    }

    /// Paste clipboard text into the open label editor (2026-10-26
    /// review: modifier chords were rejected outright, so a label could
    /// not be pasted). Control characters are dropped — the input is
    /// single-line. Inert with no editor open.
    pub(super) fn paste_into_label_edit(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(edit) = self.highlight_edit.as_mut() {
            edit.buffer.insert_str(&text);
            cx.notify();
        }
    }

    /// Route the label input's keystrokes: Enter confirms, Escape cancels,
    /// editing keys go to the buffer. Bound on the input element itself, so
    /// it only fires while the editor is open.
    pub(super) fn handle_label_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match keystroke.key.as_str() {
            "escape" => self.cancel_label_edit(cx),
            "enter" => self.confirm_label_edit(cx),
            _ => {
                if let Some(edit) = self.highlight_edit.as_mut()
                    && edit.buffer.apply_keystroke(keystroke)
                {
                    cx.notify();
                }
            }
        }
    }

    // ── Selection popover (S2: the completed range's action tray) ─────────

    /// Open the selection popover (S2) over the completed range — the
    /// selection's action tray (Highlight / Cut / Copy time / Include in
    /// reel), pinned above the list like the label editor. The tray takes
    /// focus so its keys work without clicking first: Enter highlights,
    /// Delete cuts, Escape dismisses. Not modal — a newer completed range
    /// replaces this one, a fresh anchor dismisses it, and seek clicks
    /// leave it be (seeking is preview, the pending range stands).
    pub(super) fn open_selection_popover(
        &mut self,
        start_word: usize,
        end_word: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self
            .selection_popover_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The tray owns the completed range; no anchor lingers.
        self.selection_anchor = None;
        self.selection_popover = Some((start_word, end_word));
        // The tray's hint names its mode's actions (S5): the full tray on
        // index-identical views, the correction tray on re_anchored views.
        let mode = self
            .transcript
            .as_ref()
            .and_then(|state| selection_tray_mode(&state.alignment).ok());
        self.status = Some(match mode {
            Some(SelectionTrayMode::CorrectOnly) => {
                "Selection ready — this corrected view supports Suggest correction and \
                 Copy time. Enter corrects; Escape dismisses."
                    .into()
            }
            _ => "Selection ready — Highlight, Cut, Copy time, Suggest correction, or \
                  Include in reel. Delete cuts; Escape dismisses."
                .into(),
        });
        focus.focus(window, cx);
        cx.notify();
    }

    /// Dismiss the selection popover without acting (the tray's Escape).
    pub(super) fn dismiss_selection_popover(&mut self, cx: &mut Context<Self>) {
        if self.selection_popover.take().is_some() {
            self.status = Some("Selection dismissed.".into());
            cx.notify();
        }
    }

    /// S8: the right-click context menu's target range — the words the
    /// menu's actions apply to. An open selection popover owns its range;
    /// otherwise a pending two-click anchor completes to the right-click
    /// target word, and a bare right click over a word selects that word
    /// alone. A right click outside the words (no target, no anchor) has
    /// no range and opens nothing.
    pub(super) fn context_menu_range(&self) -> Option<(usize, usize)> {
        if let Some((start_word, end_word)) = self.selection_popover {
            return Some((start_word, end_word));
        }
        match (self.selection_anchor, self.context_menu_fallback) {
            (Some(anchor), Some(word)) => Some((anchor.min(word), anchor.max(word))),
            (Some(anchor), None) => Some((anchor, anchor)),
            (_, Some(word)) => Some((word, word)),
            _ => None,
        }
    }

    /// S8: a right mouse-up over the word surface opens the context menu
    /// for the range under the pointer — the pointer alternative to the
    /// select-then-tray gesture. The menu's actions reuse the tray's
    /// dispatchers (Highlight opens the label editor, Strike out
    /// dispatches the strike, Copy time writes the span, Include in reel
    /// flips or creates membership); the range is captured when the menu
    /// opens, so the actions never read a working selection that has
    /// since moved.
    pub(super) fn open_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The menu is a word-surface affordance — the events surface has
        // its own span authoring (tag/redact), not word highlights.
        if self.transcript_surface != TranscriptSurface::Words {
            self.status =
                Some("Right-click the word transcript to choose Highlight or Strike out.".into());
            cx.notify();
            return;
        }
        let Some((start_word, end_word)) = self.context_menu_range() else {
            self.status = Some("Right-click a word to choose Highlight or Strike out.".into());
            cx.notify();
            return;
        };
        // A right click completes a pending two-click selection — the
        // menu owns the completed range.
        self.selection_anchor = None;
        self.context_menu = Some((start_word, end_word));
        let focus = self
            .context_menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        focus.focus(window, cx);
        self.status = Some(format!(
            "Context menu — words {start_word}–{end_word}: Highlight or Strike out."
        ));
        cx.notify();
    }

    /// Dismiss the right-click context menu without acting (its Escape).
    pub(super) fn dismiss_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            self.status = Some("Context menu dismissed.".into());
            cx.notify();
        }
    }

    /// S8: consume the pending selection state the context menu acts on —
    /// an open tray or a two-click anchor — so the action's range is not
    /// left dangling after the menu fires.
    pub(super) fn clear_pending_selection(&mut self) {
        self.selection_popover = None;
        self.selection_anchor = None;
    }

    /// Whether the open view serves the correction tray (S5) — the
    /// index-identical-only actions refuse on such views.
    pub(super) fn selection_tray_is_correct_only(&self) -> bool {
        self.transcript.as_ref().is_some_and(|state| {
            selection_tray_mode(&state.alignment) == Ok(SelectionTrayMode::CorrectOnly)
        })
    }

    /// Route the selection popover's keys by the view's tray mode: Enter
    /// opens the low-effort editor (the label editor on the full tray,
    /// the correction editor on the correction tray), Delete cuts on the
    /// full tray only, Escape dismisses. Bound on the tray itself, so
    /// the keys only fire while it holds focus — the label input's
    /// Delete stays text editing (forward-delete), which is exactly why
    /// this binding is not on the input.
    pub(super) fn handle_selection_popover_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = self
            .transcript
            .as_ref()
            .and_then(|state| selection_tray_mode(&state.alignment).ok());
        match keystroke.key.as_str() {
            "escape" => self.dismiss_selection_popover(cx),
            "enter" => match mode {
                Some(SelectionTrayMode::CorrectOnly) => self.popover_correct(window, cx),
                _ => self.popover_highlight(window, cx),
            },
            // Delete cuts on the full tray only — the correction tray has
            // nothing to cut (cut needs an index-identical view).
            "delete" | "backspace" => {
                if mode != Some(SelectionTrayMode::CorrectOnly) {
                    self.popover_cut(cx);
                }
            }
            _ => {}
        }
    }

    /// The tray's Suggest correction action (S5): close the tray and open
    /// the correction editor — the label-editor pattern — over the same
    /// range, the replacement step. Available on every timed view:
    /// original/aligned selections are source-identical, re_anchored
    /// selections translate at dispatch through educt_locate.
    pub(super) fn popover_correct(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.open_label_editor(
            HighlightEdit {
                kind: HighlightEditKind::Correction {
                    start_word,
                    end_word,
                },
                buffer: LabelBuffer::default(),
            },
            window,
            cx,
        );
    }

    /// The tray's Highlight action: close the tray and open the label
    /// editor over the same range — the naming step. Index-identical
    /// views only: highlight anchors source words through the annotation
    /// tools without a translation, so a re_anchored view refuses (a
    /// stale tray or a mis-routed key must never anchor working
    /// indices — the 2026-10-25 overshoot trap).
    pub(super) fn popover_highlight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.open_highlight_editor(start_word, end_word, window, cx);
    }

    /// Open the label editor over a range to create a highlight — the
    /// naming step, shared by the selection popover's Highlight arm and
    /// the right-click context menu's Highlight entry. Index-identical
    /// views only (the popover_highlight guard's reason).
    pub(super) fn open_highlight_editor(
        &mut self,
        start_word: usize,
        end_word: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Highlight needs an index-identical view — Suggest correction instead.".into(),
            );
            cx.notify();
            return;
        }
        self.open_label_editor(
            HighlightEdit {
                kind: HighlightEditKind::Create {
                    start_word,
                    end_word,
                },
                buffer: LabelBuffer::default(),
            },
            window,
            cx,
        );
    }

    /// The tray's Cut action (the Delete key shares it): strike the range —
    /// a cut carries no label, so the tray ends here. Index-identical
    /// views only (the popover_highlight guard's reason).
    pub(super) fn popover_cut(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        if self.selection_tray_is_correct_only() {
            self.status =
                Some("Cut needs an index-identical view — Suggest correction instead.".into());
            cx.notify();
            return;
        }
        self.strike_selected_range(start_word, end_word, cx);
    }

    /// The tray's Copy time action: close the tray and copy the range's
    /// time span — the shared range step, also the context menu's Copy
    /// time entry.
    pub(super) fn popover_copy_time(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.copy_time_range(start_word, end_word, cx);
    }

    /// Copy a word range's time span to the clipboard in the panel's
    /// marks vocabulary ("in 2.0s out 5.9s" — the same readout the trim
    /// marks label uses, so one vocabulary serves the panel). A view
    /// without word timings degrades honestly instead of copying a
    /// made-up span.
    pub(super) fn copy_time_range(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        let span = self.transcript.as_ref().and_then(|state| {
            let words = state.words.as_deref()?;
            let first = words.get(start_word)?;
            let last = words.get(end_word)?;
            Some((first.start_ms, last.end_ms))
        });
        let Some((start_ms, end_ms)) = span else {
            self.status = Some("No word timings on this view — nothing to copy.".into());
            cx.notify();
            return;
        };
        let copied = format!(
            "in {:.1}s out {:.1}s",
            start_ms as f64 / 1000.0,
            end_ms as f64 / 1000.0
        );
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()));
        self.status = Some(format!("Copied {copied}."));
        cx.notify();
    }

    /// S6: the composer's re-match on every workspace load — the reload
    /// persistence mechanism. Existing clips keep their composer deltas
    /// (order, inclusion, in-reel strikes) keyed to highlight identity
    /// (annotation id + source range), with labels refreshed from the
    /// reloaded highlights; new researcher highlights join included at the
    /// tail (the fresh-workspace semantics); deleted highlights drop their
    /// clips; model-pass rows never compose.
    pub(super) fn rematch_reel_clips(&mut self, state: &TranscriptPaneState) {
        let mut clips: Vec<ReelClip> = self
            .reel_clips
            .drain(..)
            .filter(|clip| {
                state.highlights.iter().any(|highlight| {
                    highlight.researcher_supplied
                        && Self::reel_clip_identity_matches(clip, highlight)
                })
            })
            .collect();
        for clip in &mut clips {
            if let Some(highlight) = state.highlights.iter().find(|highlight| {
                highlight.researcher_supplied && Self::reel_clip_identity_matches(clip, highlight)
            }) {
                clip.label = highlight.label.clone();
            }
        }
        for highlight in state
            .highlights
            .iter()
            .filter(|highlight| highlight.researcher_supplied)
        {
            let exists = clips
                .iter()
                .any(|clip| Self::reel_clip_identity_matches(clip, highlight));
            if !exists {
                clips.push(ReelClip {
                    annotation_id: highlight.annotation_id.clone(),
                    start_word: highlight.start_word,
                    end_word: highlight.end_word,
                    label: highlight.label.clone(),
                    include: true,
                    strikes: Vec::new(),
                });
            }
        }
        self.reel_clips = clips;
    }

    /// The composer's identity key: a highlight is the same clip across
    /// reloads when its annotation id AND source range match (a rename
    /// changes neither).
    pub(super) fn reel_clip_identity_matches(clip: &ReelClip, highlight: &PaneHighlight) -> bool {
        clip.annotation_id == highlight.annotation_id
            && clip.start_word == highlight.start_word
            && clip.end_word == highlight.end_word
    }

    /// S6: move a clip within the composed order — the reel row's ↑/↓.
    /// Bounds-guarded: a no-op at the list's edges, never a panic.
    pub(super) fn move_reel_clip(&mut self, clip_index: usize, up: bool, cx: &mut Context<Self>) {
        if clip_index >= self.reel_clips.len() {
            return;
        }
        let target = if up {
            clip_index.checked_sub(1)
        } else {
            Some(clip_index + 1)
        };
        if let Some(target) = target
            && target < self.reel_clips.len()
        {
            self.reel_clips.swap(clip_index, target);
            cx.notify();
        }
    }

    /// S6: flip one clip's inclusion — the highlights row's toggle and the
    /// popover's Include arm both write here; the clip's include flag is
    /// the ONE membership source of truth (no set beside the list). Keyed
    /// to highlight identity, so the flip survives reloads and is safe on
    /// re_anchored views too (it flips stored source-anchored membership,
    /// never a working selection).
    pub(super) fn toggle_reel_clip_include(
        &mut self,
        annotation_id: &str,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(clip) = self.reel_clips.iter_mut().find(|clip| {
            clip.annotation_id == annotation_id
                && clip.start_word == start_word
                && clip.end_word == end_word
        }) {
            clip.include = !clip.include;
            cx.notify();
        }
    }

    /// S6: undo one in-reel strike on a clip (the reel row's per-strike
    /// Restore). Bounds-guarded.
    pub(super) fn remove_reel_clip_strike(
        &mut self,
        clip_index: usize,
        strike_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(clip) = self.reel_clips.get_mut(clip_index)
            && strike_index < clip.strikes.len()
        {
            clip.strikes.remove(strike_index);
            cx.notify();
        }
    }

    /// S6: the selection popover's Include-in-reel arm. Over a composed
    /// clip's range it flips that clip's include flag (the row toggle's
    /// path — one source of truth, no dispatch); over a fresh range it
    /// dispatches the existing create path with the default label — the
    /// reload re-matches every researcher highlight into the composer, so
    /// the one click carries the range into the reel (a clip with no
    /// highlight behind it would have no reload identity to survive on).
    pub(super) fn popover_toggle_reel_include(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        self.include_reel_range(start_word, end_word, cx);
    }

    /// Flip a range's reel membership — the shared range step, also the
    /// context menu's Include in reel entry. Over a composed clip's range
    /// it flips that clip's include flag (the row toggle's path — one
    /// source of truth, no dispatch); over a fresh range it dispatches
    /// the existing create path with the default label — the reload
    /// re-matches every researcher highlight into the composer, so the
    /// one click carries the range into the reel (a clip with no
    /// highlight behind it would have no reload identity to survive on).
    pub(super) fn include_reel_range(
        &mut self,
        start_word: usize,
        end_word: usize,
        cx: &mut Context<Self>,
    ) {
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Include in reel needs an index-identical view — Suggest correction \n                 instead."
                    .into(),
            );
            cx.notify();
            return;
        }
        match self
            .reel_clips
            .iter()
            .find(|clip| clip.start_word == start_word && clip.end_word == end_word)
        {
            Some(clip) => {
                let now_included = !clip.include;
                let (annotation_id, clip_start, clip_end) =
                    (clip.annotation_id.clone(), clip.start_word, clip.end_word);
                self.toggle_reel_clip_include(&annotation_id, clip_start, clip_end, cx);
                self.status = Some(
                    if now_included {
                        "Added to the reel."
                    } else {
                        "Removed from the reel."
                    }
                    .into(),
                );
                cx.notify();
            }
            None => {
                self.create_workspace_highlight(start_word, end_word, resolve_create_label(""), cx);
            }
        }
    }

    /// S6: the selection popover's in-reel strike arm — a sub-range cut
    /// inside ONE clip, recorded on that clip (the compose bakes it into
    /// the clip's Keep op; never a whole-recording strike). The completed
    /// range must lie within exactly one INCLUDED clip; anything else
    /// refuses honestly. Index-identical views only — the tray gate (the
    /// strike anchors working-view indices, which only match source tools
    /// on index-identical views).
    pub(super) fn popover_strike_in_reel(&mut self, cx: &mut Context<Self>) {
        let Some((start_word, end_word)) = self.selection_popover.take() else {
            return;
        };
        if self.selection_tray_is_correct_only() {
            self.status = Some(
                "Strike in reel clip needs an index-identical view — Suggest \n                 correction instead."
                    .into(),
            );
            cx.notify();
            return;
        }
        let inside: Vec<usize> = self
            .reel_clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| {
                clip.include && clip.start_word <= start_word && end_word <= clip.end_word
            })
            .map(|(index, _)| index)
            .collect();
        match inside.as_slice() {
            [clip_index] => {
                self.reel_clips[*clip_index]
                    .strikes
                    .push((start_word, end_word));
                self.status = Some("Struck the sub-range inside the reel clip.".into());
                cx.notify();
            }
            _ => {
                self.status = Some(
                    "Strike in reel clip needs a range inside exactly one included clip.".into(),
                );
                cx.notify();
            }
        }
    }

    // ── In-transcript search (S3) ───────────────────────────────────────

    /// S3: schedule the debounced search rescan — one burst of edits
    /// rescans once, 150 ms after the last keystroke. Taking the old task
    /// cancels any pending rescan (the follow-task lifecycle pattern); the
    /// timer is GPUI-native (never smol::Timer — .rules).
    pub(super) fn schedule_transcript_search_rescan(&mut self, cx: &mut Context<Self>) {
        self.transcript_search_task.take();
        self.transcript_search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(TRANSCRIPT_SEARCH_DEBOUNCE)
                .await;
            this.update(cx, |viewer, cx| {
                viewer.rescan_transcript_search(cx);
            })
            .ok();
        }));
    }

    /// S3: recompute the search matches from the typed query over the
    /// loaded transcript — the debounced rescan's landing. A fresh match
    /// list starts the cycle on its first match and lands it in view (the
    /// find-bar precedent: the stabilized query shows where the results
    /// are); zero matches clear the marks and surface beside the input,
    /// never silently. The query is trimmed and matched case-insensitively
    /// for ASCII, exactly for non-ASCII (the server's lexical rule).
    pub(super) fn rescan_transcript_search(&mut self, cx: &mut Context<Self>) {
        self.transcript_search_task.take();
        let words = self
            .transcript
            .as_ref()
            .and_then(|state| state.words.as_deref());
        let query = self.transcript_search.query.text();
        self.transcript_search.matches = transcript_search_matches(words.unwrap_or(&[]), &query);
        self.transcript_search.current = 0;
        if !self.transcript_search.matches.is_empty() {
            self.reveal_transcript_search_match();
        }
        cx.notify();
    }

    /// S3: scroll the current match's block into view — Enter/Shift-Enter's
    /// jump and the rescan's first-match landing, using the follow reveal's
    /// item-anchored idiom (blocks deep in a long recording are unmeasured
    /// until rendered, so the reveal anchors by item index, not pixels). The
    /// reveal accounting keeps the follow tick from misreading this scroll
    /// as manual.
    pub(super) fn reveal_transcript_search_match(&mut self) {
        let Some(&(start_word, _)) = self
            .transcript_search
            .matches
            .get(self.transcript_search.current)
        else {
            return;
        };
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(block_ix) = block_index_for_word(state, start_word) else {
            return;
        };
        let item_ix = WORKSPACE_HEADER_ITEMS + block_ix;
        let visible = self
            .transcript_list
            .bounds_for_item(item_ix)
            .is_some_and(|item| {
                let viewport = self.transcript_list.viewport_bounds();
                item.top() >= viewport.top() && item.bottom() <= viewport.bottom()
            });
        if !visible {
            self.follow_reveals_in_flight = self.follow_reveals_in_flight.saturating_add(1);
            self.transcript_list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        }
    }

    /// Route the search input's keystrokes (S3): Enter cycles to the next
    /// match and lands it (Shift-Enter cycles back; both wrap), Escape
    /// clears the query and its marks, editing keys go to the buffer and
    /// schedule the debounced rescan. Bound on the input element itself, so
    /// the keys only fire while it holds focus.
    pub(super) fn handle_transcript_search_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) {
        let count = self.transcript_search.matches.len();
        match keystroke.key.as_str() {
            "enter" if count > 0 => {
                self.transcript_search.current = if keystroke.modifiers.shift {
                    (self.transcript_search.current + count - 1) % count
                } else {
                    (self.transcript_search.current + 1) % count
                };
                self.reveal_transcript_search_match();
                cx.notify();
            }
            "escape" => {
                // Escape clears the query — the search leaves no marks
                // behind, immediately (no debounce on the way out).
                self.transcript_search = TranscriptSearch::default();
                self.transcript_search_task.take();
                cx.notify();
            }
            _ => {
                if self.transcript_search.query.apply_keystroke(keystroke) {
                    self.schedule_transcript_search_rescan(cx);
                    cx.notify();
                }
            }
        }
    }

    /// Paste clipboard text into the search query (S3) — the label
    /// editor's paste fix (2026-10-26): modifier chords never type, so a
    /// phrase could not be pasted otherwise. Control characters are
    /// dropped — the input is single-line. Inert with no clipboard text.
    pub(super) fn paste_into_transcript_search(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.transcript_search.query.insert_str(&text);
        self.schedule_transcript_search_rescan(cx);
        cx.notify();
    }

    /// S3: clear the transcript search with the surface — the query, the
    /// matches, the pending rescan, and the follow custody all belong to the
    /// workspace they were opened on (a reload's fresh working view makes
    /// the pending matches stale).
    pub(super) fn reset_transcript_search(&mut self) {
        self.transcript_search = TranscriptSearch::default();
        self.transcript_search_task.take();
        self.transcript_search_suspended_follow = false;
    }

    /// S3: register the search input's focus/blur listeners — focusing the
    /// search suspends viewport tracking (typing and the auto-landed matches
    /// must not fight the follow-along — the manual-scroll precedent), and
    /// blur restores tracking only when the search took it (a manually
    /// suspended follow stays suspended). Idempotent; called from the
    /// workspace render, so the listeners are live before any focus path
    /// can reach the input (click or keyboard). Render-time registration
    /// works (the deferred activation rides the normal effect cycle) —
    /// verified by bisect 2026-11-05 after an earlier false claim that it
    /// never activates; do not re-assert that.
    pub(super) fn ensure_transcript_search_focus_listeners(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.transcript_search_focus_subscriptions.is_empty() {
            return;
        }
        let Some(focus) = self.transcript_search_focus.clone() else {
            return;
        };
        let on_focus = cx.on_focus(&focus, window, |viewer, _window, cx| {
            if viewer.follow_viewport {
                viewer.follow_viewport = false;
                viewer.transcript_search_suspended_follow = true;
                cx.notify();
            }
        });
        let on_blur = cx.on_blur(&focus, window, |viewer, _window, cx| {
            if viewer.transcript_search_suspended_follow {
                viewer.transcript_search_suspended_follow = false;
                viewer.follow_viewport = true;
                cx.notify();
            }
        });
        self.transcript_search_focus_subscriptions = vec![on_focus, on_blur];
    }

    /// The label editor row (S4-3): the focused single-line input plus
    /// confirm/cancel affordances, rendered above the transcript text so it
    /// stays in view. An associated fn so the caller can hold the workspace
    /// state's immutable borrow.
    pub(super) fn render_label_editor(
        edit: &HighlightEdit,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (context, confirm_text, placeholder) = match &edit.kind {
            HighlightEditKind::Create { .. } => (
                "Name this highlight".to_string(),
                "Create",
                "Highlight name",
            ),
            HighlightEditKind::Correction { .. } => {
                ("Correct selection".to_string(), "Apply", "Corrected text")
            }
            HighlightEditKind::EventTag { .. } => {
                ("Name this event tag".to_string(), "Create tag", "Tag name")
            }
            HighlightEditKind::Rename { .. } => {
                ("Rename highlight".to_string(), "Save", "Highlight name")
            }
        };
        let typed = edit.buffer.text();
        let cursor = edit.buffer.cursor();
        let before: String = typed.chars().take(cursor).collect();
        let after: String = typed.chars().skip(cursor).collect();
        let mut input = div()
            .id("media-transcript-label-input")
            .track_focus(focus)
            .flex()
            .items_center()
            .gap_0()
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
                    this.paste_into_label_edit(cx);
                } else {
                    this.handle_label_key(&event.keystroke, cx);
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
                Label::new(SharedString::from(placeholder))
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
                        .id("media-transcript-label-caret")
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
        let editor = div()
            .id("media-transcript-label-editor")
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-label-editor".into())
            .child(
                Label::new(SharedString::from(context))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(input)
            .child(
                div()
                    .id("media-transcript-label-confirm")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(confirm_text))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_label_edit(cx);
                    })),
            );
        // R2: the create arm also offers Strike — cut the range without a
        // label (a rename has no strike affordance).
        let editor = match &edit.kind {
            HighlightEditKind::Create {
                start_word,
                end_word,
            } => {
                let (start_word, end_word) = (*start_word, *end_word);
                editor.child(
                    div()
                        .id("media-transcript-label-strike")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Cut selection"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.strike_selected_range(start_word, end_word, cx);
                        })),
                )
            }
            HighlightEditKind::Correction { .. }
            | HighlightEditKind::EventTag { .. }
            | HighlightEditKind::Rename { .. } => editor,
        };
        editor
            .child(
                div()
                    .id("media-transcript-label-cancel")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Cancel"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_label_edit(cx);
                    })),
            )
            .into_any_element()
    }

    /// The selection popover (S2): the completed range's action tray —
    /// Highlight / Cut / Copy time / Suggest correction / Include in reel
    /// on the full tray; Suggest correction + Copy time on the correction
    /// tray (re_anchored views, S5) — pinned above the list (the
    /// label-editor pin pattern) so the actions never virtualize away.
    /// The tray holds focus while open; its keys bind here by the same
    /// mode (Enter opens the mode's editor, Delete cuts on the full tray
    /// only, Escape dismisses), never on the label input (Delete there is
    /// text editing). An associated fn so the caller can hold the
    /// workspace state's immutable borrow.
    pub(super) fn render_selection_popover(
        excerpt: &str,
        include_label: &str,
        strike_in_clip: bool,
        correct_only: bool,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("media-transcript-selection-popover")
            .track_focus(focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_selection_popover_key(&event.keystroke, window, cx);
                cx.stop_propagation();
            }))
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-selection-popover".into())
            .child(
                Label::new(SharedString::from(format!("Selection — {excerpt}")))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .when(!correct_only, |tray| {
                tray.child(
                    div()
                        .id("media-transcript-popover-highlight")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Highlight"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.popover_highlight(window, cx);
                        })),
                )
                .child(
                    div()
                        .id("media-transcript-popover-cut")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("Cut"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Error),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.popover_cut(cx);
                        })),
                )
                .when(strike_in_clip, |tray| {
                    tray.child(
                        div()
                            .id("media-transcript-popover-strike-in-reel")
                            .cursor_pointer()
                            .hover(|el| el.bg(cx.theme().colors().element_hover))
                            .child(
                                Label::new(SharedString::from("Strike in reel clip"))
                                    .size(LabelSize::XSmall)
                                    .color(ui::Color::Error),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.popover_strike_in_reel(cx);
                            })),
                    )
                })
            })
            .child(
                div()
                    .id("media-transcript-popover-copy-time")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Copy time"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.popover_copy_time(cx);
                    })),
            )
            .child(
                div()
                    .id("media-transcript-popover-correct")
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Suggest correction"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.popover_correct(window, cx);
                    })),
            )
            .when(!correct_only, |tray| {
                tray.child(
                    div()
                        .id("media-transcript-popover-include")
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from(include_label))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.popover_toggle_reel_include(cx);
                        })),
                )
            })
            .into_any_element()
    }

    /// S8: the right-click context menu — the pointer alternative to the
    /// select-then-tray gesture. The entries reuse the tray's dispatchers
    /// over the captured range: Highlight opens the label editor, Strike
    /// out dispatches the strike, Copy time writes the span, Include in
    /// reel flips or creates membership. Escape dismisses.
    pub(super) fn render_context_menu(
        excerpt: &str,
        include_label: &str,
        focus: &gpui::FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id("media-transcript-context-menu")
            .track_focus(focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    this.dismiss_context_menu(cx);
                }
                cx.stop_propagation();
            }))
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .debug_selector(|| "media-transcript-context-menu".into())
            .child(
                Label::new(SharedString::from(format!("Right-click — {excerpt}")))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-highlight")
                    .debug_selector(|| "media-transcript-context-menu-highlight".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Highlight"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.open_highlight_editor(start_word, end_word, window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-strike")
                    .debug_selector(|| "media-transcript-context-menu-strike".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Strike out"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.strike_selected_range(start_word, end_word, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-copy-time")
                    .debug_selector(|| "media-transcript-context-menu-copy-time".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Copy time"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.copy_time_range(start_word, end_word, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("media-transcript-context-menu-include")
                    .debug_selector(|| "media-transcript-context-menu-include".into())
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(include_label))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some((start_word, end_word)) = this.context_menu {
                            this.clear_pending_selection();
                            this.include_reel_range(start_word, end_word, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    // ── S6 reel preview (the auto-seek chain on the shared player) ─────────

    /// S6: toggle the composed reel's preview — plays the composed spans on
    /// the shared player without rendering, seeking span-to-span as playback
    /// crosses each span's end. Player-local (no admission — the seek paths'
    /// class); the position source and the seek target are the S4
    /// rate-contract pair.
    pub(super) fn toggle_reel_preview(&mut self, cx: &mut Context<Self>) {
        if self.reel_preview.is_some() {
            self.stop_reel_preview(cx);
            self.status = Some("Reel preview stopped.".into());
            cx.notify();
            return;
        }
        self.start_reel_preview(cx);
    }

    /// S6: stop the preview and cancel its task — the one teardown entry
    /// every site shares (surface switch, open, close, reload, the toggle's
    /// off arm) and idempotent.
    pub(super) fn stop_reel_preview(&mut self, cx: &mut Context<Self>) {
        self.reel_preview_task.take();
        self.reel_preview = None;
        cx.notify();
    }

    /// S6: arm and start the preview — build the span plan (the render
    /// path's own clip plan over the SOURCE bundle words), prime the first
    /// span with an immediate seek, then poll the player on the follow
    /// loop's task pattern: single-instance guard, GPUI-native timer,
    /// self-cancel when the substrate leaves, poll only while playing.
    pub(super) fn start_reel_preview(&mut self, cx: &mut Context<Self>) {
        if self.reel_preview.is_some() {
            return;
        }
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let spans = match reel_preview_plan(state, &self.reel_clips) {
            Ok(spans) => spans,
            Err(reason) => {
                self.status = Some(reason.into());
                cx.notify();
                return;
            }
        };
        let first_start = spans[0].0;
        self.reel_preview = Some(ReelPreview { spans, index: 0 });
        self.seek_workspace_transcript(first_start, cx);
        self.status = Some("Previewing the composed reel — press play to hear it.".into());
        cx.notify();
        let entity = cx.entity().downgrade();
        self.reel_preview_task = Some(cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor().timer(FOLLOW_POLL_INTERVAL).await;
                let keep_going = entity
                    .update(cx, |viewer, cx| viewer.reel_preview_tick(cx))
                    .unwrap_or_default();
                if !keep_going {
                    return;
                }
            }
        }));
    }

    /// S6: one preview poll — the task body, directly callable (the
    /// teardown pins drive it). Returns false (state cleared) when the
    /// substrate is gone — the surface left, the transcript closed, the
    /// widget cleared (hidden media suspends); holds while playback is
    /// paused (the D3 poll-only-while-playing discipline); otherwise
    /// advances the walk. Position reads `transport_position_secs` — the
    /// S4 rate-conversion accessor, media seconds at every rate.
    pub(super) fn reel_preview_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.active_tab != ViewerTab::Workspace
            || self.transcript.is_none()
            || self.media_widget.is_none()
            || self.reel_preview.is_none()
        {
            self.stop_reel_preview(cx);
            return false;
        }
        let playing = self
            .media_widget
            .as_ref()
            .is_some_and(|widget| widget.read(cx).is_transport_playing());
        if !playing {
            return true;
        }
        let Some(position_ms) = self
            .media_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).transport_position_secs())
            .map(|secs| (secs * 1000.0) as u64)
        else {
            return true;
        };
        self.step_reel_preview(position_ms, cx)
    }

    /// S6: one walk step — advance past every span the position has crossed,
    /// seeking the newly-current span's start through the bound widget.
    /// Separated from the tick so the walk is drivable with a scripted
    /// position timeline (the playing gate lives in the tick).
    pub(super) fn step_reel_preview(&mut self, position_ms: u64, cx: &mut Context<Self>) -> bool {
        let step = self
            .reel_preview
            .as_mut()
            .map(|preview| preview.advance(position_ms));
        match step {
            Some(ReelPreviewStep::Hold) => true,
            Some(ReelPreviewStep::Seek(target_ms)) => {
                self.seek_workspace_transcript(target_ms, cx);
                true
            }
            Some(ReelPreviewStep::Finished) => {
                self.reel_preview = None;
                self.reel_preview_task.take();
                self.status = Some("Reel preview finished.".into());
                cx.notify();
                false
            }
            None => false,
        }
    }

    /// Seek the workspace's bound player to a working word's start — the
    /// click-to-seek entry (S3b-2); the interactive text's click listener
    /// resolves the word's start_ms through the pane's word map.
    pub(super) fn seek_workspace_transcript(&mut self, start_ms: u64, cx: &mut Context<Self>) {
        if let Some(widget) = &self.media_widget {
            widget.update(cx, |widget, cx| {
                widget.seek_to_secs(start_ms as f64 / 1000.0, cx)
            });
        }
    }

    /// Follow-along (S3c): poll the bound player's transport position on a
    /// GPUI-native timer and mark the word containing it, while the
    /// transcript sub-surface is visible (Workspace mode with a transcript
    /// open). Self-cancels when the mode leaves or the workspace closes;
    /// the handle drop cancels the spawned future (the same lifecycle as the
    /// queue poll).
    pub(super) fn start_transcript_follow(&mut self, cx: &mut Context<Self>) {
        if self.transcript_follow_task.is_some() {
            return;
        }
        let entity = cx.entity().downgrade();
        self.transcript_follow_task = Some(cx.spawn(async move |_this, cx| {
            loop {
                cx.background_executor().timer(FOLLOW_POLL_INTERVAL).await;
                let keep_going = match entity.update(cx, |viewer, cx| {
                    if viewer.active_tab != ViewerTab::Workspace || viewer.transcript.is_none() {
                        viewer.transcript_follow_task = None;
                        viewer.current_word = None;
                        cx.notify();
                        return false;
                    }
                    // D3: poll only while playing — a paused or stopped
                    // player keeps its frozen mark without the widget
                    // reads (2026-10-26 review: the loop previously polled
                    // regardless of the playing state). The scroll baseline
                    // still updates while paused, so a manual scroll during
                    // a pause suspends follow too (S3d).
                    let playing = viewer
                        .media_widget
                        .as_ref()
                        .is_some_and(|widget| widget.read(cx).is_transport_playing());
                    if !playing {
                        viewer.note_follow_scroll_tick(cx);
                        return true;
                    }
                    let position = viewer
                        .media_widget
                        .as_ref()
                        .and_then(|widget| widget.read(cx).transport_position_secs());
                    let words = viewer
                        .transcript
                        .as_ref()
                        .and_then(|state| state.words.as_deref());
                    let next = position
                        .and_then(|secs| {
                            words.map(|words| current_word_index(words, (secs * 1000.0) as u64))
                        })
                        .flatten();
                    if next != viewer.current_word {
                        viewer.current_word = next;
                        cx.notify();
                    }
                    // S3d: keep the marked block visible — a manual scroll
                    // (any offset change not from a follow reveal)
                    // suspends tracking; the Follow toggle re-enables it.
                    viewer.note_follow_scroll_tick(cx);
                    viewer.follow_marked_block_into_view();
                    true
                }) {
                    Ok(keep_going) => keep_going,
                    Err(_) => break,
                };
                if !keep_going {
                    break;
                }
            }
        }));
    }

    /// Observe the workspace list's scroll position once per follow tick.
    /// Any movement not accounted for by a pending follow reveal is a
    /// manual scroll: tracking suspends until the operator re-enables it
    /// through the Follow toggle (S3d).
    pub(super) fn note_follow_scroll_tick(&mut self, cx: &mut Context<Self>) {
        let top = self.transcript_list.logical_scroll_top();
        if top != self.follow_scroll_baseline {
            if self.follow_reveals_in_flight > 0 {
                self.follow_reveals_in_flight -= 1;
            } else if self.follow_viewport {
                self.follow_viewport = false;
                cx.notify();
            }
            self.follow_scroll_baseline = top;
        }
    }

    /// Keep the follow-along mark visible (S3d): scroll the marked block
    /// into view when tracking is enabled and the block is not already
    /// fully within the list's viewport. Pure list-state work — callable
    /// from the follow task without a window.
    pub(super) fn follow_marked_block_into_view(&mut self) {
        // The reveal targets word blocks (S3d); on the events surface the
        // body rows are events, so the word mark must not drive scrolling.
        if !self.follow_viewport || self.transcript_surface != TranscriptSurface::Words {
            return;
        }
        let Some(word) = self.current_word else {
            return;
        };
        let Some(state) = self.transcript.as_ref() else {
            return;
        };
        let Some(block_ix) = block_index_for_word(state, word) else {
            return;
        };
        let item_ix = WORKSPACE_HEADER_ITEMS + block_ix;
        let visible = self
            .transcript_list
            .bounds_for_item(item_ix)
            .is_some_and(|item| {
                let viewport = self.transcript_list.viewport_bounds();
                item.top() >= viewport.top() && item.bottom() <= viewport.bottom()
            });
        if !visible {
            self.follow_reveals_in_flight = self.follow_reveals_in_flight.saturating_add(1);
            // Anchor by item index, not by measured pixels: blocks deep in
            // a long recording are unmeasured until rendered, so a
            // pixel-based reveal would compute against zero heights and
            // scroll nowhere. The item-anchored scroll lands the marked
            // block at the viewport top (the chat list's scroll-to-item
            // precedent).
            self.transcript_list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// The Follow toggle (S3d): re-enable viewport tracking (revealing the
    /// current mark) or suspend it. A manual scroll suspends tracking; this
    /// is the operator's way back.
    pub(super) fn toggle_follow_viewport(&mut self, cx: &mut Context<Self>) {
        self.follow_viewport = !self.follow_viewport;
        if self.follow_viewport
            && let Some(word) = self.current_word
            && let Some(state) = self.transcript.as_ref()
            && let Some(block_ix) = block_index_for_word(state, word)
        {
            self.follow_reveals_in_flight = self.follow_reveals_in_flight.saturating_add(1);
            self.transcript_list.scroll_to(ListOffset {
                item_ix: WORKSPACE_HEADER_ITEMS + block_ix,
                offset_in_item: px(0.),
            });
        }
        self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        cx.notify();
    }

    /// The gallery recording a workspace transcript belongs to (R3): the
    /// linked asset from the panel's current gallery page. Recordings on
    /// unlisted pages fall to the unlinked path — the honest degradation.
    pub(super) fn recording_asset<'a>(
        &'a self,
        state: &'a TranscriptPaneState,
    ) -> Option<&'a MediaAsset> {
        let asset_id = state.gallery_asset_id.as_deref()?;
        self.assets
            .iter()
            .find(|asset| asset.gallery_asset_id.as_deref() == Some(asset_id))
    }

    /// The workspace's media body (R3): the RECORDING's asset body when the
    /// transcript links to a live gallery recording — the same body the
    /// workspace player renders, so a video-keyed transcript plays the video. An
    /// unlinked transcript (its recording left the gallery index) falls
    /// to its stored media_path — the transcript and its durable exports
    /// stay reachable.
    pub(super) fn workspace_media_body(&self, state: &TranscriptPaneState) -> String {
        match self.recording_asset(state) {
            Some(asset) => asset.body.clone(),
            None => transcript_media_body(&state.media_path),
        }
    }

    /// The transcript sub-surface of the asset workspace (T3, S3a): the
    /// open transcript's working view rendered as paragraph blocks with
    /// speaker labels and a layers strip, INSIDE the workspace — the
    /// reading list replaces the plain player area while the header, the
    /// edit toolbar, and the inspector pane stay the recording's (the
    /// ruling: you select an asset and stay with it). The player inside
    /// the list's header rows is the recording's shared player (S3b: the
    /// same viz-core-registry widget the workspace and inline renders use
    /// — one player per media identity, rebound through the R3 body,
    /// suspended on leave; D1). Alignment degradations surface as visible
    /// status notes, never silence (D5); click-to-seek and follow-along
    /// land with the S3b hit-testing element. An unlinked transcript
    /// renders here too — reachable, with its own degradation copy,
    /// never silently hidden.
    pub(super) fn render_transcript_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // The search input's focus handle (S3) — created on first need, and
        // its focus/blur listeners registered with it on the same first
        // render (`ensure_transcript_search_focus_listeners` — idempotent).
        // Registering at render (not from the input's click handler) keeps
        // keyboard-driven focus covered: an arrow/tab arrival at the input
        // suspends follow exactly like a click. Placed before the state
        // borrow (the registration takes &mut self). The header's search
        // row reads the stored handle.
        if self.transcript_search_focus.is_none() {
            self.transcript_search_focus = Some(cx.focus_handle());
        }
        self.ensure_transcript_search_focus_listeners(window, cx);
        let Some(state) = self.transcript.as_ref() else {
            // The caller (render_workspace) opens the sub-surface only
            // with a transcript loaded; keep the function total anyway.
            return div().into_any_element();
        };
        // The workspace media body is the RECORDING's (R3) — the header's
        // name and hint derive from the same linkage, all read before the
        // widget binding mutates the viewer.
        let body = self.workspace_media_body(state);
        let recording = self.recording_asset(state).cloned();
        let recording_name = match recording.as_ref() {
            Some(asset) => recording_display_name(&asset.src),
            None => recording_display_name(&state.media_path),
        };
        let tool_label = recording.as_ref().map(|asset| asset.tool.to_string());
        let unlinked = recording.is_none();
        if self.media_widget_body.as_deref() != Some(body.as_str()) {
            match hkask_viz_core::shared_media_widget(&body, window, cx) {
                Some(widget) => {
                    self.media_widget = Some(widget);
                    self.media_widget_body = Some(body);
                }
                None => {
                    // Degradation surfaced, never silent: the transcript text
                    // still renders without the player.
                    self.media_widget = None;
                    self.media_widget_body = None;
                }
            }
        }
        let widget = self.media_widget.clone();
        // The sub-surface's header: the close affordance returns to the
        // plain asset workspace; the recording's name flows from the
        // gallery asset (R3) and falls to the stored media_path when
        // unlinked — the hint names what the player follows. The inspector
        // toggle keeps the pane reachable while reading (T3).
        let close = div()
            .id("media-transcript-close")
            .debug_selector(|| "media-transcript-close".into())
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("← Close transcript"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_transcript_workspace(cx);
            }));
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .min_w_0()
            .overflow_hidden()
            .bg(cx.theme().colors().element_active)
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(close)
            .child(
                Label::new(SharedString::from(recording_name))
                    .size(LabelSize::Small)
                    .color(ui::Color::Default)
                    .flex_1()
                    .truncate(),
            )
            .when(unlinked, |el| {
                el.child(
                    Label::new(SharedString::from(
                        "unlinked recording — the player follows the stored media path",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Hint)
                    .truncate(),
                )
            })
            .when_some(tool_label, |el, tool| {
                el.child(
                    Label::new(SharedString::from(tool))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Hint)
                        .truncate(),
                )
            })
            .child(self.workspace_inspector_toggle(cx));
        // The edit toolbar serves the same shared player while the
        // recording is the workspace's selected asset (open_transcript
        // selects it, T3); an unlinked transcript has no gallery asset for
        // the trim/concat actions — honest absence, not a dead toolbar.
        let recording_selected = recording.as_ref().is_some_and(|asset| {
            self.selected
                .and_then(|ix| self.assets.get(ix))
                .is_some_and(|selected| selected.gallery_asset_id == asset.gallery_asset_id)
        });
        let toolbar = match (recording_selected, recording.as_ref(), widget.as_ref()) {
            (true, Some(asset), Some(widget)) => Some(self.render_edit_toolbar(widget, asset, cx)),
            _ => None,
        };
        // The label input's focus handle, created on first need (pure tests
        // construct the viewer without a GPUI context).
        let label_input_focus = self
            .label_input_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The selection popover's focus handle — the same lazy idiom.
        let selection_popover_focus = self
            .selection_popover_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The right-click context menu's focus handle — the same lazy idiom.
        let context_menu_focus = self
            .context_menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        // The open label editor (S4-3) stays pinned above the list —
        // always in view while a create or rename is pending.
        let pending_editor = self.highlight_edit.clone();
        // The open selection popover (S2) pins beside it — the completed
        // range's action tray. The render carries the excerpt and the
        // include toggle's CURRENT label ("Include in reel" vs "Remove
        // from reel" over an included highlight's exact range); the
        // actions read the range from the tray state, not from the render.
        let pending_popover = self.selection_popover.map(|(start_word, end_word)| {
            let excerpt = transcript_range_excerpt(state, start_word, end_word);
            let include_label = if self.reel_clips.iter().any(|clip| {
                clip.include && clip.start_word == start_word && clip.end_word == end_word
            }) {
                "Remove from reel"
            } else {
                "Include in reel"
            };
            // The in-reel strike arm (S6) needs the completed range inside
            // exactly one INCLUDED clip.
            let strike_in_clip = self
                .reel_clips
                .iter()
                .filter(|clip| {
                    clip.include && clip.start_word <= start_word && end_word <= clip.end_word
                })
                .count()
                == 1;
            // The tray's action set follows the view's mode (S5): the
            // correction-only set on re_anchored views.
            let correct_only =
                selection_tray_mode(&state.alignment) == Ok(SelectionTrayMode::CorrectOnly);
            (excerpt, include_label, strike_in_clip, correct_only)
        });
        // The whole sub-surface content (header rows, surface body rows,
        // annotation sections) renders as ONE virtualized list (S3d):
        // everything scrolls together exactly as the workspace always
        // did. On the word surface, timed blocks are interactive text —
        // every word is a click range that seeks the bound player to its
        // start (S3b-2), the word containing the playback position carries
        // the follow-along mark (S3c), and the follow task keeps the marked
        // block in view through the list state. On the events surface
        // (S5) the body rows are the visual-transcript events — click an
        // event to seek to its start, the same seek discipline — and the
        // tail is the authoring section.
        let blocks = Arc::new(build_pane_blocks(state));
        let events = Arc::new(
            self.visual_events
                .as_ref()
                .map(|state| state.events.clone())
                .unwrap_or_default(),
        );
        let item_count = match self.transcript_surface {
            TranscriptSurface::Words => {
                WORKSPACE_HEADER_ITEMS + blocks.len() + WORKSPACE_LIST_TAIL_ITEMS
            }
            TranscriptSurface::Events => {
                WORKSPACE_HEADER_ITEMS
                    + events.len()
                    + WORKSPACE_EVENTS_TAIL_ITEMS
                    + usize::from(self.visual_events_truncated())
            }
        };
        if self.transcript_list_count != item_count {
            self.transcript_list.reset(item_count);
            self.transcript_list_count = item_count;
            self.follow_reveals_in_flight = 0;
            self.follow_scroll_baseline = self.transcript_list.logical_scroll_top();
        }
        let list_state = self.transcript_list.clone();
        let workspace_items = cx.processor(move |this, ix, _window, cx| {
            this.render_workspace_list_item(ix, blocks.as_slice(), events.as_slice(), cx)
        });
        let workspace_list = gpui::list(list_state, workspace_items)
            .flex_1()
            .min_h_0()
            .p_3();
        let mut workspace = v_flex()
            .id("media-viewer-transcript")
            .flex_1()
            .min_h_0()
            // S2: press-and-release drag selection over the word surface.
            // The surface root spans every block plus the toolbar, so a
            // press anywhere in the surface resets or arms the pending
            // drag from the exact word hover (None outside words cancels a
            // stale drag — including one left mid-drag on the toolbar),
            // and a release anywhere in the surface completes it:
            // cross-word releases open the label editor on the union
            // range, same-word releases stay plain clicks
            // (InteractiveText seeks through its own click).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    this.word_drag_anchor = if this.highlight_edit.is_some() {
                        None
                    } else {
                        this.drag_last_word
                    };
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| {
                    this.word_drag_released(window, cx);
                }),
            )
            // S8: right-click opens the context menu — the pointer
            // alternative to the select-then-tray gesture. The press
            // remembers the hovered word as the menu's fallback target;
            // the release resolves the range (an open tray or pending
            // anchor wins over the hovered word) and opens the menu.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _event, _window, cx| {
                    this.context_menu_fallback = this.drag_last_word;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _event, window, cx| {
                    this.open_context_menu(window, cx);
                }),
            );
        // The pinned interaction band (the composed two-pane fix,
        // 2026-11-07): the player, the surface/mode chips, and the search
        // input pin ABOVE the list — master-detail inside the
        // sub-surface: the player is the persistent master object; the
        // transcript is the navigated detail. The follow-along's deep
        // reveals scroll the text BENEATH the band instead of displacing
        // the player out of the pane (the operator's "pushes the video
        // from the view"), and the search input the operator reaches for
        // mid-reading never scrolls away. THE HEIGHT BUDGET (the one-list
        // design's rejected concern, now answered by composition): the
        // band caps at 55% of the workspace, and the player row is the
        // flexible member — the reading area keeps at least 45% at every
        // pane height, the chips/search rows are irreducible, and below
        // them the player row shrinks first (the widget's own pins keep
        // the transport visible at every height) with overflow_hidden as
        // the degenerate-clip backstop (the S1 discipline: visible
        // truncation, never corruption of a neighboring surface).
        // Pinned by follow_scroll_keeps_the_pinned_player_in_view.
        let player_row = match widget.clone() {
            Some(widget) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .child(widget)
                .into_any_element(),
            // Degradation surfaced, never silent: the transcript text
            // still renders without the player.
            None => div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    Label::new(SharedString::from(
                        "Playback unavailable — the transcript and editing controls remain available.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error),
                )
                .into_any_element(),
        };
        let chips_row = self.render_transcript_chips_row(cx);
        let search_row = self.render_transcript_search_row(cx);
        let words_surface = self.transcript_surface == TranscriptSurface::Words;
        workspace = workspace.child(
            v_flex()
                .id("media-transcript-band")
                .debug_selector(|| "media-transcript-band".into())
                .max_h(gpui::relative(0.55))
                .overflow_hidden()
                .px_3()
                .pt_2()
                .gap_1()
                .child(player_row)
                .child(chips_row)
                .when(words_surface, |el| el.child(search_row)),
        );
        if let Some(edit) = pending_editor {
            workspace = workspace.child(v_flex().px_3().pt_3().child(Self::render_label_editor(
                &edit,
                &label_input_focus,
                cx,
            )));
        }
        if let Some((excerpt, include_label, strike_in_clip, correct_only)) = pending_popover {
            workspace =
                workspace.child(v_flex().px_3().pt_3().child(Self::render_selection_popover(
                    &excerpt,
                    include_label,
                    strike_in_clip,
                    correct_only,
                    &selection_popover_focus,
                    cx,
                )));
        }
        // S8: the right-click context menu — pinned above the list like
        // the selection popover, over the range captured when it opened.
        if let Some((start_word, end_word)) = self.context_menu {
            let excerpt = transcript_range_excerpt(state, start_word, end_word);
            let include_label = if self.reel_clips.iter().any(|clip| {
                clip.include && clip.start_word == start_word && clip.end_word == end_word
            }) {
                "Remove from reel"
            } else {
                "Include in reel"
            };
            workspace = workspace.child(v_flex().px_3().pt_3().child(Self::render_context_menu(
                &excerpt,
                include_label,
                &context_menu_focus,
                cx,
            )));
        }
        workspace = workspace.child(workspace_list);
        let inspector = self.inspector_open.then(|| self.render_inspector(cx));
        v_flex()
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "The recording's transcript in its asset workspace — select text to highlight and cut, compose the reel; the player stays bound to the recording.",
            ))
            .child(header)
            .when_some(toolbar, |el, toolbar| el.child(toolbar))
            .child(workspace)
            .when_some(inspector, |el, pane| el.child(pane))
            .into_any_element()
    }

    /// One workspace list item (S3d): a fixed header row, one body row of
    /// the active surface (a word block or a visual-transcript event), or
    /// one of the fixed tail sections — highlights, cuts, reel actions on
    /// the word surface; the visual-transcript authoring section on the
    /// events surface.
    pub(super) fn render_workspace_list_item(
        &self,
        ix: usize,
        blocks: &[PaneBlock],
        events: &[PaneEventRow],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if ix < WORKSPACE_HEADER_ITEMS {
            return self.render_workspace_header_item(ix, cx);
        }
        let body_ix = ix - WORKSPACE_HEADER_ITEMS;
        match self.transcript_surface {
            TranscriptSurface::Words => {
                if let Some(block) = blocks.get(body_ix) {
                    return div()
                        .pb_2()
                        .child(self.render_workspace_block(block, body_ix, cx))
                        .into_any_element();
                }
                match body_ix - blocks.len() {
                    0 => self.render_workspace_highlights_section(cx),
                    1 => self.render_workspace_cuts_section(cx),
                    _ => self.render_workspace_reel_section(cx),
                }
            }
            TranscriptSurface::Events => {
                if let Some(row) = events.get(body_ix) {
                    return div()
                        .pb_1()
                        .child(self.render_workspace_event_row(row, body_ix, cx))
                        .into_any_element();
                }
                // A page shorter than the recording's total events carries
                // the truncation note as the list's bottom row — the count
                // the operator reads never silently claims completeness.
                let tail_ix = body_ix - events.len();
                if tail_ix == 0 && self.visual_events_truncated() {
                    return self.render_workspace_events_truncation_note();
                }
                self.render_workspace_redaction_section(cx)
            }
        }
    }

    /// The workspace list's fixed header items (the composed two-pane fix,
    /// 2026-11-07): the status row (the events surface's load state, or the
    /// words surface's alignment note) and the surface's section label —
    /// read-once status copy, content-like, scrolling with the text. The
    /// player, the chips, and the search input live in the PINNED
    /// interaction band above the list (render_transcript_workspace) —
    /// never virtualized away with the navigated detail.
    pub(super) fn render_workspace_header_item(
        &self,
        ix: usize,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = match ix {
            0 => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                // On the events surface this row carries the event log's
                // own load status — the named no_layers degradation and
                // load failures surface here, never silently (design §8).
                if self.transcript_surface == TranscriptSurface::Events {
                    return match &self.visual_events_request.state {
                        ResourceState::Loading => {
                            Label::new(SharedString::from("Loading visual transcript…"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Muted)
                                .into_any_element()
                        }
                        ResourceState::Failed(error) => Label::new(SharedString::from(format!(
                            "Visual transcript unavailable — {error}"
                        )))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Error)
                        .into_any_element(),
                        _ => match self.visual_events.as_ref() {
                            Some(events) if events.status == "no_layers" => Label::new(
                                SharedString::from(events.note.clone().unwrap_or_else(|| {
                                    "No stored layers for this recording — run a layer pass \
                                     (layers_shot_pass, layers_semantic_pass, layers_audio_pass) \
                                     or author one (layers_store)."
                                        .to_string()
                                })),
                            )
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .into_any_element(),
                            _ => div().into_any_element(),
                        },
                    };
                }
                match state.alignment.as_str() {
                    "re_anchored" => Label::new(SharedString::from(
                        "Estimated timings — review existing ranges before creating a reel.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .into_any_element(),
                    "unaligned" => Label::new(SharedString::from(
                        "Corrected text only — playback and timed selection are unavailable.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error)
                    .into_any_element(),
                    "untimed" => Label::new(SharedString::from(
                        "Text-only transcript — playback and timed selection are unavailable.",
                    ))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error)
                    .into_any_element(),
                    _ => div().into_any_element(),
                }
            }
            _ => {
                let Some(state) = self.transcript.as_ref() else {
                    return div().into_any_element();
                };
                // The section label names the active reading surface: the
                // word pane's review note, or the event log's counts and
                // kinds.
                if self.transcript_surface == TranscriptSurface::Events {
                    let mut row = v_flex();
                    row = row.child(Self::section_label("Visual transcript"));
                    if let Some(events) = self.visual_events.as_ref()
                        && events.status == "completed"
                    {
                        let kinds = events.kinds_present.join(", ");
                        row = row.child(
                            Label::new(SharedString::from(format!(
                                "{} events across: {}",
                                events.events.len(),
                                if kinds.is_empty() { "no kinds" } else { &kinds }
                            )))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .mt_1(),
                        );
                    }
                    return div().pb_2().child(row).into_any_element();
                }
                let mut row = v_flex();
                row = row.child(Self::section_label("Transcript"));
                // Review flags (task 1a): the derived marker rides the
                // payload; the pane renders it, never re-derives (D5).
                let review = state
                    .review
                    .as_ref()
                    .filter(|_| matches!(state.alignment.as_str(), "original" | "aligned"));
                if let Some(note) = review.and_then(|review| review.note.as_deref()) {
                    row = row.child(
                        Label::new(SharedString::from(format!("Review: {note}")))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Muted)
                            .mt_1(),
                    );
                }
                row.into_any_element()
            }
        };
        div().pb_2().child(row).into_any_element()
    }

    /// The surface and mode chips (the pinned interaction band): the
    /// Words/Events reading-surface toggle, the Select-text mode toggle,
    /// and the Follow toggle (words surface only). Previously list item
    /// 1 of the one-list design; pinned with the player and the search
    /// input so mid-reading affordances never scroll away with the text
    /// (the composed two-pane fix, 2026-11-07).
    pub(super) fn render_transcript_chips_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let selection_mode = self.selection_mode;
        let following = self.follow_viewport;
        let surface = self.transcript_surface;
        // The reading-surface toggle (operator refinement
        // 2026-10-30): word-transcript pane ⇄ visual-transcript
        // event log — parallel annotations over the same recording;
        // the active surface renders Default, the other Muted.
        let words_chip = div()
            .id("media-transcript-surface-words")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Words"))
                    .size(LabelSize::XSmall)
                    .color(if surface == TranscriptSurface::Words {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_transcript_surface(TranscriptSurface::Words, cx);
            }));
        let events_chip = div()
            .id("media-transcript-surface-events")
            .debug_selector(|| "media-transcript-surface-events".into())
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Events"))
                    .size(LabelSize::XSmall)
                    .color(if surface == TranscriptSurface::Events {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_transcript_surface(TranscriptSurface::Events, cx);
            }));
        // S4 part-2: the select-range mode toggle — an explicit mode
        // beats every-click-seek plus accidental creation. The same
        // mode serves both surfaces: word ranges on the word surface,
        // event spans on the events surface.
        let select_chip = div()
            .id("media-transcript-select-mode")
            .debug_selector(|| "media-transcript-select-mode".into())
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(if selection_mode {
                    "Finish selection"
                } else if surface == TranscriptSurface::Events {
                    "Select span"
                } else {
                    "Select text"
                }))
                .size(LabelSize::XSmall)
                .color(if selection_mode {
                    ui::Color::Default
                } else {
                    ui::Color::Muted
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selection_mode = !this.selection_mode;
                this.selection_anchor = None;
                // Toggling the mode abandons any open label editor —
                // it is the tail of a selection this toggle ends —
                // and any pending event span (the same rule on the
                // events surface).
                this.highlight_edit = None;
                this.event_selection_anchor = None;
                this.event_selection_span = None;
                this.status = Some(if this.selection_mode {
                    if surface == TranscriptSurface::Events {
                        "Select mode on — click the first event of the span.".into()
                    } else {
                        "Select mode on — click the first word of the range.".into()
                    }
                } else {
                    "Select mode off — clicks seek playback.".into()
                });
                cx.notify();
            }));
        let mut chips = h_flex().items_center().flex_wrap().gap_2();
        chips = chips
            .child(words_chip)
            .child(events_chip)
            .child(select_chip);
        // The Follow toggle (S3d): viewport tracking is on by
        // default; a manual scroll suspends it and this affordance
        // re-enables it. A word-surface affordance — the mark and
        // reveal target word blocks.
        if surface == TranscriptSurface::Words {
            let follow_chip = div()
                .id("media-transcript-follow-toggle")
                .debug_selector(|| "media-transcript-follow-toggle".into())
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(if following {
                        "Following"
                    } else {
                        "Follow"
                    }))
                    .size(LabelSize::XSmall)
                    .color(if following {
                        ui::Color::Muted
                    } else {
                        ui::Color::Default
                    }),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_follow_viewport(cx);
                }));
            chips = chips.child(follow_chip);
        }
        chips.into_any_element()
    }

    /// The in-transcript search row (S3): the lexical input with its live
    /// count, pinned in the interaction band on the words surface only
    /// (the events log has its own reading model). Previously list item 2
    /// of the one-list design, which kept a placeholder row on the events
    /// surface for reading-position stability — pinning removes that
    /// hack: the row is never a list item, and the operator reaches for
    /// the search mid-reading, where the one-list design had already
    /// scrolled it away.
    pub(super) fn render_transcript_search_row(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(focus) = self.transcript_search_focus.clone() {
            let search = &self.transcript_search;
            let typed = search.query.text();
            let cursor = search.query.cursor();
            let before: String = typed.chars().take(cursor).collect();
            let after: String = typed.chars().skip(cursor).collect();
            let mut input = div()
                .id("media-transcript-search-input")
                .track_focus(&focus)
                .flex()
                .items_center()
                .min_w_0()
                .w(px(160.))
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border)
                .px_1()
                .debug_selector(|| "media-transcript-search-input".into())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    let keystroke = &event.keystroke;
                    if (keystroke.modifiers.control || keystroke.modifiers.platform)
                        && keystroke.key == "v"
                    {
                        this.paste_into_transcript_search(cx);
                    } else {
                        this.handle_transcript_search_key(keystroke, cx);
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
                    Label::new(SharedString::from("Search transcript"))
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
                            .id("media-transcript-search-caret")
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
            let has_words = self
                .transcript
                .as_ref()
                .is_some_and(|state| state.words.is_some());
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(input)
                .when_some(
                    transcript_search_status(&self.transcript_search, has_words),
                    |row, status| {
                        row.child(
                            div()
                                .id("media-transcript-search-status")
                                .debug_selector(|| "media-transcript-search-status".into())
                                .child(
                                    Label::new(SharedString::from(status))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Muted),
                                ),
                        )
                    },
                )
                .into_any_element()
        } else {
            // Unreachable in practice — the workspace render creates
            // the handle before the list renders — but a render path
            // never unwraps.
            div().into_any_element()
        }
    }

    /// One transcript block as a workspace list item (S3a/S3b/S3c):
    /// speaker label, per-block review marker, interactive text with word
    /// click-to-seek ranges, highlight and strike overlays, and the
    /// follow-along mark. The marked block carries the layout-test
    /// selector (S3d).
    pub(super) fn render_workspace_block(
        &self,
        block: &PaneBlock,
        block_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        let current_word = self.current_word;
        let selection_anchor = self.selection_anchor;
        let highlight_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_active),
            ..Default::default()
        };
        // R2: struck words render line-through.
        let strike_style = HighlightStyle {
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(cx.theme().colors().text),
            }),
            ..Default::default()
        };
        let review = state
            .review
            .as_ref()
            .filter(|_| matches!(state.alignment.as_str(), "original" | "aligned"));
        let per_block_no_confidence = review.is_some_and(|review| review.note.is_none());
        let mut section = v_flex();
        if let Some(speaker) = &block.speaker {
            section = section.child(
                Label::new(SharedString::from(speaker.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted)
                    .mt_1(),
            );
        }
        if let Some(flag) = review.and_then(|review| review.blocks.get(block_index)) {
            let marker = match flag.status {
                PaneReviewStatus::Flagged => Some("Review suggested".to_string()),
                PaneReviewStatus::NoConfidenceRecorded if per_block_no_confidence => {
                    Some("Review unavailable".to_string())
                }
                _ => None,
            };
            if let Some(marker) = marker {
                section = section.child(
                    Label::new(SharedString::from(marker))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted)
                        .mt_1(),
                );
            }
        }
        if block.word_char_ranges.is_empty() {
            // Text-only view: no word map, no interaction — a wrapping
            // StyledText (a single-line Label would overflow the same
            // way the interactive blocks did).
            return section
                .child(div().min_w_0().child(StyledText::new(block.text.clone())))
                .into_any_element();
        }
        let mut highlights = Vec::new();
        if let Some(current) = current_word
            && let Some(local) = block
                .word_starts
                .iter()
                .position(|(index, _)| *index == current)
        {
            highlights.push((block.word_char_ranges[local].clone(), highlight_style));
        }
        // The pending selection's anchor carries the same mark.
        if let Some(anchor) = selection_anchor
            && let Some(local) = block
                .word_starts
                .iter()
                .position(|(index, _)| *index == anchor)
        {
            highlights.push((block.word_char_ranges[local].clone(), highlight_style));
        }
        // The in-progress drag's anchor and release-target words carry
        // the same mark (S2): live feedback for the press-and-release
        // selection — the target mark vanishes over word gaps, showing
        // why a release there selects nothing.
        let drag_anchor = self.word_drag_anchor;
        let drag_target = self.drag_last_word.filter(|_| drag_anchor.is_some());
        for drag_mark in [drag_anchor, drag_target] {
            if let Some(word) = drag_mark
                && let Some(local) = block
                    .word_starts
                    .iter()
                    .position(|(index, _)| *index == word)
            {
                highlights.push((block.word_char_ranges[local].clone(), highlight_style));
            }
        }
        // Highlight overlays (S4): researcher/model ranges painted in
        // deterministic label colors — only on index-identical views
        // (D5: the panel never re-implements the server's projection).
        // Strike marks (R2): struck words render line-through under the
        // same gate.
        if matches!(state.alignment.as_str(), "original" | "aligned") {
            for highlight in &state.highlights {
                if let Some(range) =
                    block_word_char_range(block, highlight.start_word, highlight.end_word)
                {
                    highlights.push((
                        range,
                        HighlightStyle {
                            background_color: Some(highlight_label_color(&highlight.label)),
                            ..Default::default()
                        },
                    ));
                }
            }
            for strike in &state.strikes {
                if let Some(range) =
                    block_word_char_range(block, strike.start_word, strike.end_word)
                {
                    highlights.push((range, strike_style));
                }
            }
        }
        // S3: search matches paint after the researcher overlays so a match
        // stays visible inside a highlight, and the field-wise composition
        // keeps a struck match's line-through; the current match paints over
        // the plain match. Marks render on every words view — the search is
        // working-view-local, never source-mapped.
        let search_match_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_hover),
            ..Default::default()
        };
        let search_current_style = HighlightStyle {
            background_color: Some(cx.theme().colors().element_active),
            ..Default::default()
        };
        for (index, (start_word, end_word)) in self.transcript_search.matches.iter().enumerate() {
            if let Some(range) = block_word_char_range(block, *start_word, *end_word) {
                let style = if index == self.transcript_search.current {
                    search_current_style
                } else {
                    search_match_style
                };
                highlights.push((range, style));
            }
        }
        let word_starts = block.word_starts.clone();
        let entity = cx.entity().downgrade();
        // S2: exact hover tracking for the press-and-release selection —
        // map the hovered character to its word; characters outside the
        // word ranges (gaps) read as None so a release there cancels the
        // pending drag instead of selecting from a stale position. The
        // state tracks always (the press needs the current word); the
        // repaint fires only while a drag is pending.
        let hover_entity = entity.clone();
        let hover_ranges = block.word_char_ranges.clone();
        let interactive = InteractiveText::new(
            SharedString::from(format!("media-transcript-paragraph-{block_index}")),
            StyledText::new(block.text.clone())
                .with_highlights(resolved_highlight_runs(highlights)),
        )
        .on_hover(move |hovered, _event, _window, cx| {
            let hovered_word = hovered.and_then(|char_ix| {
                hover_ranges
                    .iter()
                    .position(|range| range.contains(&char_ix))
            });
            hover_entity
                .update(cx, |viewer, cx| {
                    if viewer.drag_last_word != hovered_word {
                        viewer.drag_last_word = hovered_word;
                        if viewer.word_drag_anchor.is_some() {
                            cx.notify();
                        }
                    }
                })
                .ok();
        })
        .on_click(
            block.word_char_ranges.clone(),
            move |range_index, window, cx| {
                if let Some((global_word, start_ms)) = word_starts.get(range_index) {
                    entity
                        .update(cx, |viewer, cx| {
                            viewer.word_clicked(*global_word, *start_ms, window, cx)
                        })
                        .ok();
                }
            },
        );
        // Block layout (NOT flex): the text element wraps only when it
        // receives a definite width — a flex row lets the paragraph be
        // probed at max-content and laid out as one overflowing line
        // (operator report 2026-10-26). min_w_0 + block gives the pane
        // width down the chain.
        let container = div()
            .id(SharedString::from(format!(
                "media-transcript-block-{block_index}"
            )))
            .debug_selector(|| format!("media-transcript-block-{block_index}"))
            .min_w_0();
        let marked = current_word
            .is_some_and(|word| block.word_starts.iter().any(|(index, _)| *index == word));
        let container = if marked {
            container
                // Test-support hook (S3d): the follow-along visibility
                // test locates the marked block through this selector
                // (noop in release).
                .debug_selector(|| "media-transcript-marked-block".into())
        } else if block_index == 0 {
            container
                // Test-support hook: exposes this element's laid-out
                // bounds to layout ground-truth tests (noop in release).
                .debug_selector(|| "media-transcript-paragraph".into())
        } else {
            container
        };
        section
            .child(container.child(interactive))
            .into_any_element()
    }

    /// The highlights tail section (S4): labeled selection rows — seek,
    /// reel-inclusion toggle, rename, delete — on index-identical views.
    pub(super) fn render_workspace_highlights_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        if state.highlights.is_empty() {
            return div().into_any_element();
        }
        let index_identical = matches!(state.alignment.as_str(), "original" | "aligned");
        let mut section = v_flex().gap_2();
        section = section.child(Self::section_label("Highlights"));
        if !index_identical {
            section = section.child(
                Label::new(SharedString::from(
                    "Timed navigation and text overlays are unavailable for this view.",
                ))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
        }
        for (row_index, highlight) in state.highlights.iter().enumerate() {
            let row = if index_identical {
                format!(
                    "{} — {}",
                    highlight.label,
                    transcript_range_excerpt(state, highlight.start_word, highlight.end_word)
                )
            } else {
                highlight.label.clone()
            };
            let start_ms = if index_identical {
                state
                    .words
                    .as_deref()
                    .and_then(|words| words.get(highlight.start_word))
                    .map(|word| word.start_ms)
            } else {
                None
            };
            let row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-highlight-row-{row_index}"
                )))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
            if let Some(start_ms) = start_ms {
                section =
                    section.child(row_element.on_click(cx.listener(move |this, _, _, cx| {
                        this.seek_workspace_transcript(start_ms, cx);
                    })));
            } else {
                section = section.child(row_element);
            }
            if highlight.researcher_supplied {
                let annotation_id = highlight.annotation_id.clone();
                let (start_word, end_word) = (highlight.start_word, highlight.end_word);
                // S6: membership reads the clip's include flag — the ONE
                // source of truth. Safe on re_anchored views too: the flag
                // keys the stored highlight's own source anchors, never a
                // working selection.
                let included = self.reel_clips.iter().any(|clip| {
                    clip.annotation_id == annotation_id
                        && clip.start_word == start_word
                        && clip.end_word == end_word
                        && clip.include
                });
                let include_toggle = div()
                    .id(SharedString::from(format!(
                        "media-transcript-reel-toggle-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if included {
                            "Included in reel"
                        } else {
                            "Add to reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if included {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                    )
                    .on_click({
                        let toggle_annotation_id = annotation_id.clone();
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_reel_clip_include(
                                &toggle_annotation_id,
                                start_word,
                                end_word,
                                cx,
                            );
                        })
                    });
                let edit = HighlightEdit {
                    kind: HighlightEditKind::Rename {
                        annotation_id: highlight.annotation_id.clone(),
                        start_word: highlight.start_word,
                        end_word: highlight.end_word,
                        note: highlight.note.clone(),
                    },
                    buffer: LabelBuffer::from_str(highlight.label.as_str()),
                };
                let rename = div()
                    .id(SharedString::from(format!(
                        "media-transcript-highlight-rename-{}",
                        highlight.annotation_id
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Rename"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_label_editor(edit.clone(), window, cx);
                    }));
                let delete = div()
                    .id(SharedString::from(format!(
                        "media-transcript-highlight-delete-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Delete"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_workspace_annotation(annotation_id.clone(), "Highlight", cx);
                    }));
                section = section.child(
                    h_flex()
                        .flex_wrap()
                        .gap_3()
                        .child(include_toggle)
                        .child(rename)
                        .child(delete),
                );
            }
        }
        section.into_any_element()
    }

    /// The cuts tail section (R2): struck ranges with seek and Restore.
    pub(super) fn render_workspace_cuts_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        if state.strikes.is_empty() {
            return div().into_any_element();
        }
        let index_identical = matches!(state.alignment.as_str(), "original" | "aligned");
        let mut section = v_flex().gap_2();
        section = section.child(Self::section_label("Cuts"));
        if !index_identical {
            section = section.child(
                Label::new(SharedString::from(
                    "Timed navigation and line-through overlays are unavailable for this view.",
                ))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
        }
        for strike in &state.strikes {
            let row = if index_identical {
                transcript_range_excerpt(state, strike.start_word, strike.end_word)
            } else {
                "Cut selection".to_string()
            };
            let start_ms = if index_identical {
                state
                    .words
                    .as_deref()
                    .and_then(|words| words.get(strike.start_word))
                    .map(|word| word.start_ms)
            } else {
                None
            };
            let row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-strike-row-{}",
                    strike.annotation_id
                )))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_hover))
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Default),
                );
            if let Some(start_ms) = start_ms {
                section =
                    section.child(row_element.on_click(cx.listener(move |this, _, _, cx| {
                        this.seek_workspace_transcript(start_ms, cx);
                    })));
            } else {
                section = section.child(row_element);
            }
            let annotation_id = strike.annotation_id.clone();
            section = section.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-strike-delete-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Restore"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_workspace_annotation(annotation_id.clone(), "Strike", cx);
                    })),
            );
        }
        section.into_any_element()
    }

    /// The reel tail section (S5/S6): the reel composer's surface — the
    /// ordered clip list with per-clip reorder, include toggle, duration
    /// readout, and in-reel strikes with their Restore — plus the preview
    /// toggle (player-local, no admission) and the plain-language compose
    /// actions. Visible only when their inputs exist.
    pub(super) fn render_workspace_reel_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.transcript.as_ref() else {
            return div().into_any_element();
        };
        let any_researcher_highlight = state
            .highlights
            .iter()
            .any(|highlight| highlight.researcher_supplied);
        let affordances = [
            (ReelComposeMode::Assemble, any_researcher_highlight),
            (ReelComposeMode::Reduce, !state.strikes.is_empty()),
            (
                ReelComposeMode::Mixed,
                any_researcher_highlight && !state.strikes.is_empty(),
            ),
        ];
        let any_included_clip = self.reel_clips.iter().any(|clip| clip.include);
        let mut section = v_flex().gap_2();
        if affordances.iter().any(|(_, visible)| *visible)
            || !self.reel_clips.is_empty()
            || any_included_clip
        {
            section = section.child(Self::section_label("Compose a reel"));
        }
        // The composed clip list (S6): order, inclusion, and in-reel strikes,
        // editable in place. Clip rows carry the composed position, label,
        // word range, and duration (the baked span total over the source
        // bundle words); the ↑/↓ buttons reorder, the include toggle flips
        // membership, and each in-reel strike restores individually.
        let clip_count = self.reel_clips.len();
        for (clip_index, clip) in self.reel_clips.iter().enumerate() {
            let row = format!(
                "{}. {} — words {}–{}",
                clip_index + 1,
                clip.label,
                clip.start_word,
                clip.end_word,
            );
            let duration = reel_clip_duration_ms(state, clip);
            let mut row_element = div()
                .id(SharedString::from(format!(
                    "media-transcript-reel-clip-{clip_index}"
                )))
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Label::new(SharedString::from(row))
                        .size(LabelSize::XSmall)
                        .color(if clip.include {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                );
            if let Some(duration_ms) = duration {
                row_element = row_element.child(
                    Label::new(SharedString::from(format!(
                        "{:.1}s",
                        duration_ms as f64 / 1000.0
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            section = section.child(row_element);

            let mut controls = div()
                .id(SharedString::from(format!(
                    "media-transcript-reel-clip-controls-{clip_index}"
                )))
                .flex()
                .flex_wrap()
                .gap_2();
            if clip_index > 0 {
                controls = controls.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-up-{clip_index}"
                        )))
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("↑"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_reel_clip(clip_index, true, cx);
                        })),
                );
            }
            if clip_index + 1 < clip_count {
                controls = controls.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-down-{clip_index}"
                        )))
                        .cursor_pointer()
                        .hover(|el| el.bg(cx.theme().colors().element_hover))
                        .child(
                            Label::new(SharedString::from("↓"))
                                .size(LabelSize::XSmall)
                                .color(ui::Color::Default),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.move_reel_clip(clip_index, false, cx);
                        })),
                );
            }
            let annotation_id = clip.annotation_id.clone();
            let (start_word, end_word) = (clip.start_word, clip.end_word);
            controls = controls.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-reel-toggle-{annotation_id}"
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if clip.include {
                            "Included in reel"
                        } else {
                            "Add to reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if clip.include {
                            ui::Color::Default
                        } else {
                            ui::Color::Muted
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_reel_clip_include(&annotation_id, start_word, end_word, cx);
                    })),
            );
            section = section.child(controls);

            for (strike_index, (strike_start, strike_end)) in clip.strikes.iter().enumerate() {
                section = section.child(
                    div()
                        .id(SharedString::from(format!(
                            "media-transcript-reel-clip-strike-{clip_index}-{strike_index}"
                        )))
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Label::new(SharedString::from(format!(
                                "Struck words {strike_start}–{strike_end}"
                            )))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Error),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-transcript-reel-clip-restore-{clip_index}-{strike_index}"
                                )))
                                .cursor_pointer()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .child(
                                    Label::new(SharedString::from("Restore"))
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Default),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_reel_clip_strike(clip_index, strike_index, cx);
                                })),
                        ),
                );
            }
        }
        // The preview toggle (S6): player-local — the seek paths' class, no
        // admission. The auto-seek chain plays the composed spans on the
        // shared player without rendering.
        if any_included_clip {
            let previewing = self.reel_preview.is_some();
            section = section.child(
                div()
                    .id("media-transcript-reel-preview-toggle")
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(if previewing {
                            "Stop preview"
                        } else {
                            "Preview reel"
                        }))
                        .size(LabelSize::XSmall)
                        .color(if previewing {
                            ui::Color::Error
                        } else {
                            ui::Color::Default
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_reel_preview(cx);
                    })),
            );
        }
        for (mode, visible) in affordances {
            if !visible {
                continue;
            }
            section = section.child(
                div()
                    .id(SharedString::from(format!(
                        "media-transcript-compose-reel-{}",
                        mode.key()
                    )))
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from(mode.label()))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.compose_workspace_reel(mode, cx);
                    })),
            );
        }
        section.into_any_element()
    }

    /// One visual-transcript event row (S5): kind, source-PTS range,
    /// searchable text, and the provenance model — the row seeks playback
    /// to its start on click (the word pane's click-to-seek discipline,
    /// criterion 1). Box-carrying rows carry the Target arm chip (criterion
    /// 3's "select target"); the anchored row and rows inside the pending
    /// span highlight.
    pub(super) fn render_workspace_event_row(
        &self,
        row: &PaneEventRow,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let time = match row.end_sec {
            Some(end) => format!("{:.1}s–{:.1}s", row.start_sec, end),
            None => format!("{:.1}s", row.start_sec),
        };
        let anchored = self.event_selection_anchor == Some(index);
        let in_span = self
            .event_selection_span
            .is_some_and(|(start, end)| row.start_sec < end && row.end_or_start() > start);
        let row_id = SharedString::from(format!("media-event-row-{index}"));
        let mut element = div()
            .id(row_id)
            .debug_selector(move || format!("media-event-row-{index}"))
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .when(anchored || in_span, |el| {
                el.bg(cx.theme().colors().element_hover)
            })
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(row.kind.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(time))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .child(
                Label::new(SharedString::from(row.searchable_text.clone()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
            .child(
                Label::new(SharedString::from(format!("· {}", row.model)))
                    .size(LabelSize::XSmall)
                    .color(if row.model == "user" {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            );
        // The S1 refinement's read-time corroboration badges: the sensory
        // streams that confirm this event (the door slam "heard" AND
        // "seen" renders both). Derived server-side at read time; a
        // human-confirmed corroboration can be authored as a researcher
        // item.
        for badge in &row.evidence {
            element = element.child(
                Label::new(SharedString::from(format!("· {}", badge)))
                    .size(LabelSize::XSmall)
                    .color(if badge == "authored" {
                        ui::Color::Muted
                    } else {
                        ui::Color::Accent
                    }),
            );
        }
        // The redaction target arm: only rows carrying a normalized box
        // (machine object tags — the tier-1 spatial source). Arming stops
        // propagation so the click does not also seek.
        if let Some(_box_norm) = row.box_norm {
            let armed = self
                .redaction_target
                .as_ref()
                .is_some_and(|target| target.source_ref == row.source_ref());
            let target_chip = div()
                .id(SharedString::from(format!("media-event-target-{index}")))
                .cursor_pointer()
                .flex()
                .flex_wrap()
                .gap_2()
                .hover(|el| el.bg(cx.theme().colors().element_active))
                .child(
                    Label::new(SharedString::from(if armed {
                        "◎ Target"
                    } else {
                        "Target"
                    }))
                    .size(LabelSize::XSmall)
                    .color(if armed {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
                )
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.arm_redaction_target(index, cx);
                }));
            element = element.child(target_chip);
        }
        let start_sec = row.start_sec;
        element
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.event_clicked(index, start_sec, cx);
            }))
            .into_any_element()
    }

    /// Whether the loaded event log is a truncated page: the server pages
    /// `layers_transcript` (the panel requests the 5000-event maximum),
    /// so a longer recording's log reads as a prefix of its total.
    pub(super) fn visual_events_truncated(&self) -> bool {
        self.visual_events
            .as_ref()
            .is_some_and(VisualEventsState::truncated)
    }

    /// The truncation note at the bottom of the event list: a page
    /// shorter than the recording's total names what it omits — page vs
    /// total, never a bare count that silently claims completeness.
    pub(super) fn render_workspace_events_truncation_note(&self) -> AnyElement {
        let Some(state) = self.visual_events.as_ref() else {
            return div().into_any_element();
        };
        div()
            .pb_2()
            .debug_selector(|| "media-events-truncation-note".into())
            .child(
                Label::new(SharedString::from(state.truncation_note()))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            )
            .into_any_element()
    }

    /// The events surface's tail section (S5): the pending span, the
    /// select-to-author tag flow (criterion 2), the redaction dispatch
    /// (criterion 3 — armed target + effect + span → author the approved
    /// decision and render it), and the last render's review/lineage lines
    /// (S3's segments[] + applied_items[]).
    pub(super) fn render_workspace_redaction_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut section = v_flex()
            .gap_2()
            .debug_selector(|| "media-events-authoring-section".into());
        section = section.child(Self::section_label("Visual-transcript actions"));
        // The pending span: what both authoring flows consume.
        match self.event_selection_span {
            Some((start, end)) => {
                let span_label = Label::new(SharedString::from(format!(
                    "Selected span: {start:.1}s–{end:.1}s"
                )))
                .size(LabelSize::XSmall)
                .color(ui::Color::Default);
                let tag_chip = div()
                    .id("media-event-tag-span")
                    .cursor_pointer()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .hover(|el| el.bg(cx.theme().colors().element_hover))
                    .child(
                        Label::new(SharedString::from("Tag span"))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Default),
                    )
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.open_event_span_tag_editor(window, cx);
                    }));
                section = section.child(
                    h_flex()
                        .items_center()
                        .flex_wrap()
                        .gap_2()
                        .child(span_label)
                        .child(tag_chip),
                );
            }
            None => {
                let hint = if self.selection_mode {
                    "Click two events to select a span."
                } else {
                    "Turn on Select span, then click two events to select a span."
                };
                section = section.child(
                    Label::new(SharedString::from(hint))
                        .size(LabelSize::XSmall)
                        .color(ui::Color::Muted),
                );
            }
        }
        // The redaction block: armed target, effect cycle, dispatch.
        let mut redaction = v_flex().gap_1();
        redaction = redaction.child(
            Label::new(SharedString::from("Redaction"))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
        );
        let target_line = match self.redaction_target.as_ref() {
            Some(target) => format!(
                "Target: {} (box {:.2},{:.2} {:.0}%×{:.0}%)",
                target.label,
                target.box_norm[0],
                target.box_norm[1],
                target.box_norm[2] * 100.0,
                target.box_norm[3] * 100.0
            ),
            None => "No target armed — click Target on an event with a box.".to_string(),
        };
        redaction = redaction.child(
            Label::new(SharedString::from(target_line))
                .size(LabelSize::XSmall)
                .color(if self.redaction_target.is_some() {
                    ui::Color::Default
                } else {
                    ui::Color::Muted
                }),
        );
        let effect = self.redaction_effect;
        let effect_chip = div()
            .id("media-event-redaction-effect")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(format!("Effect: {}", effect.as_str())))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Default),
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.redaction_effect = this.redaction_effect.next();
                cx.notify();
            }));
        let ready = self.redaction_target.is_some() && self.event_selection_span.is_some();
        let redact_chip = div()
            .id("media-event-redact-span")
            .cursor_pointer()
            .flex()
            .flex_wrap()
            .gap_2()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from("Redact span"))
                    .size(LabelSize::XSmall)
                    .color(if ready {
                        ui::Color::Error
                    } else {
                        ui::Color::Muted
                    }),
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.dispatch_redaction_render(cx);
            }));
        redaction = redaction.child(
            h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(effect_chip)
                .child(redact_chip),
        );
        section = section.child(redaction);
        // The last render's review/lineage lines (S3's result rendered for
        // review — criterion 3).
        if let Some(summary) = self.redaction_render.as_ref() {
            let mut render = v_flex().gap_1();
            render = render.child(
                Label::new(SharedString::from(format!(
                    "Last render — {} decision(s) · {} re-encoded · {} stream-copied · layer {}",
                    summary.decisions_rendered,
                    summary.reencoded_segments,
                    summary.stream_copied_segments,
                    summary.decision_layer_id
                )))
                .size(LabelSize::XSmall)
                .color(ui::Color::Muted),
            );
            for segment in &summary.segments {
                render = render.child(
                    Label::new(SharedString::from(format!(
                        "  {} {:.1}s–{:.1}s ({} item(s))",
                        segment.mode, segment.start_sec, segment.end_sec, segment.items
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            for applied in &summary.applied {
                render = render.child(
                    Label::new(SharedString::from(format!(
                        "  {} {}×{} at {},{} over {:.1}s–{:.1}s",
                        applied.effect,
                        applied.pixel_w,
                        applied.pixel_h,
                        applied.pixel_x,
                        applied.pixel_y,
                        applied.local_start_sec,
                        applied.local_end_sec
                    )))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
                );
            }
            section = section.child(render);
        }
        section.into_any_element()
    }
}
