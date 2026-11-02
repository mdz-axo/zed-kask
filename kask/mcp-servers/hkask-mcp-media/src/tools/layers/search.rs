//! The search family: unified lexical search across stored media layers
//! and transcripts (the unified search — one query over each recording's
//! layer item searchable_text and its newest transcript's working text,
//! joined by media path; the S1 pin surface). The scan-stream machinery
//! (ScanSegment, stream_is_char_boundary) is private to this family; the
//! root keeps the shared surface (MAX_LAYERS_PER_RECORDING and the shared
//! imports).

use super::*;
use crate::types::LayersSearchRequest;

/// One searchable segment of a recording's unified scan stream (t6): a
/// layer item's text with its layer context, or the newest transcript's
/// working text. Segments are matched in isolation — a phrase crossing a
/// segment boundary is not eligible.
enum ScanSegment<'a> {
    LayerItem {
        text: &'a str,
        layer: &'a MediaLayerRecord,
        item: &'a StoredMediaLayerItem,
    },
    Transcript {
        working: &'a WorkingTranscript,
        transcript_id: &'a str,
    },
}

impl ScanSegment<'_> {
    fn text(&self) -> &str {
        match self {
            ScanSegment::LayerItem { text, .. } => text,
            ScanSegment::Transcript { working, .. } => working.text(),
        }
    }
}

