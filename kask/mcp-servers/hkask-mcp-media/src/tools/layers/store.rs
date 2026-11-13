//! The store family: store and recall media layers — the write path
//! (layers_store validates before persisting; a failing layer is
//! rejected with the named invariant and nothing is written) and the
//! read paths (layers_list, layers_get, and layers_transcript — the
//! visual transcript with read-time cross-stream corroboration).
//! decode_item, sense_for_kind and transcript_stream are private to
//! this family; the root keeps the shared map_layer_store_error.

use super::*;

/// Decode the wire item form into the store's domain item: the payload
/// crosses the boundary as `AnyJsonValue` JSON text and is decoded here
/// (the transcript-store precedent).
fn decode_item(
    item: &LayersStoreItemRequest,
    index: usize,
) -> Result<MediaLayerItem, McpToolError> {
    let payload = parse_json_value(item.payload.0.clone(), &format!("items[{index}].payload"))?;
    Ok(MediaLayerItem {
        start_sec: item.start_sec,
        end_sec: item.end_sec,
        searchable_text: item.searchable_text.clone(),
        payload,
        frame_asset_ids: item.frame_asset_ids.clone(),
    })
}

/// The sensory badge for one event kind (the S1 refinement's read-time
/// corroboration): audio_event → heard, transcript → spoken, the vision
/// kinds → seen, the authored kinds → authored. Every event carries at
/// least its own badge plus the badges of every overlapping event.
fn sense_for_kind(kind: &str) -> &'static str {
    match kind {
        "audio_event" => "heard",
        "transcript" => "spoken",
        "researcher" | "edl" => "authored",
        _ => "seen",
    }
}

/// The word transcript as one synchronized stream (the S1 refinement):
/// the newest transcript's segments as (start_sec, end_sec, text) events —
/// kind "transcript", spoken-word provenance. `None` when no transcript
/// exists for the path — the event log reads the layer streams alone.
fn transcript_stream(
    driver: &dyn DatabaseDriver,
    media_path: &str,
) -> Result<Option<(String, Option<String>, Vec<(f64, f64, String)>)>, McpToolError> {
    let summaries = transcript_store::list_transcripts(
        driver,
        &TranscriptFilter {
            media_path: Some(media_path.to_string()),
            gallery_asset_id: None,
            limit: 1,
        },
    )
    .map_err(map_store_error)?;
    let Some(summary) = summaries.first() else {
        return Ok(None);
    };
    let Some((_summary, bundle)) =
        transcript_store::load_transcript(driver, &summary.id).map_err(map_store_error)?
    else {
        return Ok(None);
    };
    let segments = bundle
        .segments
        .iter()
        .map(|segment| {
            (
                segment.start_ms as f64 / 1000.0,
                segment.end_ms as f64 / 1000.0,
                segment.text.clone(),
            )
        })
        .collect();
    Ok(Some((summary.id.clone(), bundle.model, segments)))
}

