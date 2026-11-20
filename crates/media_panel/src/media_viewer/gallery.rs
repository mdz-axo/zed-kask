use super::*;

impl MediaViewer {
    /// Load the gallery's current page. The committed offset changes only
    /// after a complete response parses, so a failed page request preserves
    /// the entire last-good snapshot.
    pub(super) fn load_gallery(&mut self, cx: &mut Context<Self>) {
        self.request_gallery_page(self.gallery_offset, cx);
    }

    pub(super) fn reload_gallery(&mut self, cx: &mut Context<Self>) {
        self.gallery_request.invalidate();
        self.request_gallery_page(self.gallery_offset, cx);
    }

    pub(super) fn request_gallery_page(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(epoch) = self
            .gallery_request
            .begin(RequestOwner::GalleryPage(offset))
        else {
            return;
        };
        self.status = None;
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.apply_gallery_listing(
                epoch,
                Err("Tool invoker not wired — panel cannot query the gallery.".into()),
                cx,
            );
            return;
        };
        cx.notify();
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "gallery_list_assets",
            serde_json::json!({ "offset": offset, "limit": GALLERY_PAGE_LIMIT }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(text) => parse_gallery_listing(
                    hkask_types::tool_response::parse_tool_response(&text),
                    offset,
                ),
                Err(error) => Err(format!("gallery_list_assets failed: {}", error.message())),
            };
            this.update(cx, |this, cx| this.apply_gallery_listing(epoch, result, cx))
                .log_err();
        })
        .detach();
    }

    /// expect: Gallery rows and pagination metadata change only for the latest complete response.
    /// [P1] Motivating: malformed or superseded pages cannot corrupt the last-good Library snapshot.
    /// pre: epoch was returned by gallery_request.begin.
    /// post: stale outcomes are ignored; valid owned snapshots commit atomically; owned failures preserve data.
    pub(super) fn apply_gallery_listing(
        &mut self,
        epoch: u64,
        result: Result<GalleryListing, String>,
        cx: &mut Context<Self>,
    ) {
        if !self.gallery_request.owns(epoch) {
            return;
        }
        match result {
            Ok(listing) => {
                self.commit_gallery_listing(listing, cx);
                self.gallery_request.complete(epoch, Ok(()));
            }
            Err(error) => {
                self.gallery_request.complete(epoch, Err(error));
            }
        }
        cx.notify();
    }

    pub(super) fn commit_gallery_listing(
        &mut self,
        listing: GalleryListing,
        cx: &mut Context<Self>,
    ) {
        let gallery_changed = self
            .gallery_id
            .as_ref()
            .is_some_and(|gallery_id| gallery_id != &listing.gallery_id);
        let selected_src = self
            .selected
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        let confirmed_src = self
            .confirm_delete
            .armed
            .and_then(|ix| self.assets.get(ix))
            .map(|asset| asset.src.clone());
        for record in &listing.assets {
            if let Some(path) = record.get("path").and_then(Value::as_str)
                && let Err(error) =
                    hkask_media_widget::approve_gallery_media_path(std::path::Path::new(path))
            {
                log::warn!("failed to approve listed gallery media path: {error}");
            }
        }
        let mut next_assets = self.assets.clone();
        if gallery_changed {
            next_assets.retain(|asset| asset.gallery_index.is_none());
        }
        merge_gallery_records(
            &mut next_assets,
            &listing.assets,
            listing.offset,
            Some(listing.total),
        );

        let next_selected = if gallery_changed {
            None
        } else {
            selected_src
                .as_ref()
                .and_then(|src| next_assets.iter().position(|asset| &asset.src == src))
                .or_else(|| (!next_assets.is_empty()).then_some(0))
        };
        let next_confirm_delete = if gallery_changed {
            None
        } else {
            confirmed_src
                .as_ref()
                .and_then(|src| next_assets.iter().position(|asset| &asset.src == src))
        };
        let selected_changed = selected_src
            != next_selected.and_then(|ix| next_assets.get(ix).map(|asset| asset.src.clone()));

        self.assets = next_assets;
        self.gallery_total = Some(listing.total);
        self.gallery_offset = listing.offset;
        self.gallery_limit = listing.limit;
        self.gallery_id = Some(listing.gallery_id);
        self.selected = next_selected;
        self.confirm_delete.armed = next_confirm_delete;
        if gallery_changed || selected_changed {
            self.clear_media_widget(cx);
            self.detail = None;
            self.detail_request.invalidate();
            self.detail_transcripts = None;
            self.detail_transcripts_request.invalidate();
            self.detail_annotations = None;
            self.detail_annotations_request.invalidate();
            // The inspector keeps its Idle copy after a listing repair
            // moves the selection (the pane's reads follow the user's
            // actions — entry, selection change, toggle — not the
            // background listing merge; reopening the pane reloads).
        }
    }

    #[cfg(test)]
    pub(super) fn merge_gallery_listing(&mut self, payload: Option<Value>, cx: &mut Context<Self>) {
        let requested_offset = payload
            .as_ref()
            .and_then(|payload| payload.get("offset"))
            .and_then(Value::as_u64)
            .and_then(|offset| usize::try_from(offset).ok())
            .unwrap_or(self.gallery_offset);
        match parse_gallery_listing(payload, requested_offset) {
            Ok(listing) => self.commit_gallery_listing(listing, cx),
            Err(error) => self.gallery_request.state = ResourceState::Failed(error),
        }
    }

    pub(super) fn previous_gallery_page(&mut self, cx: &mut Context<Self>) {
        let offset = self.gallery_offset.saturating_sub(self.gallery_limit);
        self.request_gallery_page(offset, cx);
    }

    pub(super) fn next_gallery_page(&mut self, cx: &mut Context<Self>) {
        let Some(total) = self
            .gallery_total
            .and_then(|total| usize::try_from(total).ok())
        else {
            return;
        };
        let offset = self.gallery_offset.saturating_add(self.gallery_limit);
        if offset < total {
            self.request_gallery_page(offset, cx);
        }
    }

    /// Delete the gallery-index entry for the selected asset (two-step confirm; the
    /// file on disk is left untouched — `delete_file: false`). Addresses the
    /// asset by its stable gallery id — a positional index captured before a
    /// root switch could delete a DIFFERENT asset in the new gallery.
    pub(super) fn delete_asset(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(asset) = self.assets.get(ix) else {
            return;
        };
        let Some(asset_id) = asset.gallery_asset_id.clone() else {
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
            serde_json::json!({ "image_id": asset_id, "delete_file": false }),
        );
        cx.spawn(async move |this, cx| {
            match task.await.and_then(|response| {
                parse_delete_acknowledgement(&response, &asset_id)
                    .map_err(|message| hkask_tool_invoker::InvokeError::Failed { message, kind: None })
            }) {
                Ok(transcripts_detached) => {
                    this.update(cx, |this, cx| {
                        this.confirm_delete.disarm();
                        this.status = Some(format!(
                            "Asset removed from the gallery index; {transcripts_detached} transcript link(s) detached."
                        ));
                        // Supersede any listing admitted before deletion.
                        this.reload_gallery(cx);
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

    /// The Library (T3): the browse mode — every indexed asset as rows,
    /// and the recordings browser's new home: a gallery video/audio asset
    /// carries its transcription state with the Transcribe affordance on
    /// its row (R5) and its stored transcripts nested beneath it (open,
    /// delete — durable exports are preserved by the server on deletion).
    /// Unlinked transcripts — their recordings left the gallery index —
    /// trail in their own section with honest degradation copy, reachable,
    /// never silently hidden (operator report 2026-10-26: the section must
    /// list what's available, not dead-end).
    pub(super) fn render_library(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        if self.gallery_total.is_none() {
            match &self.gallery_request.state {
                ResourceState::Loading => {
                    return self
                        .render_empty("Loading Library", "Fetching the first gallery page…")
                        .into_any_element();
                }
                ResourceState::Failed(error) => {
                    return self
                        .render_empty("Library unavailable", error)
                        .into_any_element();
                }
                ResourceState::Idle => {
                    return self
                        .render_empty("Library not loaded", "Open or refresh Library to load it.")
                        .into_any_element();
                }
                ResourceState::Ready => {}
            }
        }
        let total = self.gallery_total.unwrap_or(0);
        let total_as_usize = usize::try_from(total).unwrap_or(usize::MAX);
        let page_end = self
            .gallery_offset
            .saturating_add(self.gallery_limit)
            .min(total_as_usize);
        // The stored transcripts whose recordings left the gallery index —
        // rendered in their own section below the rows, never hidden.
        let unlinked_rows: Vec<usize> = self
            .transcript_listing
            .as_ref()
            .map(|listing| {
                listing
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| row.gallery_asset_id.is_none())
                    .map(|(row_index, _)| row_index)
                    .collect()
            })
            .unwrap_or_default();
        let rows = self
            .assets
            .iter()
            .enumerate()
            .filter(|(_, asset)| {
                asset
                    .gallery_index
                    .is_none_or(|index| (self.gallery_offset..page_end).contains(&index))
            })
            .map(|(ix, asset)| {
                let selected = self.selected == Some(ix);
                let confirming = self.confirm_delete.is_armed(&ix);
                let icon = if asset.kind == "image" {
                    IconName::Image
                } else {
                    IconName::File
                };
                // The recording's transcription state (R2/R5, T3): with
                // the listing loaded, the full status — a live transcribe
                // job (the observed-anywhere live set), a stored linked
                // transcript, or Not transcribed. Without the listing,
                // only a live job is knowable — the recordings status
                // line above the rows explains the rest; a not-yet-loaded
                // listing must not render a dishonest Not-transcribed.
                let recording_id = if asset.kind == "image" {
                    None
                } else {
                    asset.gallery_asset_id.as_deref()
                };
                let transcription = match recording_id {
                    Some(_) => match self.transcript_listing.as_ref() {
                        Some(_) => Some(self.recording_transcription_status(asset)),
                        None => self
                            .live_jobs
                            .iter()
                            .any(|job| {
                                job.op == "transcribe"
                                    && job.source.as_deref() == Some(asset.src.as_str())
                            })
                            .then_some(RecordingTranscriptionStatus::Transcribing),
                    },
                    None => None,
                };
                let transcript_row_indices: Vec<usize> =
                    match (recording_id, self.transcript_listing.as_ref()) {
                        (Some(id), Some(listing)) => listing
                            .iter()
                            .enumerate()
                            .filter(|(_, row)| row.gallery_asset_id.as_deref() == Some(id))
                            .map(|(row_index, _)| row_index)
                            .collect(),
                        _ => Vec::new(),
                    };
                let line = h_flex()
                    .h_8()
                    .gap_2()
                    .flex()
                    .items_center()
                    .min_w_0()
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
                            // The ambient activity badge (T2): a live
                            // research job whose primary source is this
                            // asset — transient progress, never
                            // error-styled (HIG Progress indicators).
                            .when_some(self.asset_activity_verb(asset), |el, verb| {
                                let badge_selector = format!("media-library-activity-{ix}");
                                el.child(
                                    div()
                                        .id(SharedString::from(badge_selector.clone()))
                                        .debug_selector(move || badge_selector)
                                        .flex_shrink_0()
                                        .child(
                                            Label::new(verb)
                                                .size(LabelSize::XSmall)
                                                .color(ui::Color::Hint),
                                        ),
                                )
                            })
                            // The recording's transcription state (T3) —
                            // the status the old recordings browser carried,
                            // now beside the recording's own Library row.
                            .when_some(transcription, |el, status| {
                                el.child(
                                    Label::new(match status {
                                        RecordingTranscriptionStatus::Transcribing => {
                                            "Transcribing…"
                                        }
                                        RecordingTranscriptionStatus::Transcribed => "Transcribed",
                                        RecordingTranscriptionStatus::NotTranscribed => {
                                            "Not transcribed"
                                        }
                                    })
                                    .size(LabelSize::XSmall)
                                    .color(match status {
                                        RecordingTranscriptionStatus::Transcribing => {
                                            ui::Color::Hint
                                        }
                                        RecordingTranscriptionStatus::Transcribed => {
                                            ui::Color::Muted
                                        }
                                        RecordingTranscriptionStatus::NotTranscribed => {
                                            ui::Color::Error
                                        }
                                    })
                                    .flex_shrink_0(),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_selection(Some(ix), cx);
                                this.activate_tab(ViewerTab::Workspace, cx);
                            })),
                    );
                // The Transcribe affordance (R5, operator ruling 2026-10-28):
                // a not-transcribed recording can be taken to its
                // transcript without a chat turn — a sibling of the select
                // area, so opening and transcribing never fire together.
                let line = match transcription {
                    Some(RecordingTranscriptionStatus::NotTranscribed) => {
                        let recording_id = recording_id.unwrap_or_default().to_string();
                        let source = asset.src.clone();
                        line.child(
                            div()
                                .id(SharedString::from(format!(
                                    "media-recording-transcribe-{ix}"
                                )))
                                .debug_selector(|| "media-recording-transcribe".into())
                                .cursor_pointer()
                                .px_2()
                                .h_5()
                                .flex()
                                .items_center()
                                .flex_shrink_0()
                                .rounded_sm()
                                .hover(|el| el.bg(cx.theme().colors().element_hover))
                                .child(
                                    Label::new("Transcribe")
                                        .size(LabelSize::XSmall)
                                        .color(ui::Color::Default),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.transcribe_recording(
                                        recording_id.clone(),
                                        source.clone(),
                                        cx,
                                    );
                                })),
                        )
                    }
                    _ => line,
                };
                let line = line.child(
                    // Curate affordance: two-step delete (index entry only).
                    div()
                        .id(SharedString::from(format!("media-library-delete-{ix}")))
                        .px_2()
                        .h_5()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
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
                            if this.confirm_delete.confirm(&ix) {
                                this.delete_asset(ix, cx);
                            } else {
                                cx.notify();
                            }
                        })),
                );
                div()
                    .id(SharedString::from(format!("media-library-row-{ix}")))
                    .px_2()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(line)
                    // The recording's stored transcripts, nested beneath
                    // its row (the old recordings browser's arrangement).
                    .children(transcript_row_indices.iter().map(|row_index| {
                        div()
                            .pl_4()
                            .child(self.render_transcript_row(*row_index, cx))
                    }))
            })
            .collect::<Vec<_>>();
        let first = if total == 0 {
            0
        } else {
            self.gallery_offset.saturating_add(1)
        };
        let pagination = h_flex()
            .h_8()
            .gap_2()
            .px_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                ui::Button::new("media-library-previous", "Previous")
                    .label_size(LabelSize::XSmall)
                    .disabled(self.gallery_offset == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.previous_gallery_page(cx))),
            )
            .child(
                ui::Button::new("media-library-next", "Next")
                    .label_size(LabelSize::XSmall)
                    .disabled(page_end >= total_as_usize)
                    .on_click(cx.listener(|this, _, _, cx| this.next_gallery_page(cx))),
            )
            .child(
                Label::new(format!("Showing {first}–{page_end} of {total}"))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        // The recordings section's status line (T3): when the listing is
        // not loaded, the rows render without transcription states — this
        // line names why (D5, never silence), always visible above the
        // content, never scrolled out of view.
        let recordings_note = match self.transcript_listing.as_ref() {
            Some(_) => None,
            None => match &self.transcript_listing_request.state {
                ResourceState::Loading => Some((
                    "Loading the recordings' transcripts…".to_string(),
                    ui::Color::Muted,
                )),
                ResourceState::Failed(error) => Some((
                    format!("Transcripts unavailable: {error}"),
                    ui::Color::Error,
                )),
                ResourceState::Idle | ResourceState::Ready => Some((
                    "Recordings' transcripts not loaded — reload the Library to list them."
                        .to_string(),
                    ui::Color::Muted,
                )),
            },
        };
        let content = if rows.is_empty() && unlinked_rows.is_empty() {
            self.render_empty(
                "Library empty",
                "This page has no indexed assets and the conversation has surfaced none.",
            )
            .into_any_element()
        } else {
            v_flex()
                .id("media-viewer-library-rows")
                .flex_1()
                .overflow_y_scroll()
                .gap_0p5()
                .p_2()
                .children(rows)
                .when(!unlinked_rows.is_empty(), |el| {
                    el.child(Self::section_label("Unlinked transcripts"))
                        .child(
                            Label::new(SharedString::from(
                                "Their recordings left the gallery index — the transcripts, \
                                 their durable exports, and the stored media path remain \
                                 openable here.",
                            ))
                            .size(LabelSize::XSmall)
                            .color(ui::Color::Hint),
                        )
                        .children(
                            unlinked_rows
                                .iter()
                                .map(|row_index| self.render_transcript_row(*row_index, cx))
                                .collect::<Vec<_>>(),
                        )
                })
                .into_any_element()
        };
        v_flex()
            .id("media-viewer-library")
            .flex_1()
            .min_h_0()
            .child(Self::tab_purpose(
                "Every indexed asset — browse and select; recordings carry their transcription state and their transcripts beneath them. Delete removes the listing, never the file on disk.",
            ))
            .child(pagination)
            .when_some(recordings_note, |el, (note, color)| {
                el.child(
                    div().px_3().py_1().child(
                        Label::new(SharedString::from(note))
                            .size(LabelSize::XSmall)
                            .color(color)
                            .truncate(),
                    ),
                )
            })
            .child(content)
            .into_any_element()
    }

    /// One stored-transcript row (S3a, T3): the recording's name and
    /// metadata; a click opens the workspace's transcript sub-surface.
    /// Rendered nested beneath its recording's Library row, and in the
    /// unlinked section below the rows. The Delete affordance is a SIBLING
    /// of the clickable open area (the Library row pattern), so opening and
    /// deleting can never fire together. Two clicks: Delete arms the
    /// confirmation, Confirm delete dispatches the server's
    /// `educt_delete_transcript` (durable exports and rendered reels are
    /// preserved by the server, detached from the removed record).
    pub(super) fn render_transcript_row(
        &self,
        row_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(listing) = self.transcript_listing.as_ref() else {
            return div().into_any_element();
        };
        let Some(transcript) = listing.get(row_index) else {
            return div().into_any_element();
        };
        let timings = if transcript.has_word_timings {
            ""
        } else {
            " · text only"
        };
        let transcript_id = transcript.id.clone();
        let title = recording_display_name(&transcript.media_path);
        let metadata = format!(
            "{} · {} words{timings}",
            format_recording_duration(transcript.duration_secs),
            transcript.words_count
        );
        let mut row = div()
            .id(SharedString::from(format!(
                "media-transcript-open-{}",
                transcript.id
            )))
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap_1()
            .py_1()
            .flex_1()
            .min_w_0()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(SharedString::from(title))
                    .size(LabelSize::Small)
                    .color(ui::Color::Default),
            )
            .child(
                Label::new(SharedString::from(metadata))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Muted),
            );
        if let Some(note) = transcript_alignment_note(&transcript.alignment) {
            row = row.child(
                Label::new(SharedString::from(note))
                    .size(LabelSize::XSmall)
                    .color(ui::Color::Error),
            );
        }
        let confirming = self.confirm_delete_transcript.is_armed(&transcript.id);
        let delete_row_id = transcript.id.clone();
        let delete_button = div()
            .id(SharedString::from(format!(
                "media-transcript-delete-{}",
                transcript.id
            )))
            .debug_selector(move || {
                if confirming {
                    "media-transcript-delete-confirm".into()
                } else {
                    "media-transcript-delete".into()
                }
            })
            .cursor_pointer()
            .px_2()
            .py_1()
            .flex_none()
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .child(
                Label::new(if confirming {
                    SharedString::from("Confirm delete")
                } else {
                    SharedString::from("Delete")
                })
                .size(LabelSize::XSmall)
                .color(if confirming {
                    ui::Color::Error
                } else {
                    ui::Color::Muted
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.confirm_delete_transcript.confirm(&delete_row_id) {
                    this.delete_transcript_recording(delete_row_id.clone(), cx);
                } else {
                    cx.notify();
                }
            }));
        div()
            .id(SharedString::from(format!(
                "media-transcript-row-{}",
                transcript.id
            )))
            .flex()
            .items_start()
            .gap_2()
            .child(row.on_click(cx.listener(move |this, _, _, cx| {
                this.open_transcript(transcript_id.clone(), cx);
            })))
            .child(delete_button)
            .into_any_element()
    }

    /// The per-recording transcription status (R2/R5): a live
    /// transcribe job whose source is the recording shows Transcribing
    /// (the observed-anywhere live set — a deep job counts too, T2); a
    /// stored transcript linked to the recording shows Transcribed;
    /// otherwise Not transcribed.
    pub(super) fn recording_transcription_status(
        &self,
        asset: &MediaAsset,
    ) -> RecordingTranscriptionStatus {
        let running = self
            .live_jobs
            .iter()
            .any(|job| job.source.as_deref() == Some(asset.src.as_str()));
        if running {
            return RecordingTranscriptionStatus::Transcribing;
        }
        let recording_id = asset.gallery_asset_id.as_deref();
        let stored = self.transcript_listing.as_ref().is_some_and(|listing| {
            listing
                .iter()
                .any(|row| row.gallery_asset_id.as_deref() == recording_id)
        });
        if stored {
            RecordingTranscriptionStatus::Transcribed
        } else {
            RecordingTranscriptionStatus::NotTranscribed
        }
    }

    /// The ambient activity badge for one Library row (T2): the verb of
    /// a live research job whose primary source is this asset —
    /// transient progress (HIG Progress indicators: present only while
    /// an operation is ongoing, never error-styled). `None` when no
    /// live job targets the asset. The job record carries its primary
    /// `source` (the transcribe input, the trim/clip source); for concat
    /// jobs only the first input badges — the server exposes no wider
    /// subject list on the record.
    pub(super) fn asset_activity_verb(&self, asset: &MediaAsset) -> Option<&'static str> {
        self.live_jobs
            .iter()
            .find(|job| job.source.as_deref() == Some(asset.src.as_str()))
            .map(|job| job_op_verb(&job.op))
    }

    /// The Transcribe affordance (R5, operator ruling 2026-10-28):
    /// submit the async transcribe job against the recording's source,
    /// storing and linking the transcript on completion — the operator
    /// can take a recording from import to transcript to reel entirely
    /// in the panel.
    pub(super) fn transcribe_recording(
        &mut self,
        gallery_asset_id: String,
        source: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = hkask_tool_invoker::shared_tool_invoker() else {
            self.status = Some("Tool invoker not wired — panel cannot transcribe.".into());
            cx.notify();
            return;
        };
        self.status = Some("Transcription queued — the Activity drawer tracks it.".into());
        let task = invoker.invoke_tool(
            MEDIA_SERVER,
            "job_submit",
            serde_json::json!({
                "op": "transcribe",
                "params": serde_json::json!({
                    "audio_url": source,
                    "gallery_asset_id": gallery_asset_id,
                })
                .to_string(),
            }),
        );
        cx.spawn(async move |this, cx| {
            let result = match task.await {
                Ok(_text) => Ok(()),
                Err(error) => Err(format!("transcription submit failed: {}", error.message())),
            };
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.status = Some(error);
                }
                this.load_jobs(cx);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }
}