/// Whether `pos` is a UTF-8 char boundary of the segment containing it —
/// a position between segments or at the stream end is always valid.
fn stream_is_char_boundary(segments: &[ScanSegment<'_>], pos: usize) -> bool {
    let mut base = 0;
    for segment in segments {
        let len = segment.text().len();
        if pos < base + len {
            return segment.text().is_char_boundary(pos - base);
        }
        if pos == base + len {
            return true;
        }
        base += len;
    }
    pos == base
}

#[tool_router(router = layers_search_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Search across media layers plus transcripts lexically — one query over each recording's layer item searchable_text and its newest transcript's working text, joined by media path (the unified search). ASCII case-insensitive literal substring, a 4 MiB per-call text budget, and (recording_offset, start_byte) resume offsets — exactly the educt_search_transcripts semantics, which the two tools share: the matcher, budget and cursor machinery are the same code. Each recording's scan stream is its kind-filtered layer items (layer order, then stored item order), then its working transcript text. Layer hits carry the item time range, layer id, kind, item id and provenance; transcript hits carry the educt citation fields with stream byte positions. Recordings scanned without layers, without a transcript, or with older transcripts are named in scope_notes — never a silent skip. The kind filter restricts layer hits; media_path restricts the corpus to one recording. Only lexical is supported; corpus export remains the semantic handoff."
    )]
    pub async fn layers_search(
        &self,
        Parameters(request): Parameters<LayersSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "layers_search", async {
            let (limit, recording_limit, offset) = validate_lexical_search(
                &request.mode,
                &request.query,
                request.limit,
                request.recording_limit,
                request.recording_offset,
            )?;
            let driver = &**self.gallery_store.driver();
            let mut recordings = layer_store::list_unified_recordings(
                driver,
                request.media_path.as_deref(),
                recording_limit + 1,
                offset,
            )
            .map_err(map_layer_store_error)?;
            let more_recordings = recordings.len() > recording_limit;
            recordings.truncate(recording_limit);
            if recordings.is_empty() && request.start_byte.unwrap_or(0) != 0 {
                return Err(McpToolError::invalid_argument(
                    "start_byte requires a recording at recording_offset",
                ));
            }
            let mut hits: Vec<serde_json::Value> = Vec::new();
            let mut scanned = 0;
            let mut text_bytes_scanned = 0;
            let mut result_limit_reached = false;
            let mut scan_byte_limit_reached = false;
            let mut next_cursor: Option<(usize, usize)> = None;
            let mut scope_notes: Vec<String> = Vec::new();
            for (index, recording) in recordings.iter().enumerate() {
                if text_bytes_scanned == SEARCH_TEXT_BYTE_BUDGET {
                    scan_byte_limit_reached = true;
                    next_cursor = Some((offset + index, 0));
                    break;
                }
                // The recording's scan stream: its kind-filtered layers'
                // items (list order, then stored item order), then its
                // newest transcript's working text.
                let layer_filter = MediaLayerFilter {
                    media_path: Some(recording.media_path.clone()),
                    kind: request.kind,
                    limit: MAX_LAYERS_PER_RECORDING,
                };
                let layer_summaries =
                    layer_store::list_layers(driver, &layer_filter)
                        .map_err(map_layer_store_error)?;
                if layer_summaries.len() == MAX_LAYERS_PER_RECORDING {
                    return Err(McpToolError::invalid_argument(format!(
                        "recording {} has {} {}matching layers — above the unified search's per-recording bound",
                        recording.media_path,
                        layer_summaries.len(),
                        request.kind.map(|k| format!("{} ", k.as_str())).unwrap_or_default(),
                    )));
                }
                if layer_summaries.is_empty() {
                    scope_notes.push(format!(
                        "no {}layers for {}",
                        request
                            .kind
                            .map(|k| format!("{} ", k.as_str()))
                            .unwrap_or_default(),
                        recording.media_path,
                    ));
                }
                let mut layer_records = Vec::with_capacity(layer_summaries.len());
                for summary in &layer_summaries {
                    let Some(record) =
                        layer_store::get_layer(driver, &summary.id)
                            .map_err(map_layer_store_error)?
                    else {
                        return Err(McpToolError::not_found(format!(
                            "layer {} disappeared during search",
                            summary.id
                        )));
                    };
                    layer_records.push(record);
                }
                let transcripts = crate::transcript_store::list_transcripts_page(
                    driver,
                    &crate::transcript_store::TranscriptFilter {
                        media_path: Some(recording.media_path.clone()),
                        gallery_asset_id: None,
                        limit: 2,
                    },
                    0,
                )
                .map_err(map_store_error)?;
                match transcripts.as_slice() {
                    [] => {
                        scope_notes
                            .push(format!("no transcript for {}", recording.media_path));
                    }
                    [_newest] => {}
                    _ => scope_notes.push(format!(
                        "older transcripts exist for {}; only the newest was searched",
                        recording.media_path
                    )),
                }
                let working = match transcripts.first() {
                    Some(summary) => {
                        let Some((summary, bundle)) =
                            crate::transcript_store::load_transcript(
                                driver,
                                &summary.id,
                            )
                            .map_err(map_store_error)?
                        else {
                            return Err(McpToolError::not_found(format!(
                                "transcript {} disappeared during search",
                                summary.id
                            )));
                        };
                        let working = working_transcript(driver, &summary.id, &bundle)?;
                        Some((summary, working))
                    }
                    None => None,
                };

                let mut segments: Vec<ScanSegment<'_>> = Vec::new();
                for record in &layer_records {
                    for item in &record.items {
                        segments.push(ScanSegment::LayerItem {
                            text: &item.item.searchable_text,
                            layer: record,
                            item,
                        });
                    }
                }
                if let Some((summary, working)) = &working {
                    segments.push(ScanSegment::Transcript {
                        working,
                        transcript_id: &summary.id,
                    });
                }
                scanned += 1;
                let stream_len: usize = segments.iter().map(|s| s.text().len()).sum();
                let start = if index == 0 {
                    request.start_byte.unwrap_or(0)
                } else {
                    0
                };
                if start > stream_len || !stream_is_char_boundary(&segments, start) {
                    return Err(McpToolError::invalid_argument(
                        "start_byte must be a UTF-8 boundary within the first recording's current scan stream",
                    ));
                }
                let mut end =
                    (start + SEARCH_TEXT_BYTE_BUDGET - text_bytes_scanned).min(stream_len);
                while !stream_is_char_boundary(&segments, end) {
                    end -= 1;
                }
                let mut seg_base = 0;
                'segments: for segment in &segments {
                    let seg_len = segment.text().len();
                    let seg_start = seg_base;
                    let seg_end = seg_base + seg_len;
                    seg_base = seg_end;
                    let win_lo = start.max(seg_start);
                    let win_hi = end.min(seg_end);
                    if win_lo >= win_hi {
                        continue;
                    }
                    let local_lo = win_lo - seg_start;
                    let local_hi = win_hi - seg_start;
                    match segment {
                        ScanSegment::LayerItem { text, layer, item } => {
                            for local_match in lexical_item_matches(
                                text,
                                &request.query,
                                local_lo,
                                local_hi,
                            ) {
                                if hits.len() == limit {
                                    result_limit_reached = true;
                                    // Resume AT the first omitted match, not
                                    // after its recording — the educt cursor
                                    // discipline.
                                    next_cursor = Some((offset + index, seg_start + local_match));
                                    end = (seg_start + local_match).min(end);
                                    break 'segments;
                                }
                                hits.push(serde_json::json!({
                                    "source": "layer",
                                    "matched_text": &text[local_match..local_match + request.query.len()],
                                    "start_byte": seg_start + local_match,
                                    "end_byte": seg_start + local_match + request.query.len(),
                                    "media_path": recording.media_path,
                                    "layer_id": layer.id,
                                    "kind": layer.kind.as_str(),
                                    "label": layer.label,
                                    "item_id": item.id,
                                    "start_sec": item.item.start_sec,
                                    "end_sec": item.item.end_sec,
                                    "provenance": layer.provenance,
                                }));
                            }
                        }
                        ScanSegment::Transcript {
                            working,
                            transcript_id,
                        } => {
                            let citations = lexical_citations(
                                working,
                                &request.query,
                                local_lo,
                                local_hi,
                                limit - hits.len() + 1,
                            );
                            for (local_start, mut citation) in citations {
                                let stream_start = seg_start + local_start;
                                if hits.len() == limit {
                                    result_limit_reached = true;
                                    next_cursor = Some((offset + index, stream_start));
                                    end = stream_start.min(end);
                                    break 'segments;
                                }
                                // Stream positions: the educt citation's
                                // local bytes remapped onto the unified
                                // scan stream.
                                citation["start_byte"] = serde_json::json!(stream_start);
                                citation["end_byte"] =
                                    serde_json::json!(stream_start + request.query.len());
                                citation["source"] = serde_json::json!("transcript");
                                citation["transcript_id"] =
                                    serde_json::json!(transcript_id);
                                citation["media_path"] =
                                    serde_json::json!(recording.media_path);
                                hits.push(citation);
                            }
                        }
                    }
                }
                // Count the covered prefix; one <=256-byte lookahead may
                // also have been compared — the educt accounting.
                text_bytes_scanned += end - start;
                if result_limit_reached {
                    break;
                }
                if end < stream_len {
                    scan_byte_limit_reached = true;
                    next_cursor = Some((offset + index, end));
                    break;
                }
            }
            if next_cursor.is_none() && more_recordings {
                next_cursor = Some((offset + scanned, 0));
            }
            Ok(serde_json::json!({
                "mode": "lexical",
                "matching": "ascii_case_insensitive_literal_substring",
                "hits": hits,
                "count": hits.len(),
                "recordings_scanned": scanned,
                "recording_offset": offset,
                "has_more_recordings": more_recordings || scanned < recordings.len(),
                "has_more": next_cursor.is_some(),
                "next_recording_offset": next_cursor.map(|(recording, _)| recording),
                "next_start_byte": next_cursor.map(|(_, byte)| byte),
                "text_bytes_scanned": text_bytes_scanned,
                "scan_byte_limit": SEARCH_TEXT_BYTE_BUDGET,
                "scan_byte_limit_reached": scan_byte_limit_reached,
                "result_limit_reached": result_limit_reached,
                "search_complete": next_cursor.is_none(),
                "scope_notes": scope_notes,
                "coverage_note": "Bounded search from the supplied cursor over each recording's scan stream (its kind-filtered layer items, then its newest transcript's working text). Resume with next_recording_offset and next_start_byte; offsets refer to the current recording order and scan streams, not a frozen snapshot. The byte budget bounds matching, not loading/projection.",
            }))
        })
        .await
    }
}