#[tool_router(router = layers_store_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Store one media layer with time-keyed items over a local or URL video/audio file. Item ranges are source-PTS seconds validated against the ffprobe-anchored media duration; reversed or out-of-bounds ranges, blank searchable_text, frame asset ids absent from the gallery, unknown gallery asset ids, a layer with no items and a blank provenance model or prompt_template are rejected with named errors and nothing is persisted. Provenance (model, prompt_template) is recorded as the caller declares it; created_at is stamped server-side. Layers are database rows only — source media bytes are never modified."
    )]
    pub async fn layers_store(
        &self,
        Parameters(request): Parameters<LayersStoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_store", async {
            // A layer records who or what made it and annotates something:
            // one with neither is refused before the media is probed.
            if request.provenance.model.trim().is_empty()
                || request.provenance.prompt_template.trim().is_empty()
            {
                return Err(McpToolError::invalid_argument(
                    "provenance.model and provenance.prompt_template must be nonblank",
                ));
            }
            if request.items.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "items is empty: a layer with no items annotates nothing",
                ));
            }
            admit_media_source(&request.media_path).await?;
            let ffmpeg = self.require_ffmpeg()?;
            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            let provenance = LayerProvenance {
                model: request.provenance.model,
                prompt_template: request.provenance.prompt_template,
                created_at: hkask_types::time::now_rfc3339(),
                parameters: None,
            };
            let items = request
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| decode_item(item, index))
                .collect::<Result<Vec<_>, _>>()?;
            let new_layer = NewMediaLayer {
                media_path: request.media_path.clone(),
                gallery_asset_id: request.gallery_asset_id.clone(),
                kind: request.kind,
                label: request.label.clone(),
                provenance,
                duration_secs: probe.duration_secs as f64,
                items,
            };
            let driver = &**self.gallery_store.driver();
            let summary =
                layer_store::store_layer(driver, &new_layer).map_err(map_layer_store_error)?;
            let mut value = serde_json::to_value(&summary)
                .map_err(|e| McpToolError::internal(format!("encode layer summary: {e}")))?;
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "probed_duration_secs".to_string(),
                    serde_json::json!(probe.duration_secs),
                );
                object.insert(
                    "time_basis".to_string(),
                    serde_json::json!("source_pts_seconds"),
                );
            }
            Ok(value)
        })
        .await
    }

    #[tool(
        description = "List stored media layers, newest first, optionally filtered by media_path and/or kind. Returns summaries with provenance and item counts; load full items with layers_get. limit defaults to 50, must be at least 1 and is capped at 500."
    )]
    pub async fn layers_list(
        &self,
        Parameters(request): Parameters<LayersListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_list", async {
            if request.limit == Some(0) {
                return Err(McpToolError::invalid_argument(
                    "limit must be at least 1; a limit of 0 lists nothing",
                ));
            }
            let driver = &**self.gallery_store.driver();
            let filter = MediaLayerFilter {
                media_path: request.media_path,
                kind: request.kind,
                limit: request.limit.unwrap_or(50).min(500) as usize,
            };
            let layers =
                layer_store::list_layers(driver, &filter).map_err(map_layer_store_error)?;
            Ok(serde_json::json!({
                "layers": layers,
                "count": layers.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Load one stored media layer with its full items: stable item ids, source-PTS time ranges, searchable text, payload, and frame asset ids. Unknown layer ids fail as not_found naming the id."
    )]
    pub async fn layers_get(
        &self,
        Parameters(request): Parameters<LayersGetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_get", async {
            let driver = &**self.gallery_store.driver();
            let Some(record) =
                layer_store::get_layer(driver, &request.layer_id).map_err(map_layer_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "layer {} not found",
                    request.layer_id
                )));
            };
            Ok(serde_json::json!({
                "layer": record,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }

    #[tool(
        description = "Read a recording's visual transcript: every stored layer item merged into one time-ordered event list across layer kinds — the media sibling of reading a word transcript. Events carry stable item ids (resolvable via layers_get), source-PTS second ranges, kind, layer label, searchable text, payload, frame asset ids and provenance (researcher layers record model \"user\" — the human/machine distinction), plus read-time cross-stream corroboration `evidence` — the sensory badges of every overlapping event in the returned page plus its own (the S1 refinement: a door slam \"heard\" via audio_event AND \"seen\" via object/semantic at overlapping PTS ranges shows both badges; single-stream hits show one). The corroboration derives only at read time — nothing is stored, and a human-confirmed corroboration can be authored as a researcher item if wanted. No stored layers is the named no_layers state naming the passes that create layers; nothing is silent. Optional kind filter, source-PTS window (events overlapping the window are returned), limit (default 500, 1..=5000) and resume cursor; a longer transcript returns next_from. Source media bytes are never modified."
    )]
    pub async fn layers_transcript(
        &self,
        Parameters(request): Parameters<LayersTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_transcript", async {
            // A malformed window is a caller error, never a nonsensical
            // read: the overlap filter below would answer a reversed range
            // with events straddling both boundaries instead of rejecting
            // it — the same admission the bounded passes enforce
            // (layers_semantic_pass, layers_audio_pass), ahead of any store
            // read. A zero-width window (start == end) stays a legitimate
            // instant read — point-in-time items intersect it.
            if let Some(range) = &request.time_range {
                if !range.start_sec.is_finite() || range.start_sec < 0.0 {
                    return Err(McpToolError::invalid_argument(format!(
                        "time_range start_sec must be >= 0, got {}",
                        range.start_sec
                    )));
                }
                if let Some(end_sec) = range.end_sec {
                    if range.start_sec > end_sec {
                        return Err(McpToolError::invalid_argument(format!(
                            "reversed time_range: start_sec {} > end_sec {}",
                            range.start_sec, end_sec
                        )));
                    }
                }
            }
            // The stated bounds are enforced, not clamped: a limit of 0
            // returned one event and 4,000,000,000 returned 5000, each
            // while reporting the page as what was asked for.
            let limit = request.limit.unwrap_or(500);
            if !(1..=5000).contains(&limit) {
                return Err(McpToolError::invalid_argument(format!(
                    "limit must be between 1 and 5000, got {limit}"
                )));
            }
            let driver = &**self.gallery_store.driver();
            let records = layer_store::recording_layers(driver, &request.media_path, request.kind)
                .map_err(map_layer_store_error)?;
            let transcript_events = if request.kind.is_none() {
                transcript_stream(driver, &request.media_path)?
            } else {
                None
            };
            if records.is_empty() && transcript_events.is_none() {
                // The note states the actual absence: under a kind filter
                // that matches nothing, the recording may still carry
                // layers of other kinds — "no stored layers" would be
                // false.
                let note = match request.kind.as_ref() {
                    Some(kind) => format!(
                        "no layers of kind '{}' for this recording — try without a \
                         kind filter or run the appropriate pass (layers_shot_pass, \
                         layers_semantic_pass, layers_audio_pass) or author one \
                         (layers_store)",
                        kind.as_str()
                    ),
                    None => "no stored layers for this recording — run a layer pass \
                            (layers_shot_pass, layers_semantic_pass, layers_audio_pass) \
                            or author one (layers_store)"
                        .to_string(),
                };
                return Ok(serde_json::json!({
                    "status": "no_layers",
                    "media_path": request.media_path,
                    "events": [],
                    "note": note,
                }));
            }
            let mut events: Vec<(f64, f64, String, String, serde_json::Value)> = Vec::new();
            for record in &records {
                let kind = record.kind.as_str().to_string();
                let layer_id = record.id.clone();
                let label = record.label.clone();
                let provenance = serde_json::to_value(&record.provenance).map_err(|error| {
                    McpToolError::internal(format!("encode provenance: {error}"))
                })?;
                for stored in &record.items {
                    let start = stored.item.start_sec;
                    let end = stored.item.end_sec.unwrap_or(start);
                    if let Some(range) = &request.time_range {
                        if end < range.start_sec {
                            continue;
                        }
                        if let Some(window_end) = range.end_sec {
                            if start > window_end {
                                continue;
                            }
                        }
                    }
                    events.push((
                        start,
                        end,
                        kind.clone(),
                        stored.id.clone(),
                        serde_json::json!({
                            "start_sec": start,
                            "end_sec": stored.item.end_sec,
                            "kind": kind.clone(),
                            "layer_id": layer_id.clone(),
                            "item_id": stored.id.clone(),
                            "label": label.clone(),
                            "searchable_text": stored.item.searchable_text.clone(),
                            "payload": stored.item.payload.clone(),
                            "frame_asset_ids": stored.item.frame_asset_ids.clone(),
                            "provenance": provenance.clone(),
                        }),
                    ));
                }
            }
            let mut transcript_id = serde_json::Value::Null;
            if let Some((id, model, segments)) = transcript_events {
                transcript_id = serde_json::json!(id);
                for (start, end, text) in segments {
                    if let Some(range) = &request.time_range {
                        if end < range.start_sec {
                            continue;
                        }
                        if let Some(window_end) = range.end_sec {
                            if start > window_end {
                                continue;
                            }
                        }
                    }
                    events.push((
                        start,
                        end,
                        "transcript".to_string(),
                        format!("segment-{start:.3}-{end:.3}"),
                        serde_json::json!({
                            "start_sec": start,
                            "end_sec": end,
                            "kind": "transcript",
                            "layer_id": id.clone(),
                            "item_id": format!("segment-{start:.3}-{end:.3}"),
                            "label": "spoken words",
                            "searchable_text": text,
                            "payload": serde_json::Value::Null,
                            "frame_asset_ids": serde_json::Value::Null,
                            "provenance": {
                                "model": model.clone(),
                                "prompt_template": "transcript_segments",
                            },
                        }),
                    ));
                }
            }
            events.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.2.cmp(&b.2))
                    .then_with(|| a.3.cmp(&b.3))
            });
            let total = events.len();
            let kinds_present: Vec<String> = events
                .iter()
                .map(|event| event.2.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let resume_from = (request.resume_from.unwrap_or(0) as usize).min(total);
            let page_end = (resume_from + limit as usize).min(total);
            // Read-time cross-stream corroboration (the S1 refinement): each
            // event carries the sensory badges of every overlapping event in
            // the returned page plus its own (the door slam heard AND seen).
            // Derived only — nothing stored; a human-confirmed corroboration
            // can be authored as a researcher item if wanted.
            let page = &events[resume_from..page_end];
            let mut values: Vec<serde_json::Value> = Vec::with_capacity(page.len());
            for (index, event) in page.iter().enumerate() {
                let mut badges = vec![sense_for_kind(&event.2).to_string()];
                for (other_index, other) in page.iter().enumerate() {
                    if other_index == index || other.1 < event.0 || event.1 < other.0 {
                        continue;
                    }
                    let badge = sense_for_kind(&other.2).to_string();
                    if !badges.contains(&badge) {
                        badges.push(badge);
                    }
                }
                let mut object = event.4.as_object().cloned().unwrap_or_default();
                object.insert("evidence".to_string(), serde_json::json!(badges));
                values.push(serde_json::Value::Object(object));
            }
            let next_from = if page_end < total {
                Some(page_end as u64)
            } else {
                None
            };
            Ok(serde_json::json!({
                "status": "completed",
                "media_path": request.media_path,
                "transcript_id": transcript_id,
                "events": values,
                "events_total": total,
                "events_page": page_end - resume_from,
                "kinds_present": kinds_present,
                "layers_included": records
                    .iter()
                    .map(|record| record.id.clone())
                    .collect::<Vec<String>>(),
                "resume_from": resume_from as u64,
                "next_from": next_from,
                "time_basis": "source_pts_seconds",
            }))
        })
        .await
    }
}
