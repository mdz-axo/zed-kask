//! Deterministic track stitching over stored object-layer detections
//! (S2, design `visual-event-tagging-design.md` §4 delta 3).
//!
//! The pass emits per-chunk, per-frame detections with no temporal
//! identity. The stitcher links same-label detections across consecutive
//! chunks by box IoU over time and produces the derived span structure a
//! redaction decision can target (`track_id` in payload). The algorithm is
//! pure and deterministic — every parameter is recorded in the derived
//! layer's provenance, and the version label rides `prompt_template`.
//!
//! Degradation discipline (design §8): no track is invented — singleton
//! detections stay unlinked and are named in stats; occlusion gaps keep the
//! track's spans disjoint and counted, never smoothed; cross-shot merging
//! is off by default (shot boundaries are hard cuts by construction).

use crate::layer_store::MediaLayerItem;

/// The stitcher's parameters — defaults recorded in provenance and pinned
/// by tests. `iou_threshold` gates linking, `max_gap_secs` bounds the
/// unobserved interval that still links two detections into one track,
/// and `cross_shot` allows merging across shot boundaries when the source
/// pass was chunked by shots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StitchParams {
    pub iou_threshold: f64,
    pub max_gap_secs: f64,
    pub cross_shot: bool,
}

/// The stitcher's defaults (recorded in the derived layer's provenance
/// parameters — the versioned algorithm label carries the same values).
pub(crate) const DEFAULT_IOU_THRESHOLD: f64 = 0.3;
pub(crate) const DEFAULT_MAX_GAP_SECS: f64 = 2.0;

/// One detection parsed from a source object layer's item — the frame PTS
/// and sampling interval are resolved by the caller from the frame asset's
/// generation params (the pass always stamps them).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Detection {
    pub label: String,
    pub confidence: f64,
    /// Normalized box: [x, y, w, h], every value in 0..1, x+w <= 1, y+h <= 1.
    pub r#box: [f64; 4],
    /// The detection's frame source-PTS seconds.
    pub frame_pts: f64,
    /// The source pass's sampling interval for this frame's chunk (seconds).
    pub sampling_interval: f64,
    pub source_item_id: String,
    pub frame_index: u32,
    pub frame_asset_id: Option<String>,
}

/// The surfaced stitching stats (design §4 delta 3): linked tracks,
/// unlinked detections, gaps, shot fragmentation.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub(crate) struct StitchStats {
    pub tracks: usize,
    pub spans: usize,
    pub unlinked_detections: usize,
    /// Span breaks inside tracks — unobserved intervals that kept the track.
    pub gaps: usize,
    /// Links blocked by `cross_shot: false` at shot boundaries (the source
    /// was shot-chunked; zero otherwise).
    pub shot_fragments: usize,
}

/// Intersection-over-union of two normalized boxes — the linking metric.
pub(crate) fn box_iou(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let (ax, ay, aw, ah) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bw, bh) = (b[0], b[1], b[2], b[3]);
    let x1 = ax.max(bx);
    let y1 = ay.max(by);
    let x2 = (ax + aw).min(bx + bw);
    let y2 = (ay + ah).min(by + bh);
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = aw * ah + bw * bh - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// A track under construction — the walk's working state.
struct TrackWalk {
    label: String,
    detections: Vec<Detection>,
    /// Span boundaries as indices into `detections` starts.
    span_starts: Vec<usize>,
}

impl TrackWalk {
    fn new(label: String, first: Detection) -> Self {
        Self {
            label,
            detections: vec![first],
            span_starts: vec![0],
        }
    }

    fn last(&self) -> &Detection {
        self.detections
            .last()
            .expect("a track holds at least one detection")
    }

    /// Link a detection: same track; a new span when the object was
    /// unobserved for at least one sample (gap > 1.5x the sampling
    /// interval — adjacent samples are ~1x apart).
    fn link(&mut self, detection: Detection) {
        let previous = self.last();
        let interval = previous.sampling_interval.max(detection.sampling_interval);
        let adjacent = detection.frame_pts - previous.frame_pts <= interval * 1.5;
        if !adjacent {
            self.span_starts.push(self.detections.len());
        }
        self.detections.push(detection);
    }
}

