//! The edl family: compose researcher selections into EDL layers and
//! render them — layers_edl_compose (union-merged Keep ops validated
//! against media duration), layers_render_edl (the playable reel), and
//! layers_render_redactions (tier-1 cover effects behind the
//! human-approved decision gate). The root keeps the shared
//! MAX_LAYERS_PER_RECORDING cap.

use super::*;

#[tool_router(router = layers_edl_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Compose researcher selections into a media EDL layer with union-merged ordered Keep ops validated against media duration. Takes a media_path plus either a layer_id (of a researcher-authored selection layer over that recording) or query, the exact label of the recording's researcher layers to compose from. Overlapping selections merge into one Keep op; duplicates collapse; a selection with no duration (a point-in-time item) keeps nothing and is left out, counted in point_selections_skipped. Ops outside the probed media duration reject with a typed error naming the bounds. The composed EDL is stored as a new kind=edl layer whose provenance records the source (layer id or query)."
    )]
    pub async fn layers_edl_compose(
        &self,
        Parameters(request): Parameters<LayersEdlComposeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_edl_compose", async {
            admit_media_source(&request.media_path).await?;
            let ffmpeg = self.require_ffmpeg()?;
            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            let duration_secs = probe.duration_secs as f64;
            let driver = &**self.gallery_store.driver();

            // Collect time ranges from either a selection layer or a label
            // query.
            let (source_description, ranges) = match (&request.layer_id, &request.query) {
                (Some(_), Some(_)) => {
                    return Err(McpToolError::invalid_argument(
                        "exactly one of layer_id or query is required",
                    ));
                }
                (Some(layer_id), None) => {
                    let Some(record) =
                        layer_store::get_layer(driver, layer_id).map_err(map_layer_store_error)?
                    else {
                        return Err(McpToolError::not_found(format!(
                            "layer {} not found",
                            layer_id
                        )));
                    };
                    if record.kind != MediaLayerKind::Researcher {
                        return Err(McpToolError::invalid_argument(format!(
                            "layer_id must reference a researcher-authored layer, got kind {}",
                            record.kind.as_str()
                        )));
                    }
                    // Seconds on one recording mean nothing on another: the
                    // composed layer would cut this recording at the other's
                    // moments.
                    if record.media_path != request.media_path {
                        return Err(McpToolError::invalid_argument(format!(
                            "layer {layer_id} annotates {}, not {}",
                            record.media_path, request.media_path
                        )));
                    }
                    let ranges: Vec<(f64, f64)> = record
                        .items
                        .iter()
                        .map(|item| {
                            (
                                item.item.start_sec,
                                item.item.end_sec.unwrap_or(item.item.start_sec),
                            )
                        })
                        .collect();
                    (format!("layer:{}", layer_id), ranges)
                }
                (None, Some(query)) => {
                    let filter = MediaLayerFilter {
                        media_path: Some(request.media_path.clone()),
                        kind: Some(MediaLayerKind::Researcher),
                        limit: MAX_LAYERS_PER_RECORDING,
                    };
                    let summaries =
                        layer_store::list_layers(driver, &filter).map_err(map_layer_store_error)?;
                    let mut ranges = Vec::new();
                    for summary in &summaries {
                        let Some(record) = layer_store::get_layer(driver, &summary.id)
                            .map_err(map_layer_store_error)?
                        else {
                            return Err(McpToolError::not_found(format!(
                                "layer {} disappeared during compose",
                                summary.id
                            )));
                        };
                        if record.label.as_deref() == Some(query.as_str()) {
                            for item in &record.items {
                                ranges.push((
                                    item.item.start_sec,
                                    item.item.end_sec.unwrap_or(item.item.start_sec),
                                ));
                            }
                        }
                    }
                    (format!("query:{}", query), ranges)
                }
                (None, None) => {
                    return Err(McpToolError::invalid_argument(
                        "exactly one of layer_id or query is required",
                    ));
                }
            };

            if ranges.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "no selections found for {} on {}",
                    source_description, request.media_path
                )));
            }

            // Union-merge overlapping ranges into ordered Keep ops. A point
            // selection has no duration to keep: as a Keep op it rendered a
            // container with no stream, reported as a finished reel.
            let mut sorted: Vec<(f64, f64)> = ranges
                .iter()
                .copied()
                .filter(|(start, end)| end > start)
                .collect();
            let point_selections_skipped = ranges.len() - sorted.len();
            if sorted.is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "the {point_selections_skipped} selections of {source_description} are points in time with no duration; there is nothing to keep"
                )));
            }
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            let mut merged: Vec<(f64, f64)> = Vec::new();
            for &(start, end) in &sorted {
                if start < 0.0 || end > duration_secs + 1e-6 {
                    return Err(McpToolError::invalid_argument(format!(
                        "selection {}..{}s is outside the probed media duration 0..{}s",
                        start, end, duration_secs
                    )));
                }
                match merged.last_mut() {
                    Some(last) if start <= last.1 + 1e-9 => {
                        last.1 = last.1.max(end);
                    }
                    _ => {
                        merged.push((start, end));
                    }
                }
            }

            let items: Vec<MediaLayerItem> = merged
                .iter()
                .map(|&(start, end)| MediaLayerItem {
                    start_sec: start,
                    end_sec: Some(end),
                    searchable_text: format!("keep {}..{}", start, end),
                    payload: serde_json::json!({
                        "op": "keep",
                    }),
                    frame_asset_ids: None,
                })
                .collect();
            let new_layer = NewMediaLayer {
                media_path: request.media_path.clone(),
                gallery_asset_id: request.gallery_asset_id.clone(),
                kind: MediaLayerKind::Edl,
                label: Some(format!("edl from {}", source_description)),
                provenance: LayerProvenance {
                    model: "deterministic".to_string(),
                    prompt_template: "layers_edl_compose".to_string(),
                    created_at: hkask_types::time::now_rfc3339(),
                    parameters: Some(serde_json::json!({
                        "source": source_description,
                        "ops": merged.len(),
                        "input_ranges": ranges.len(),
                    })),
                },
                duration_secs,
                items,
            };
            let summary =
                layer_store::store_layer(driver, &new_layer).map_err(map_layer_store_error)?;

            Ok(serde_json::json!({
                "status": "composed",
                "layer": summary,
                "ops": merged.len(),
                "input_ranges": ranges.len(),
                "point_selections_skipped": point_selections_skipped,
                "source": source_description,
            }))
        })
        .await
    }

    #[tool(
        description = "Render a media EDL layer into a durable playable gallery asset. Takes a media_path and an optional layer_id (a kind=edl layer over that recording; the newest when omitted). The layer's ranges are merged where they overlap and each is clipped from the source media via ffmpeg and joined: audio sources publish a PCM WAV cut on the sample, video sources an MP4 stream-copied where the cut falls on keyframes. The reel is published as a gallery asset whose lineage records the EDL layer and the clip plan that was rendered. An unknown layer_id, or a recording with no EDL layer, is not_found; a layer of another kind or over another recording is invalid_argument; a layer whose ranges have no duration is failed_precondition."
    )]
    pub async fn layers_render_edl(
        &self,
        Parameters(request): Parameters<LayersRenderEdlRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_render_edl", async {
            let _admission = self.admit_heavy_operation()?;
            admit_media_source(&request.media_path).await?;
            let ffmpeg = self.require_ffmpeg()?;
            let driver = &**self.gallery_store.driver();
            // A named layer is looked up by its id, so each way it can be
            // wrong is told apart; the listing this replaced answered every
            // one of them "no EDL layer found for <path>".
            let record = match &request.layer_id {
                Some(layer_id) => {
                    let Some(record) =
                        layer_store::get_layer(driver, layer_id).map_err(map_layer_store_error)?
                    else {
                        return Err(McpToolError::not_found(format!(
                            "layer {layer_id} not found"
                        )));
                    };
                    if record.kind != MediaLayerKind::Edl {
                        return Err(McpToolError::invalid_argument(format!(
                            "layer {layer_id} has kind {}; a reel renders from a kind=edl layer",
                            record.kind.as_str()
                        )));
                    }
                    if record.media_path != request.media_path {
                        return Err(McpToolError::invalid_argument(format!(
                            "layer {layer_id} annotates {}, not {}",
                            record.media_path, request.media_path
                        )));
                    }
                    record
                }
                None => {
                    let filter = MediaLayerFilter {
                        media_path: Some(request.media_path.clone()),
                        kind: Some(MediaLayerKind::Edl),
                        limit: 1,
                    };
                    let newest =
                        layer_store::list_layers(driver, &filter).map_err(map_layer_store_error)?;
                    let Some(summary) = newest.first() else {
                        return Err(McpToolError::not_found(format!(
                            "no EDL layer found for {}",
                            request.media_path
                        )));
                    };
                    let Some(record) = layer_store::get_layer(driver, &summary.id)
                        .map_err(map_layer_store_error)?
                    else {
                        return Err(McpToolError::not_found(format!(
                            "layer {} disappeared during render",
                            summary.id
                        )));
                    };
                    record
                }
            };
            // The plan that is rendered: ranges with a duration, in order,
            // merged where they overlap. layers_store accepts a hand-built
            // edl layer, so the stored items are not yet a plan: overlapping
            // ranges played their shared seconds twice and a point range
            // rendered a container with no stream.
            let mut ranges: Vec<(f64, f64)> = record
                .items
                .iter()
                .filter_map(|item| {
                    let end = item.item.end_sec?;
                    (end > item.item.start_sec).then_some((item.item.start_sec, end))
                })
                .collect();
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            let mut plan: Vec<(f64, f64)> = Vec::with_capacity(ranges.len());
            for (start, end) in ranges {
                match plan.last_mut() {
                    Some(last) if start <= last.1 + 1e-9 => last.1 = last.1.max(end),
                    _ => plan.push((start, end)),
                }
            }
            if plan.is_empty() {
                return Err(McpToolError::failed_precondition(format!(
                    "EDL layer {} holds no range with a duration — nothing to render",
                    record.id
                )));
            }
            let audio = source_is_audio_only(ffmpeg, &request.media_path).await?;
            let mut intermediates = crate::assets::ScratchIntermediates::new("layers_render_edl");
            let mut clip_paths: Vec<std::path::PathBuf> = Vec::with_capacity(plan.len());
            for &(start_sec, end_sec) in &plan {
                let clipped = if audio {
                    ffmpeg
                        .audio_trim(&request.media_path, start_sec as f32, end_sec as f32)
                        .await
                } else {
                    ffmpeg
                        .clip(&request.media_path, start_sec as f32, end_sec as f32)
                        .await
                }
                .map_err(map_media_error)?;
                intermediates.track(clipped.clone());
                clip_paths.push(clipped);
            }
            let output = if clip_paths.len() == 1 {
                clip_paths.first().cloned().ok_or_else(|| {
                    McpToolError::internal("EDL render produced no clip after a non-empty plan")
                })?
            } else {
                let clip_sources = clip_paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                if audio {
                    ffmpeg
                        .audio_concat(&clip_sources)
                        .await
                        .map_err(map_media_error)?
                } else {
                    ffmpeg
                        .concat(&clip_sources)
                        .await
                        .map_err(map_media_error)?
                }
            };
            let effective_params = serde_json::json!({
                "media_path": &request.media_path,
                "edl_layer_id": &record.id,
                "source": &request.media_path,
                "clip_plan": plan
                    .iter()
                    .map(|&(start_sec, end_sec)| serde_json::json!([start_sec, end_sec]))
                    .collect::<Vec<_>>(),
                "clips": clip_paths.len(),
            });
            // A media-layer EDL is not a transcript render: record_render
            // requires the EDL layer id to exist in transcript_layers (a
            // word-transcript concept) and a media_layers id never can —
            // passing one rolled back the whole render when the recording
            // carried a transcript (live failure 2026-10-30). The
            // lineage-only publish carries the EDL layer id and clip plan in
            // effective_params, which is the media surface's contract.
            let result = crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                "layers_render_edl",
                "rendered",
                if audio {
                    crate::assets::LocalMediaFormat::Wav
                } else {
                    crate::assets::LocalMediaFormat::Mp4
                },
                &effective_params,
            )?;
            intermediates.cleanup().map_err(map_media_error)?;
            Ok(serde_json::json!({
                "status": "rendered",
                "result": result,
                "edl_layer_id": record.id,
                "media_path": request.media_path,
            }))
        })
        .await
    }

    #[tool(
        description = "Render approved redaction decisions over one recording into a durable playable gallery asset — tier 1 (cover: blur, pixelate, black, delogo) of the visual-event-tagging design §6. Takes a media_path and the REQUIRED layer_id of a researcher-authored redaction decision layer (provenance model \"user\") whose items carry the vocabulary {op: redact, status: approved, effect: blur|pixelate|black|delogo, target: {kind: box, box: {x,y,w,h} in 0..1}}. No redaction renders without an approved decision layer — the hard human gate: unknown layers fail as not_found, wrong-kind/unapproved/malformed items as failed_precondition, a layer over another recording or a source without a video stream as invalid_argument, and machine-authored layers never render; every failure names the layer. Only the affected segments re-encode through the ffmpeg filter graph (time-ranged per item via enable=between over its range, spatial over the normalized box scaled to the probed frame dimensions), keyed to keyframe-aligned segment boundaries so the output timeline tiles the source exactly; untouched segments stream-copy. Source media bytes are never modified — the render publishes a new gallery asset with lineage. A mask_track target is reserved tier-2 vocabulary (representable now, renderable once tier 2 exists); requesting it fails as permission_denied naming the mask/removal serving path."
    )]
    pub async fn layers_render_redactions(
        &self,
        Parameters(request): Parameters<LayersRenderRedactionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_render_redactions", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&request.media_path)?;
            if !crate::is_local_media_path(&request.media_path) {
                validate_tool_url_with_dns(&request.media_path).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let probe = ffmpeg
                .probe(&request.media_path)
                .await
                .map_err(map_media_error)?;
            if !probe.has_video_stream {
                return Err(McpToolError::invalid_argument(format!(
                    "redaction render requires a video stream; {} has none",
                    request.media_path
                )));
            }
            let driver = &**self.gallery_store.driver();
            // The hard human gate (design §6): no redaction renders without
            // an approved researcher-authored decision layer. Every
            // rejection names the layer.
            let Some(record) =
                layer_store::get_layer(driver, &request.layer_id).map_err(map_layer_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "redaction decision layer {} not found",
                    request.layer_id
                )));
            };
            if record.kind != MediaLayerKind::Researcher {
                return Err(McpToolError::invalid_argument(format!(
                    "layer {} must be a researcher-authored redaction decision \
                     layer, got kind {}",
                    request.layer_id,
                    record.kind.as_str()
                )));
            }
            if record.provenance.model != redaction::RESEARCHER_MODEL {
                return Err(McpToolError::failed_precondition(format!(
                    "redaction decision layer {} was authored by model \
                     \"{}\" — redaction decisions are human acts (provenance \
                     model \"user\"), never machine-authored",
                    request.layer_id, record.provenance.model
                )));
            }
            // The same mistake as the other layer tools' other-recording
            // refusal, and the same kind (the layer exists; the caller paired
            // it with the wrong recording).
            if record.media_path != request.media_path {
                return Err(McpToolError::invalid_argument(format!(
                    "redaction decision layer {} is not a decision layer for {} \
                     (it annotates {})",
                    request.layer_id, request.media_path, record.media_path
                )));
            }
            if record.items.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "redaction decision layer {} is empty — nothing to render",
                    request.layer_id
                )));
            }
            let duration_secs = probe.duration_secs as f64;
            let mut decisions = Vec::with_capacity(record.items.len());
            for item in &record.items {
                match redaction::parse_decision(&record.id, item, duration_secs) {
                    Ok(decision) => decisions.push(decision),
                    Err(RedactionDecisionError::NotARedactionDecision(reason)) => {
                        return Err(McpToolError::failed_precondition(reason));
                    }
                    Err(RedactionDecisionError::Tier2NotConfigured) => {
                        // Reserved tier-2 vocabulary: permission_denied
                        // naming the serving path (the missing-credential
                        // contract; the `NotConfigured` precedent at the
                        // pass error mapper).
                        return Err(McpToolError::permission_denied(format!(
                            "redaction decision layer {} item {} targets kind \
                             mask_track — reserved tier-2 vocabulary \
                             (representable now, renderable once tier 2 exists); \
                             no mask/removal provider is configured on the \
                             tier-2 serving path (the postponed SAM-3 mask → \
                             removal pipeline, model report R3); tier-1 renders \
                             cover box targets only",
                            record.id, item.id
                        )));
                    }
                }
            }
            // Re-encoded segments keep the source's codec family so the
            // concat demuxer's uniform-stream contract holds; an unmapped
            // codec is a visible degradation, never a silent mixed-codec
            // concat.
            let encoder_args = redaction::tier1_encoder_args(&probe.codec).ok_or_else(|| {
                McpToolError::failed_precondition(format!(
                    "tier-1 redaction render re-encodes affected segments in the \
                     source's codec family; source {} is {} — extend the tier-1 \
                     encoder map before redacting this recording",
                    request.media_path, probe.codec
                ))
            })?;
            let keyframe_times = ffmpeg
                .video_keyframe_times(&request.media_path)
                .await
                .map_err(map_media_error)?;
            let plan = redaction::build_segment_plan(&decisions, duration_secs, &keyframe_times);
            let mut intermediates =
                crate::assets::ScratchIntermediates::new("layers_render_redactions");
            // One seek-free segment-muxer split produces every stream-copied
            // part at the plan's keyframe-aligned boundaries; the parts tile
            // the source exactly (a `-ss`/`-to` copy cannot — its video read
            // overshoots by the reorder depth and its audio seek lands whole
            // frames off). The parts map 1:1 onto the plan segments in time
            // order; the re-encoding segments' parts are discarded in favor
            // of the filtered renders below.
            let boundaries: Vec<f64> = plan
                .iter()
                .skip(1)
                .map(|segment| segment.start_sec)
                .collect();
            let split_parts = if plan
                .iter()
                .any(|segment| segment.mode == SegmentMode::StreamCopy)
            {
                let parts = ffmpeg
                    .segment_split(&request.media_path, &boundaries)
                    .await
                    .map_err(map_media_error)?;
                if parts.len() != plan.len() {
                    return Err(McpToolError::failed_precondition(format!(
                        "the segment split produced {} parts for a {}-segment plan \
                         — the keyframe boundaries diverged; refusing to render a \
                         misaligned timeline",
                        parts.len(),
                        plan.len()
                    )));
                }
                for part in &parts {
                    intermediates.track(part.clone());
                }
                parts
            } else {
                Vec::new()
            };
            let mut segment_paths: Vec<std::path::PathBuf> = Vec::with_capacity(plan.len());
            let mut applied_items = Vec::new();
            for (index, segment) in plan.iter().enumerate() {
                let path = match segment.mode {
                    SegmentMode::StreamCopy => {
                        split_parts.get(index).cloned().ok_or_else(|| {
                            McpToolError::internal(
                                "the segment split is missing a planned copy part",
                            )
                        })?
                    }
                    SegmentMode::Reencode => {
                        let in_segment: Vec<&RedactionDecision> = decisions
                            .iter()
                            .filter(|decision| segment.item_ids.contains(&decision.item_id))
                            .collect();
                        let (graph, applied) = redaction::build_filter_graph(
                            &in_segment,
                            segment.start_sec,
                            probe.width,
                            probe.height,
                        )
                        .map_err(|error| match error {
                            RedactionDecisionError::NotARedactionDecision(reason) => {
                                McpToolError::failed_precondition(reason)
                            }
                            // Unreachable in practice — parse_decision rejects
                            // mask_track targets before graph build — but the
                            // per-variant classification stays explicit.
                            RedactionDecisionError::Tier2NotConfigured => {
                                McpToolError::permission_denied(
                                    "mask_track target in a re-encode segment — \
                                     reserved tier-2 vocabulary; tier-1 renders \
                                     cover box targets only",
                                )
                            }
                        })?;
                        applied_items.extend(applied);
                        let rendered = ffmpeg
                            .clip_filtered(
                                &request.media_path,
                                segment.start_sec as f32,
                                segment.end_sec as f32,
                                &graph,
                                encoder_args,
                            )
                            .await
                            .map_err(map_media_error)?;
                        intermediates.track(rendered.clone());
                        rendered
                    }
                };
                segment_paths.push(path);
            }
            let output = if segment_paths.len() == 1 {
                segment_paths.first().cloned().ok_or_else(|| {
                    McpToolError::internal(
                        "redaction render produced no segment after a non-empty plan",
                    )
                })?
            } else {
                let segment_sources = segment_paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                ffmpeg
                    .concat(&segment_sources)
                    .await
                    .map_err(map_media_error)?
            };
            let segments_json: Vec<serde_json::Value> = plan
                .iter()
                .map(|segment| {
                    serde_json::json!({
                        "mode": segment.mode.as_str(),
                        "start_sec": segment.start_sec,
                        "end_sec": segment.end_sec,
                        "items": segment.item_ids,
                    })
                })
                .collect();
            let reencoded_segments = plan
                .iter()
                .filter(|segment| segment.mode == SegmentMode::Reencode)
                .count();
            let stream_copied_segments = plan.len() - reencoded_segments;
            let effective_params = serde_json::json!({
                "media_path": &request.media_path,
                "source": &request.media_path,
                "decision_layer_id": &record.id,
                "segments": &segments_json,
                "decisions_rendered": decisions.len(),
                "reencoded_segments": reencoded_segments,
                "stream_copied_segments": stream_copied_segments,
            });
            let result = crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                "layers_render_redactions",
                "rendered",
                crate::assets::LocalMediaFormat::Mp4,
                &effective_params,
            )?;
            intermediates.cleanup().map_err(map_media_error)?;
            Ok(serde_json::json!({
                "status": "rendered",
                "result": result,
                "decision_layer_id": record.id,
                "media_path": request.media_path,
                "segments": segments_json,
                "reencoded_segments": reencoded_segments,
                "stream_copied_segments": stream_copied_segments,
                "decisions_rendered": decisions.len(),
                "applied_items": applied_items,
            }))
        })
        .await
    }
}