/// The stitch result: one item per span plus the stats.
pub(crate) struct StitchResult {
    pub items: Vec<MediaLayerItem>,
    pub stats: StitchStats,
}

/// Stitch detections into tracks and build the derived layer's items.
///
/// Deterministic: detections are grouped by normalized label (trimmed,
/// lowercased for matching — the label itself keeps its source case) and
/// walked in PTS order; a detection links to the current track's last
/// detection when the PTS gap is within `max_gap_secs` and the boxes' IoU
/// clears `iou_threshold`. When the source pass was shot-chunked
/// (`source_chunked_by_shots`) and `cross_shot` is false, a link across
/// source items (shot boundaries) is refused and counted — never merged.
/// Span ranges tighten to the source frame's PTS ± half the sampling
/// interval (design §4 delta 4). `duration_secs` clamps the tightened
/// ranges; the store re-validates against the probed anchor.
pub(crate) fn stitch(
    detections: &[Detection],
    params: StitchParams,
    source_chunked_by_shots: bool,
    duration_secs: f64,
) -> StitchResult {
    let mut by_label: std::collections::BTreeMap<String, Vec<&Detection>> =
        std::collections::BTreeMap::new();
    for detection in detections {
        let key = detection.label.trim().to_lowercase();
        by_label.entry(key).or_default().push(detection);
    }

    let mut stats = StitchStats::default();
    let mut items: Vec<MediaLayerItem> = Vec::new();
    let mut track_ordinal: usize = 0;

    for (_key, group) in &by_label {
        let mut sorted: Vec<&Detection> = group.clone();
        sorted.sort_by(|a, b| {
            a.frame_pts
                .partial_cmp(&b.frame_pts)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut track: Option<TrackWalk> = None;
        let mut finished: Vec<TrackWalk> = Vec::new();
        for detection in sorted {
            let linkable = match track.as_ref() {
                None => false,
                Some(current) => {
                    let previous = current.last();
                    let gap = detection.frame_pts - previous.frame_pts;
                    let shot_boundary = source_chunked_by_shots
                        && !params.cross_shot
                        && detection.source_item_id != previous.source_item_id;
                    if shot_boundary {
                        stats.shot_fragments += 1;
                        false
                    } else if gap > params.max_gap_secs {
                        false
                    } else {
                        box_iou(&previous.r#box, &detection.r#box) >= params.iou_threshold
                    }
                }
            };
            if linkable {
                track
                    .as_mut()
                    .expect("linkable implies a live track")
                    .link((*detection).clone());
            } else {
                if let Some(current) = track.take() {
                    finished.push(current);
                }
                track = Some(TrackWalk::new(
                    detection.label.trim().to_string(),
                    (*detection).clone(),
                ));
            }
        }
        if let Some(current) = track.take() {
            finished.push(current);
        }

        for current in finished {
            if current.detections.len() < 2 {
                // No track is invented (design §8): a singleton stays
                // unlinked and searchable in the source layer — counted.
                stats.unlinked_detections += current.detections.len();
                continue;
            }
            track_ordinal += 1;
            let track_id = format!(
                "{}-{track_ordinal}",
                current.label.trim().to_lowercase().replace(' ', "-")
            );
            let span_count = current.span_starts.len();
            for (span_index, span_start) in current.span_starts.iter().enumerate() {
                let span_end = current
                    .span_starts
                    .get(span_index + 1)
                    .copied()
                    .unwrap_or(current.detections.len());
                let span_detections = &current.detections[*span_start..span_end];
                let Some(first) = span_detections.first() else {
                    continue;
                };
                let last = span_detections.last().expect("nonempty span has a last");
                if span_index > 0 {
                    stats.gaps += 1;
                }
                let start_sec = (first.frame_pts - first.sampling_interval / 2.0).max(0.0);
                let end_sec = (last.frame_pts + last.sampling_interval / 2.0).min(duration_secs);
                if end_sec <= start_sec {
                    // A degenerate tightened range (clamped at the bounds)
                    // keeps a point-in-time item rather than vanishing.
                    items.push(MediaLayerItem {
                        start_sec,
                        end_sec: None,
                        searchable_text: format!("object: {}", current.label),
                        payload: span_payload(
                            &track_id,
                            &current.label,
                            span_index,
                            span_count,
                            &current.detections,
                            span_detections,
                        ),
                        frame_asset_ids: Some(
                            span_detections
                                .iter()
                                .filter_map(|d| d.frame_asset_id.clone())
                                .collect(),
                        ),
                    });
                    stats.spans += 1;
                    continue;
                }
                items.push(MediaLayerItem {
                    start_sec,
                    end_sec: Some(end_sec),
                    searchable_text: format!("object: {}", current.label),
                    payload: span_payload(
                        &track_id,
                        &current.label,
                        span_index,
                        span_count,
                        &current.detections,
                        span_detections,
                    ),
                    frame_asset_ids: Some(
                        span_detections
                            .iter()
                            .filter_map(|d| d.frame_asset_id.clone())
                            .collect(),
                    ),
                });
                stats.spans += 1;
            }
            stats.tracks += 1;
        }
    }
    StitchResult { items, stats }
}

/// The span payload: track identity, the span's ordinal, and the span's
/// detections with their boxes (the addressable per-frame evidence).
fn span_payload(
    track_id: &str,
    label: &str,
    span_index: usize,
    span_count: usize,
    track_detections: &[Detection],
    span_detections: &[Detection],
) -> serde_json::Value {
    serde_json::json!({
        "label": label,
        "track_id": track_id,
        "span_index": span_index,
        "span_count": span_count,
        "track_detections_count": track_detections.len(),
        "detections": span_detections.iter().map(|d| serde_json::json!({
            "pts_sec": d.frame_pts,
            "box": {
                "x": d.r#box[0],
                "y": d.r#box[1],
                "w": d.r#box[2],
                "h": d.r#box[3],
            },
            "confidence": d.confidence,
            "frame_index": d.frame_index,
            "frame_asset_id": d.frame_asset_id,
            "source_item_id": d.source_item_id,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection(label: &str, pts: f64, r#box: [f64; 4], item: &str) -> Detection {
        Detection {
            label: label.to_string(),
            confidence: 0.9,
            r#box,
            frame_pts: pts,
            sampling_interval: 1.25,
            source_item_id: item.to_string(),
            frame_index: 0,
            frame_asset_id: Some(format!("asset-{label}-{pts}")),
        }
    }

    fn params() -> StitchParams {
        StitchParams {
            iou_threshold: DEFAULT_IOU_THRESHOLD,
            max_gap_secs: DEFAULT_MAX_GAP_SECS,
            cross_shot: false,
        }
    }

    #[test]
    fn box_iou_matches_overlapping_and_disjoint_boxes() {
        let overlap = box_iou(&[0.0, 0.0, 0.5, 0.5], &[0.25, 0.0, 0.5, 0.5]);
        assert!(
            (overlap - 0.25 / 0.75).abs() < 1e-9,
            "IoU is the ratio: {overlap}"
        );
        assert_eq!(box_iou(&[0.0, 0.0, 0.2, 0.2], &[0.8, 0.8, 0.2, 0.2]), 0.0);
    }

    #[test]
    fn same_label_consecutive_frames_stitch_into_one_track_one_span() {
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 2.5, [0.12, 0.1, 0.2, 0.2], "chunk-0"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(result.stats.tracks, 1);
        assert_eq!(result.stats.spans, 1);
        assert_eq!(result.stats.gaps, 0);
        assert_eq!(result.stats.unlinked_detections, 0);
        let item = &result.items[0];
        assert_eq!(item.searchable_text, "object: mug");
        // Tightened to the first/last PTS ± half the sampling interval.
        assert!((item.start_sec - 0.0).abs() < 1e-9);
        assert!((item.end_sec.expect("span end") - (2.5 + 0.625)).abs() < 1e-9);
        assert_eq!(item.payload["track_id"], serde_json::json!("mug-1"));
        assert_eq!(item.payload["detections"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn gap_within_max_gap_keeps_one_track_with_disjoint_spans() {
        // Adjacent samples, then an unobserved interval (1.9s — one sample
        // skipped, still within max_gap), then the object returns with an
        // overlapping box: one track, two disjoint spans, one counted gap.
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 3.15, [0.12, 0.1, 0.2, 0.2], "chunk-1"),
            detection("mug", 4.4, [0.13, 0.1, 0.2, 0.2], "chunk-1"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(result.stats.tracks, 1);
        assert_eq!(result.stats.spans, 2);
        assert_eq!(
            result.stats.gaps, 1,
            "the occlusion gap is visible, never smoothed"
        );
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].payload["span_index"], serde_json::json!(0));
        assert_eq!(result.items[1].payload["span_index"], serde_json::json!(1));
        assert_eq!(result.items[0].payload["span_count"], serde_json::json!(2));
    }

    #[test]
    fn gap_beyond_max_gap_ends_the_track() {
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 8.0, [0.12, 0.1, 0.2, 0.2], "chunk-2"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        // The first two detections keep one track; the 6.75s gap ends it,
        // and the lone return is a singleton — no track is invented.
        assert_eq!(result.stats.tracks, 1, "the >max_gap return ends the track");
        assert_eq!(
            result.stats.unlinked_detections, 1,
            "the lone return is unlinked"
        );
        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0].payload["track_id"],
            serde_json::json!("mug-1")
        );
    }

    #[test]
    fn singleton_detections_stay_unlinked_no_track_invented() {
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("chair", 1.25, [0.5, 0.5, 0.3, 0.3], "chunk-0"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(result.stats.tracks, 0);
        assert_eq!(
            result.stats.unlinked_detections, 2,
            "each singleton is named in stats"
        );
        assert!(result.items.is_empty(), "no track is invented");
    }

    #[test]
    fn iou_failure_fragments_the_track() {
        let detections = [
            detection("person", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            // A different person at the same moment — disjoint box, no link.
            detection("person", 1.25, [0.7, 0.7, 0.2, 0.2], "chunk-0"),
            detection("person", 2.5, [0.71, 0.7, 0.2, 0.2], "chunk-0"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(
            result.stats.tracks, 1,
            "detections 2-3 link; 1 is a singleton"
        );
        assert_eq!(result.stats.unlinked_detections, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0].payload["track_id"],
            serde_json::json!("person-1")
        );
    }

    #[test]
    fn cross_shot_false_refuses_shot_boundary_links_when_source_is_shot_chunked() {
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "shot-0"),
            detection("mug", 1.25, [0.1, 0.1, 0.2, 0.2], "shot-1"),
        ];
        let result = stitch(&detections, params(), true, 10.0);
        assert_eq!(
            result.stats.shot_fragments, 1,
            "the blocked link is counted"
        );
        assert_eq!(result.stats.tracks, 0, "cross_shot is off by default");
        let merged = stitch(
            &detections,
            StitchParams {
                cross_shot: true,
                ..params()
            },
            true,
            10.0,
        );
        assert_eq!(
            merged.stats.tracks, 1,
            "cross_shot=true merges across shots"
        );
        assert_eq!(merged.stats.shot_fragments, 0);
    }

    #[test]
    fn cross_shot_is_inert_for_time_range_sources() {
        // Time-range chunking has no shot structure — cross_shot=false
        // must not block links across source items.
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-1"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(result.stats.shot_fragments, 0);
        assert_eq!(result.stats.tracks, 1);
    }

    #[test]
    fn labels_match_case_insensitively_and_keep_source_case() {
        let detections = [
            detection("Red Mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("red mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-0"),
        ];
        let result = stitch(&detections, params(), false, 10.0);
        assert_eq!(
            result.stats.tracks, 1,
            "same object under case variance links"
        );
        assert_eq!(result.items[0].searchable_text, "object: Red Mug");
        assert_eq!(
            result.items[0].payload["track_id"],
            serde_json::json!("red-mug-1")
        );
    }

    #[test]
    fn tightened_ranges_clamp_to_media_bounds() {
        let detections = [
            detection("mug", 0.0, [0.1, 0.1, 0.2, 0.2], "chunk-0"),
            detection("mug", 1.25, [0.11, 0.1, 0.2, 0.2], "chunk-0"),
        ];
        // A duration shorter than the last PTS + half-interval clamps the end.
        let result = stitch(&detections, params(), false, 1.5);
        assert_eq!(result.stats.spans, 1);
        let item = &result.items[0];
        assert_eq!(
            item.end_sec,
            Some(1.5),
            "the end clamps to the duration anchor"
        );
        assert!((item.start_sec - 0.0).abs() < 1e-9);
    }
}
