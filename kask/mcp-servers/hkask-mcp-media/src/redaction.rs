//! Redaction as a composed edit — S3 of the visual-event-tagging design
//! (`kask/docs/research/visual-event-tagging-design.md` §6, with §3 layer
//! contracts and §7 human enablement).
//!
//! A redaction decision is a researcher-authored layer (kind `researcher`,
//! provenance `model: "user"` — a human act, never machine-authored) whose
//! items carry the redaction vocabulary:
//!
//! ```json
//! {
//!   "start_sec": 12.4, "end_sec": 17.6,
//!   "searchable_text": "redact: person (blur)",
//!   "payload": {
//!     "op": "redact", "tier": "ffmpeg", "effect": "blur|pixelate|black|delogo",
//!     "target": { "kind": "box", "box": {"x":0.12,"y":0.33,"w":0.41,"h":0.52} },
//!     "source_tag_refs": ["layer_abc/item_9f2c"],
//!     "status": "approved"
//!   }
//! }
//! ```
//!
//! `target.kind = "mask_track"` (referencing a stitcher `track_id`) is
//! reserved vocabulary for tier 2: representable now, renderable once tier 2
//! exists, so no store migration is owed later. Requesting a tier-1 render of
//! a mask_track target fails as `permission_denied` naming the serving path
//! (the postponed SAM-3 mask → removal pipeline) — the `NotConfigured`
//! precedent (`tools/layers.rs` `map_layer_pass_error`).
//!
//! The tier-1 render is segment-planned: only affected segments re-encode
//! through the ffmpeg filter graph, untouched segments stream-copy. Every
//! segment boundary lands on a video keyframe — ffmpeg's stream-copy read
//! starts at the previous keyframe and `between`-gated filters ride the
//! re-encoding segment's local timeline — so the plan tiles `[0, duration]`
//! exactly: the rendered timeline is source-aligned, with no unredacted
//! leak, no duplicated frames, and no duration drift (all pinned empirically
//! on ffmpeg 8.0.1, 2026-10-29, and by the S3 behavior tests).

use crate::layer_store::StoredMediaLayerItem;

/// Provenance model value for researcher-authored layers — the human-act
/// marker. A redaction decision layer whose provenance model differs is
/// machine-authored and never a renderable decision (design §6: "a human
/// act, never machine-authored").
pub(crate) const RESEARCHER_MODEL: &str = "user";

/// The cover effect applied over the target region (design §6: blur →
/// `gblur`, pixelate → `pixelize`, black → `drawbox` filled black, delogo →
/// `delogo`). All four filters carry the timeline flag on the local ffmpeg
/// (empirical `ffmpeg -filters`, 2026-10-29), so each is time-gated by
/// `enable='between(t,…)'` in the re-encoding segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedactionEffect {
    Blur,
    Pixelate,
    Black,
    Delogo,
}

impl RedactionEffect {
    pub(crate) const VALID_VALUES: &'static [&'static str] =
        &["blur", "pixelate", "black", "delogo"];

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            RedactionEffect::Blur => "blur",
            RedactionEffect::Pixelate => "pixelate",
            RedactionEffect::Black => "black",
            RedactionEffect::Delogo => "delogo",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "blur" => Some(RedactionEffect::Blur),
            "pixelate" => Some(RedactionEffect::Pixelate),
            "black" => Some(RedactionEffect::Black),
            "delogo" => Some(RedactionEffect::Delogo),
            _ => None,
        }
    }
}

/// What a decision item targets (design §6): a normalized box, or a stitcher
/// track — the latter reserved for tier 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedactionTargetKind {
    Box,
    MaskTrack,
}

impl RedactionTargetKind {
    pub(crate) const VALID_VALUES: &'static [&'static str] = &["box", "mask_track"];

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            RedactionTargetKind::Box => "box",
            RedactionTargetKind::MaskTrack => "mask_track",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "box" => Some(RedactionTargetKind::Box),
            "mask_track" => Some(RedactionTargetKind::MaskTrack),
            _ => None,
        }
    }
}

/// One approved redaction decision parsed from a stored researcher-layer
/// item. Field invariants are enforced by [`parse_decision`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RedactionDecision {
    pub(crate) item_id: String,
    pub(crate) start_sec: f64,
    pub(crate) end_sec: f64,
    pub(crate) effect: RedactionEffect,
    pub(crate) target_kind: RedactionTargetKind,
    /// Normalized `x, y, w, h` in 0..=1 with `x+w <= 1`, `y+h <= 1` — present
    /// iff the target kind is `Box`.
    pub(crate) box_norm: Option<[f64; 4]>,
    /// Stitcher `track_id` — present iff the target kind is `MaskTrack`
    /// (reserved tier-2 vocabulary; never renderable in tier 1).
    pub(crate) track_id: Option<String>,
    /// Originating tag provenance (OMC lineage discipline) — the referenced
    /// tag items' stable ids.
    pub(crate) source_tag_refs: Vec<String>,
}

/// Why a stored item is not a renderable tier-1 redaction decision —
/// per-variant classification for the tool layer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RedactionDecisionError {
    /// Not an approved, well-formed redaction decision: `failed_precondition`
    /// naming the layer and item.
    NotARedactionDecision(String),
    /// `target.kind = mask_track` — reserved tier-2 vocabulary with no
    /// configured serving provider: `permission_denied` naming the serving
    /// path.
    Tier2NotConfigured,
}

impl std::fmt::Display for RedactionDecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotARedactionDecision(reason) => f.write_str(reason),
            Self::Tier2NotConfigured => f.write_str(
                "mask_track target — reserved tier-2 vocabulary with no configured \
                 serving provider; tier-1 renders cover box targets only",
            ),
        }
    }
}

/// Tolerance for floating comparisons in the range/boundary arithmetic.
pub(crate) const RANGE_EPS: f64 = 1e-6;

/// Parse one stored researcher-layer item into a decision. Every rejection
/// names the layer and the item. The store validated item ranges against its
/// own probe at store time; the render re-probes, so the freshly probed
/// duration re-bounds every range here — a media file replaced on disk since
/// the decision was authored degrades visibly, never silently.
pub(crate) fn parse_decision(
    layer_id: &str,
    item: &StoredMediaLayerItem,
    duration_secs: f64,
) -> Result<RedactionDecision, RedactionDecisionError> {
    let reason = |what: String| {
        RedactionDecisionError::NotARedactionDecision(format!(
            "redaction decision layer {layer_id} item {}: {what}",
            item.id
        ))
    };
    let Some(payload) = item.item.payload.as_object() else {
        return Err(reason("payload is not an object".to_string()));
    };
    let op = payload.get("op").and_then(serde_json::Value::as_str);
    if op != Some("redact") {
        return Err(reason(format!("payload op is {op:?}, expected \"redact\"")));
    }
    let status = payload.get("status").and_then(serde_json::Value::as_str);
    if status != Some("approved") {
        return Err(reason(format!(
            "status is {status:?}, not \"approved\" — no redaction renders \
             without an approved decision"
        )));
    }
    let Some(end_sec) = item.item.end_sec else {
        return Err(reason(
            "a redaction decision requires a range end (end_sec)".to_string(),
        ));
    };
    if end_sec <= item.item.start_sec {
        return Err(reason(format!(
            "range is reversed or empty: {}..{}",
            item.item.start_sec, end_sec
        )));
    }
    if end_sec > duration_secs + RANGE_EPS {
        return Err(reason(format!(
            "range end {end_sec} is beyond the probed media duration \
             {duration_secs}"
        )));
    }
    let effect = payload
        .get("effect")
        .and_then(serde_json::Value::as_str)
        .and_then(RedactionEffect::parse)
        .ok_or_else(|| {
            reason(format!(
                "effect must be one of {}, got {:?}",
                RedactionEffect::VALID_VALUES.join("|"),
                payload.get("effect").and_then(serde_json::Value::as_str)
            ))
        })?;
    let target = payload
        .get("target")
        .ok_or_else(|| reason("payload is missing the target object".to_string()))?;
    let target_kind = target
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .and_then(RedactionTargetKind::parse)
        .ok_or_else(|| {
            reason(format!(
                "target.kind must be one of {}, got {:?}",
                RedactionTargetKind::VALID_VALUES.join("|"),
                target.get("kind").and_then(serde_json::Value::as_str)
            ))
        })?;
    let (box_norm, track_id) = match target_kind {
        RedactionTargetKind::Box => {
            let r#box = target.get("box").ok_or_else(|| {
                reason("target.kind is box but target.box is missing".to_string())
            })?;
            let mut values = [0.0; 4];
            for (slot, key) in values.iter_mut().zip(["x", "y", "w", "h"]) {
                *slot = r#box
                    .get(key)
                    .and_then(serde_json::Value::as_f64)
                    .ok_or_else(|| reason(format!("target.box.{key} is not a number")))?;
            }
            let [x, y, w, h] = values;
            for (name, value) in [("x", x), ("y", y), ("w", w), ("h", h)] {
                if !value.is_finite() || value < 0.0 || value > 1.0 {
                    return Err(reason(format!(
                        "target.box.{name} is {value}, outside 0..=1"
                    )));
                }
            }
            if w <= 0.0 || h <= 0.0 {
                return Err(reason(format!("target.box is empty: {w}x{h}")));
            }
            if x + w > 1.0 + RANGE_EPS || y + h > 1.0 + RANGE_EPS {
                return Err(reason(format!(
                    "target.box extends beyond the frame: x+w = {}, y+h = {}",
                    x + w,
                    y + h
                )));
            }
            (Some(values), None)
        }
        // Reserved tier-2 vocabulary (design §6): representable in the store
        // now, renderable once tier 2 exists. A well-formed mask_track item
        // parses (so the vocabulary round-trips the store without migration)
        // and is refused below, before any ffmpeg work — the tier-2 serving
        // path is not configured.
        RedactionTargetKind::MaskTrack => match target
            .get("track_id")
            .and_then(serde_json::Value::as_str)
            .filter(|track_id| !track_id.trim().is_empty())
        {
            Some(track_id) => (None, Some(track_id.to_string())),
            None => {
                return Err(reason(
                    "target.kind is mask_track but target.track_id is \
                     missing or blank"
                        .to_string(),
                ));
            }
        },
    };
    if target_kind == RedactionTargetKind::MaskTrack {
        return Err(RedactionDecisionError::Tier2NotConfigured);
    }
    let source_tag_refs = match payload.get("source_tag_refs") {
        None => Vec::new(),
        Some(value) => match value.as_array() {
            Some(array) => array
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_string).ok_or_else(|| {
                        reason("source_tag_refs entries must be strings".to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => {
                return Err(reason(
                    "source_tag_refs must be an array of tag item ids".to_string(),
                ));
            }
        },
    };
    Ok(RedactionDecision {
        item_id: item.id.clone(),
        start_sec: item.item.start_sec,
        end_sec,
        effect,
        target_kind,
        box_norm,
        track_id,
        source_tag_refs,
    })
}

/// How a plan segment renders: the affected segments re-encode through the
/// filter graph; untouched segments stream-copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentMode {
    StreamCopy,
    Reencode,
}

impl SegmentMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            SegmentMode::StreamCopy => "stream_copy",
            SegmentMode::Reencode => "reencode",
        }
    }
}

/// One segment of the render plan. The plan tiles `[0, duration]` exactly:
/// segments are contiguous in source time, ordered, and non-degenerate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlanSegment {
    pub(crate) mode: SegmentMode,
    pub(crate) start_sec: f64,
    pub(crate) end_sec: f64,
    /// Decision item ids applied inside this segment (re-encode segments).
    pub(crate) item_ids: Vec<String>,
}

/// Touching spans merge at this tolerance — the t8 compose tolerance.
pub(crate) const MERGE_TOLERANCE: f64 = 1e-9;

/// The largest keyframe time at or before `t` — the floor-aligned boundary.
/// Frame 0 is a keyframe in any decodable stream, so the floor always
/// exists; an empty enumeration degrades to 0.0.
fn floor_keyframe(keyframe_times: &[f64], t: f64) -> f64 {
    keyframe_times
        .iter()
        .copied()
        .filter(|k| *k <= t)
        .fold(0.0, f64::max)
}

/// The smallest keyframe time at or after `t` (within the match tolerance),
/// or `None` past the last keyframe — the ceil-aligned boundary.
fn ceil_keyframe(keyframe_times: &[f64], t: f64) -> Option<f64> {
    keyframe_times
        .iter()
        .copied()
        .filter(|k| *k >= t - RANGE_EPS)
        .fold(None, |acc: Option<f64>, k| {
            Some(match acc {
                Some(existing) => existing.min(k),
                None => k,
            })
        })
}

/// Build the render plan: re-encode segments covering the merged decision
/// spans, keyframe-aligned on both sides, with stream-copy segments tiling
/// the complement. Keyframe alignment is load-bearing, not cosmetic:
///
/// - Stream-copy reads start at the previous keyframe, so a copy segment
///   starting mid-GOP would prepend unredacted frames from the redacted
///   span (empirically pinned: `-ss 2.5 -to 3.5 -c copy` on a 1s-GOP source
///   yields the 1.5s `[2.0, 3.5]`, ffmpeg 8.0.1, 2026-10-29).
/// - A re-encoding segment cut mid-GOP leaves straddling B-frames that the
///   copy read cannot bound (`-to` overshoots by the reordering depth),
///   duplicating redacted-range frames into the output.
///
/// With every boundary on a keyframe the segments tile exactly: the output
/// timeline matches the source, the untouched segments are bit-stable stream
/// copies, and only the affected neighborhoods re-encode. Degradation is
/// always toward more re-encoding, never toward an unsafe copy: a span with
/// no keyframe past its end extends to the duration, and an empty keyframe
/// enumeration floors every span to 0 — a whole-file re-encode, visible in
/// the plan as a single re-encode segment. Touching spans re-merge after
/// alignment, so no degenerate copy slivers are ever emitted.
pub(crate) fn build_segment_plan(
    decisions: &[RedactionDecision],
    duration_secs: f64,
    keyframe_times: &[f64],
) -> Vec<PlanSegment> {
    // Union-merge the decision spans in time order, carrying decision
    // indexes so each merged span knows the items it covers. `decisions`
    // arrive in stored order, not time order — collect, sort, then merge so
    // the union is order-independent.
    let mut spans: Vec<(f64, f64, usize)> = decisions
        .iter()
        .enumerate()
        .map(|(index, decision)| {
            (
                decision.start_sec.max(0.0),
                decision.end_sec.min(duration_secs),
                index,
            )
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Vec<(f64, f64, Vec<usize>)> = Vec::new();
    for (start, end, index) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 + MERGE_TOLERANCE => {
                last.1 = last.1.max(end);
                last.2.push(index);
            }
            _ => merged.push((start, end, vec![index])),
        }
    }

    // Keyframe-align each merged span on both sides; a span whose end passes
    // the last keyframe extends to the duration (no trailing copy segment
    // could start safely there).
    let filter_spans: Vec<(f64, f64, Vec<usize>)> = merged
        .into_iter()
        .map(|(start, end, indexes)| {
            let aligned_start = floor_keyframe(keyframe_times, start);
            let aligned_end = match ceil_keyframe(keyframe_times, end) {
                Some(k) => k.min(duration_secs),
                None => duration_secs,
            }
            .max(end);
            (aligned_start, aligned_end, indexes)
        })
        .collect();

    // Re-merge: alignment can push neighbors into contact or overlap.
    let mut aligned: Vec<(f64, f64, Vec<usize>)> = Vec::new();
    for (start, end, indexes) in filter_spans {
        match aligned.last_mut() {
            Some(last) if start <= last.1 + MERGE_TOLERANCE => {
                last.1 = last.1.max(end);
                last.2.extend(indexes);
            }
            _ => aligned.push((start, end, indexes)),
        }
    }

    // Interleave stream-copy segments so the plan tiles [0, duration].
    let mut plan: Vec<PlanSegment> = Vec::new();
    let mut cursor = 0.0;
    for (start, end, indexes) in &aligned {
        if start - cursor > MERGE_TOLERANCE {
            plan.push(PlanSegment {
                mode: SegmentMode::StreamCopy,
                start_sec: cursor,
                end_sec: *start,
                item_ids: Vec::new(),
            });
        }
        plan.push(PlanSegment {
            mode: SegmentMode::Reencode,
            start_sec: *start,
            end_sec: *end,
            item_ids: indexes
                .iter()
                .map(|index| decisions[*index].item_id.clone())
                .collect(),
        });
        cursor = *end;
    }
    if duration_secs - cursor > MERGE_TOLERANCE {
        plan.push(PlanSegment {
            mode: SegmentMode::StreamCopy,
            start_sec: cursor,
            end_sec: duration_secs,
            item_ids: Vec::new(),
        });
    }
    plan
}

/// Gaussian blur sigma for the blur effect — deterministic, named in the
/// graph and recorded in the applied-item details.
pub(crate) const BLUR_SIGMA: f64 = 10.0;

/// The encoder arguments a re-encoded segment uses for a given source
/// codec. Re-encoded segments keep the source's codec family so the concat
/// demuxer's uniform-stream contract holds (empirically pinned 2026-10-29:
/// a mixed-codec concat misdecodes the re-encoded middle segments
/// entirely). The h264 arm repeats in-band parameter sets
/// (`repeat-headers`) so the concatenated output re-inits the decoder
/// where the re-encoded segments begin. An unmapped codec is a visible,
/// typed degradation — never a silent mixed-codec concat.
pub(crate) fn tier1_encoder_args(codec: &str) -> Option<&'static [&'static str]> {
    match codec {
        "h264" => Some(&["-c:v", "libx264", "-x264-params", "repeat-headers=1"]),
        "mpeg4" => Some(&["-c:v", "mpeg4"]),
        _ => None,
    }
}

/// Pixelate block size: one eighth of the box's smaller pixel side, min 2.
const PIXELATE_DIVISOR: u32 = 8;

/// Delogo interpolates from the box border, so the box must sit strictly
/// inside the frame; boxes touching an edge are inset by one pixel.
const DELOGO_EDGE_INSET: u32 = 1;

/// A box scaled to probed frame pixels — rounded, clamped inside the frame,
/// never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

pub(crate) fn scale_box_to_pixels(box_norm: &[f64; 4], width: u32, height: u32) -> PixelRect {
    let [x, y, w, h] = box_norm;
    let w = ((w * f64::from(width)).round() as u32).clamp(1, width.max(1));
    let h = ((h * f64::from(height)).round() as u32).clamp(1, height.max(1));
    let x = ((x * f64::from(width)).round() as u32).min(width.saturating_sub(w));
    let y = ((y * f64::from(height)).round() as u32).min(height.saturating_sub(h));
    PixelRect { x, y, w, h }
}

/// One decision's applied rendering details — surfaced in the render result
/// so the review surface (and the S5 panel) can show exactly what was drawn
/// where: deterministic adjustments are named, never silent.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct AppliedRedaction {
    pub(crate) item_id: String,
    pub(crate) effect: &'static str,
    pub(crate) pixel_x: u32,
    pub(crate) pixel_y: u32,
    pub(crate) pixel_w: u32,
    pub(crate) pixel_h: u32,
    /// The item's enable window on the re-encoding segment's local timeline.
    pub(crate) local_start_sec: f64,
    pub(crate) local_end_sec: f64,
    /// Delogo only: the box was inset for the frame-edge interpolation band.
    pub(crate) delogo_edge_inset: bool,
}

/// Build the ffmpeg `filter_complex` graph for one re-encoding segment:
/// every decision item in the segment, time-gated by
/// `enable='between(t,start,end)'` over the item's local window and spatial
/// over the normalized box scaled to the probed frame dimensions.
///
/// Black fills via `drawbox` (region-native, timeline-native); delogo
/// interpolates in place; blur and pixelate are whole-frame filters, so the
/// region is cropped, filtered, and overlaid back — the overlay carries the
/// enable window. The graph terminates in `[vout]`.
pub(crate) fn build_filter_graph(
    decisions: &[&RedactionDecision],
    segment_start_sec: f64,
    width: u32,
    height: u32,
) -> Result<(String, Vec<AppliedRedaction>), RedactionDecisionError> {
    let mut graph = String::new();
    let mut current = "0:v".to_string();
    let mut applied = Vec::new();
    for (ordinal, decision) in decisions.iter().enumerate() {
        if decision.target_kind != RedactionTargetKind::Box {
            return Err(RedactionDecisionError::NotARedactionDecision(format!(
                "item {}: target kind {} is not box-renderable in tier 1",
                decision.item_id,
                decision.target_kind.as_str()
            )));
        }
        let Some(box_norm) = decision.box_norm else {
            return Err(RedactionDecisionError::NotARedactionDecision(format!(
                "item {}: missing the normalized box",
                decision.item_id
            )));
        };
        let mut rect = scale_box_to_pixels(&box_norm, width, height);
        let local_start = (decision.start_sec - segment_start_sec).max(0.0);
        let local_end = (decision.end_sec - segment_start_sec).max(local_start);
        let window = format!("between(t,{:.3},{:.3})", local_start, local_end);
        let out_label = format!("r{ordinal}");
        let mut delogo_edge_inset = false;
        match decision.effect {
            RedactionEffect::Black => {
                graph.push_str(&format!(
                    "[{current}]drawbox=x={x}:y={y}:w={w}:h={h}:color=black@1:t=fill:\
                     enable='{window}'[{out_label}];",
                    x = rect.x,
                    y = rect.y,
                    w = rect.w,
                    h = rect.h
                ));
            }
            RedactionEffect::Delogo => {
                // Delogo interpolates from the box border; a box touching a
                // frame edge has no interpolation band, so it is inset by
                // one pixel — a named adjustment, surfaced in the details.
                let (mut dx, mut dy) = (rect.x, rect.y);
                let (mut dw, mut dh) = (rect.w, rect.h);
                if dx == 0 {
                    dx += DELOGO_EDGE_INSET;
                    dw = dw.saturating_sub(DELOGO_EDGE_INSET);
                    delogo_edge_inset = true;
                }
                if dy == 0 {
                    dy += DELOGO_EDGE_INSET;
                    dh = dh.saturating_sub(DELOGO_EDGE_INSET);
                    delogo_edge_inset = true;
                }
                if dx + dw >= width {
                    dw = dw.saturating_sub(DELOGO_EDGE_INSET);
                    delogo_edge_inset = true;
                }
                if dy + dh >= height {
                    dh = dh.saturating_sub(DELOGO_EDGE_INSET);
                    delogo_edge_inset = true;
                }
                if dw < 2 || dh < 2 {
                    return Err(RedactionDecisionError::NotARedactionDecision(format!(
                        "item {}: the delogo box is too small after the frame-edge \
                         inset ({}x{} at {},{}) — enlarge the target box",
                        decision.item_id, dw, dh, dx, dy
                    )));
                }
                rect = PixelRect {
                    x: dx,
                    y: dy,
                    w: dw,
                    h: dh,
                };
                graph.push_str(&format!(
                    "[{current}]delogo=x={x}:y={y}:w={w}:h={h}:enable='{window}'[{out_label}];",
                    x = rect.x,
                    y = rect.y,
                    w = rect.w,
                    h = rect.h
                ));
            }
            RedactionEffect::Blur => {
                let region_label = format!("b{ordinal}");
                let filter_label = format!("f{ordinal}");
                let cropped_label = format!("c{ordinal}");
                graph.push_str(&format!(
                    "[{current}]split=2[{region_label}][{filter_label}];\
                     [{filter_label}]crop={w}:{h}:{x}:{y},gblur=sigma={sigma}\
                     [{cropped_label}];[{region_label}][{cropped_label}]\
                     overlay={x}:{y}:enable='{window}'[{out_label}];",
                    w = rect.w,
                    h = rect.h,
                    x = rect.x,
                    y = rect.y,
                    sigma = BLUR_SIGMA
                ));
            }
            RedactionEffect::Pixelate => {
                let block_w = (rect.w / PIXELATE_DIVISOR).max(2).min(rect.w.max(1));
                let block_h = (rect.h / PIXELATE_DIVISOR).max(2).min(rect.h.max(1));
                let region_label = format!("b{ordinal}");
                let filter_label = format!("f{ordinal}");
                let cropped_label = format!("c{ordinal}");
                graph.push_str(&format!(
                    "[{current}]split=2[{region_label}][{filter_label}];\
                     [{filter_label}]crop={w}:{h}:{x}:{y},\
                     pixelize=w={bw}:h={bh}:mode=avg\
                     [{cropped_label}];[{region_label}][{cropped_label}]\
                     overlay={x}:{y}:enable='{window}'[{out_label}];",
                    w = rect.w,
                    h = rect.h,
                    x = rect.x,
                    y = rect.y,
                    bw = block_w,
                    bh = block_h
                ));
            }
        }
        current = out_label;
        applied.push(AppliedRedaction {
            item_id: decision.item_id.clone(),
            effect: decision.effect.as_str(),
            pixel_x: rect.x,
            pixel_y: rect.y,
            pixel_w: rect.w,
            pixel_h: rect.h,
            local_start_sec: local_start,
            local_end_sec: local_end,
            delogo_edge_inset,
        });
    }
    // The graph always terminates in [vout]; an empty decision list (which
    // the plan never produces) still yields a valid pass-through.
    graph.push_str(&format!("[{current}]null[vout]"));
    Ok((graph, applied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_store::MediaLayerItem;

    fn stored_item(
        id: &str,
        start: f64,
        end: Option<f64>,
        payload: serde_json::Value,
    ) -> StoredMediaLayerItem {
        StoredMediaLayerItem {
            id: id.to_string(),
            layer_id: "layer_1".to_string(),
            item: MediaLayerItem {
                start_sec: start,
                end_sec: end,
                searchable_text: "redact: fixture".to_string(),
                payload,
                frame_asset_ids: None,
            },
        }
    }

    fn approved_payload(effect: &str, box_json: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "op": "redact",
            "tier": "ffmpeg",
            "effect": effect,
            "target": {"kind": "box", "box": box_json},
            "source_tag_refs": ["layer_abc/item_9f2c"],
            "status": "approved",
        })
    }

    #[test]
    fn parses_an_approved_box_decision() {
        let item = stored_item(
            "item_1",
            12.4,
            Some(17.6),
            approved_payload(
                "blur",
                serde_json::json!({"x": 0.12, "y": 0.33, "w": 0.41, "h": 0.52}),
            ),
        );
        let decision = parse_decision("layer_1", &item, 120.0).expect("approved decision parses");
        assert_eq!(decision.item_id, "item_1");
        assert_eq!(decision.effect, RedactionEffect::Blur);
        assert_eq!(decision.target_kind, RedactionTargetKind::Box);
        assert_eq!(decision.box_norm, Some([0.12, 0.33, 0.41, 0.52]));
        assert_eq!(decision.source_tag_refs, vec!["layer_abc/item_9f2c"]);
    }

    #[test]
    fn rejects_unapproved_and_malformed_decisions_naming_the_item() {
        let duration = 120.0;
        let cases = [
            (
                stored_item(
                    "i_pending",
                    1.0,
                    Some(2.0),
                    serde_json::json!({"op": "redact", "status": "pending"}),
                ),
                "status",
            ),
            (
                stored_item(
                    "i_op",
                    1.0,
                    Some(2.0),
                    serde_json::json!({"op": "highlight", "status": "approved"}),
                ),
                "op",
            ),
            (
                stored_item(
                    "i_end",
                    1.0,
                    None,
                    approved_payload(
                        "black",
                        serde_json::json!({"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}),
                    ),
                ),
                "end_sec",
            ),
            (
                stored_item(
                    "i_reversed",
                    2.0,
                    Some(1.0),
                    approved_payload(
                        "black",
                        serde_json::json!({"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}),
                    ),
                ),
                "reversed",
            ),
            (
                stored_item(
                    "i_beyond",
                    1.0,
                    Some(300.0),
                    approved_payload(
                        "black",
                        serde_json::json!({"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}),
                    ),
                ),
                "beyond the probed media duration",
            ),
            (
                stored_item(
                    "i_effect",
                    1.0,
                    Some(2.0),
                    approved_payload(
                        "vaporize",
                        serde_json::json!({"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}),
                    ),
                ),
                "effect",
            ),
            (
                stored_item(
                    "i_target_kind",
                    1.0,
                    Some(2.0),
                    serde_json::json!({
                        "op": "redact", "status": "approved", "effect": "black",
                        "target": {"kind": "blob"}
                    }),
                ),
                "target.kind",
            ),
            (
                stored_item(
                    "i_box_beyond",
                    1.0,
                    Some(2.0),
                    approved_payload(
                        "black",
                        serde_json::json!({"x": 0.9, "y": 0.0, "w": 0.5, "h": 0.5}),
                    ),
                ),
                "beyond the frame",
            ),
            (
                stored_item(
                    "i_box_missing",
                    1.0,
                    Some(2.0),
                    serde_json::json!({
                        "op": "redact", "status": "approved", "effect": "black",
                        "target": {"kind": "box"}
                    }),
                ),
                "target.box",
            ),
            (
                stored_item(
                    "i_refs",
                    1.0,
                    Some(2.0),
                    serde_json::json!({
                        "op": "redact", "status": "approved", "effect": "black",
                        "target": {"kind": "box",
                                   "box": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5}},
                        "source_tag_refs": "not-an-array"
                    }),
                ),
                "source_tag_refs",
            ),
        ];
        for (item, needle) in cases {
            let error = parse_decision("layer_1", &item, duration)
                .expect_err("the malformed decision rejects");
            match error {
                RedactionDecisionError::NotARedactionDecision(reason) => {
                    assert!(reason.contains(needle), "reason names {needle:?}: {reason}");
                    assert!(
                        reason.contains("layer_1"),
                        "reason names the layer: {reason}"
                    );
                    assert!(reason.contains(&item.id), "reason names the item: {reason}");
                }
                RedactionDecisionError::Tier2NotConfigured => {
                    panic!("expected a named rejection, got the tier-2 class");
                }
            }
        }
    }

    #[test]
    fn mask_track_is_representable_but_not_tier1_renderable() {
        // Reserved tier-2 vocabulary: parseable presence, refused at render.
        let item = stored_item(
            "i_track",
            1.0,
            Some(2.0),
            serde_json::json!({
                "op": "redact", "status": "approved", "effect": "blur",
                "target": {"kind": "mask_track", "track_id": "track_7"}
            }),
        );
        let error = parse_decision("layer_1", &item, 120.0)
            .expect_err("a mask_track decision is not tier-1 renderable");
        assert_eq!(error, RedactionDecisionError::Tier2NotConfigured);
        // A mask_track target without a track_id is malformed vocabulary,
        // not a tier-2 request.
        let item = stored_item(
            "i_trackless",
            1.0,
            Some(2.0),
            serde_json::json!({
                "op": "redact", "status": "approved", "effect": "blur",
                "target": {"kind": "mask_track"}
            }),
        );
        let error =
            parse_decision("layer_1", &item, 120.0).expect_err("trackless mask_track rejects");
        assert!(
            matches!(error, RedactionDecisionError::NotARedactionDecision(_)),
            "track_id-less mask_track is a named rejection, not tier-2: {error:?}"
        );
    }

    fn decision(id: &str, start: f64, end: f64) -> RedactionDecision {
        RedactionDecision {
            item_id: id.to_string(),
            start_sec: start,
            end_sec: end,
            effect: RedactionEffect::Black,
            target_kind: RedactionTargetKind::Box,
            box_norm: Some([0.25, 0.25, 0.5, 0.5]),
            track_id: None,
            source_tag_refs: Vec::new(),
        }
    }

    fn modes(plan: &[PlanSegment]) -> Vec<(&str, f64, f64)> {
        plan.iter()
            .map(|segment| (segment.mode.as_str(), segment.start_sec, segment.end_sec))
            .collect()
    }

    #[test]
    fn plan_tiles_keyframe_aligned_segments() {
        // Keyframes every 1s on a 4s source; one redaction [1.2, 1.8].
        let decisions = [decision("d1", 1.2, 1.8)];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![
                ("stream_copy", 0.0, 1.0),
                ("reencode", 1.0, 2.0),
                ("stream_copy", 2.0, 4.0),
            ],
            "floor/ceil keyframe alignment tiles the duration exactly"
        );
        assert_eq!(plan[1].item_ids, vec!["d1".to_string()]);
    }

    #[test]
    fn plan_extends_past_the_last_keyframe_without_a_trailing_copy() {
        let decisions = [decision("d1", 3.2, 3.8)];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![("stream_copy", 0.0, 3.0), ("reencode", 3.0, 4.0)],
            "no keyframe past the span end — the filter segment extends to \
             the duration so no trailing copy starts mid-GOP"
        );
    }

    #[test]
    fn plan_merges_overlapping_and_keeps_a_copy_segment_between() {
        // d1/d2 overlap (1.2..1.8, 1.4..2.0) and merge; alignment floors the
        // merged span to keyframe 1.0 and ceils it to 2.0. d3 (3.2..3.6)
        // floors to 3.0 and, with no keyframe past 3.6, extends to the
        // duration — exercising the mid copy segment and the tail
        // extension in one plan.
        let decisions = [
            decision("d1", 1.2, 1.8),
            decision("d2", 1.4, 2.0),
            decision("d3", 3.2, 3.6),
        ];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![
                ("stream_copy", 0.0, 1.0),
                ("reencode", 1.0, 2.0),
                ("stream_copy", 2.0, 3.0),
                ("reencode", 3.0, 4.0),
            ],
            "lead copy, merged reencode, mid copy, tail reencode"
        );
        let mut first_items = plan[1].item_ids.clone();
        first_items.sort();
        assert_eq!(first_items, vec!["d1".to_string(), "d2".to_string()]);
        assert_eq!(plan[3].item_ids, vec!["d3".to_string()]);
    }

    #[test]
    fn plan_alignment_merges_touching_filtered_spans() {
        // Two spans separated by a sub-keyframe gap: alignment pulls the
        // first to [1.0, 2.0] and the second to [2.0, 3.0] — they touch and
        // re-merge, so no degenerate copy sliver is emitted.
        let decisions = [decision("d1", 1.2, 1.5), decision("d2", 2.4, 2.8)];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![
                ("stream_copy", 0.0, 1.0),
                ("reencode", 1.0, 3.0),
                ("stream_copy", 3.0, 4.0),
            ]
        );
        let mut items = plan[1].item_ids.clone();
        items.sort();
        assert_eq!(items, vec!["d1".to_string(), "d2".to_string()]);
    }

    #[test]
    fn plan_without_keyframes_degrades_to_a_whole_file_reencode() {
        // A source with no enumerated keyframes: every copy start would be
        // mid-GOP-unsafe, so the plan degrades to one whole-file re-encode
        // segment — visible in the plan's shape, and never a silent
        // stream-copy that would leak or duplicate frames.
        let decisions = [decision("d1", 1.2, 1.8)];
        let plan = build_segment_plan(&decisions, 4.0, &[]);
        assert_eq!(modes(&plan), vec![("reencode", 0.0, 4.0)]);
        assert_eq!(plan[0].item_ids, vec!["d1".to_string()]);
    }

    #[test]
    fn plan_covers_the_whole_duration_with_one_reencode_segment() {
        let decisions = [decision("d1", 0.0, 4.0)];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![("reencode", 0.0, 4.0)],
            "a whole-file redaction is one re-encode segment — no concat"
        );
    }

    #[test]
    fn plan_spans_at_keyframe_times_stay_put() {
        // A span that already starts and ends on keyframes is unchanged by
        // alignment.
        let decisions = [decision("d1", 1.0, 2.0)];
        let keyframes = [0.0, 1.0, 2.0, 3.0];
        let plan = build_segment_plan(&decisions, 4.0, &keyframes);
        assert_eq!(
            modes(&plan),
            vec![
                ("stream_copy", 0.0, 1.0),
                ("reencode", 1.0, 2.0),
                ("stream_copy", 2.0, 4.0),
            ]
        );
    }

    #[test]
    fn graph_gates_each_effect_over_the_local_window_and_scaled_box() {
        let black = RedactionDecision {
            item_id: "d_black".to_string(),
            start_sec: 1.2,
            end_sec: 1.8,
            effect: RedactionEffect::Black,
            target_kind: RedactionTargetKind::Box,
            box_norm: Some([0.25, 0.25, 0.5, 0.5]),
            track_id: None,
            source_tag_refs: Vec::new(),
        };
        // 64x48 frame: the box scales to x=16 y=12 w=32 h=24.
        let (graph, applied) =
            build_filter_graph(&[&black], 1.0, 64, 48).expect("the black graph builds");
        assert!(
            graph.contains(
                "[0:v]drawbox=x=16:y=12:w=32:h=24:color=black@1:t=fill:\
                 enable='between(t,0.200,0.800)'[r0];[r0]null[vout]"
            ),
            "drawbox fills the scaled box over the local window: {graph}"
        );
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].pixel_x, 16);
        assert_eq!(applied[0].pixel_y, 12);
        assert_eq!(applied[0].pixel_w, 32);
        assert_eq!(applied[0].pixel_h, 24);
        assert!(!applied[0].delogo_edge_inset);

        let blur = RedactionDecision {
            effect: RedactionEffect::Blur,
            ..black.clone()
        };
        let (graph, _) = build_filter_graph(&[&blur], 1.0, 64, 48).expect("the blur graph builds");
        assert!(
            graph.contains(
                "split=2[b0][f0];[f0]crop=32:24:16:12,gblur=sigma=10[c0];\
                 [b0][c0]overlay=16:12:enable='between(t,0.200,0.800)'[r0]"
            ),
            "blur crops the scaled region, filters it, and overlays it back \
             gated by the window: {graph}"
        );

        let pixelate = RedactionDecision {
            effect: RedactionEffect::Pixelate,
            ..black.clone()
        };
        let (graph, _) =
            build_filter_graph(&[&pixelate], 1.0, 64, 48).expect("the pixelate graph builds");
        assert!(
            graph.contains("pixelize=w=4:h=3:mode=avg"),
            "pixelate blocks are one eighth of the smaller box side (min 2): \
             {graph}"
        );

        let delogo = RedactionDecision {
            effect: RedactionEffect::Delogo,
            ..black
        };
        let (graph, applied) =
            build_filter_graph(&[&delogo], 1.0, 64, 48).expect("the delogo graph builds");
        assert!(
            graph.contains("delogo=x=16:y=12:w=32:h=24:enable='between(t,0.200,0.800)'"),
            "delogo interpolates the scaled box over the local window: {graph}"
        );
        assert!(!applied[0].delogo_edge_inset);
    }

    #[test]
    fn graph_insets_delogo_boxes_touching_the_frame_edge() {
        let mut edge_decision = RedactionDecision {
            item_id: "d_edge".to_string(),
            start_sec: 1.2,
            end_sec: 1.8,
            effect: RedactionEffect::Delogo,
            target_kind: RedactionTargetKind::Box,
            box_norm: Some([0.0, 0.0, 0.5, 0.5]),
            track_id: None,
            source_tag_refs: Vec::new(),
        };
        let (graph, applied) =
            build_filter_graph(&[&edge_decision], 1.0, 64, 48).expect("edge delogo builds");
        assert!(
            graph.contains("delogo=x=1:y=1:w=31:h=23"),
            "the edge-touching box is inset by the interpolation band: {graph}"
        );
        assert!(applied[0].delogo_edge_inset);

        edge_decision.box_norm = Some([0.99, 0.0, 0.01, 0.01]);
        let error = build_filter_graph(&[&edge_decision], 1.0, 64, 48)
            .expect_err("a box too small for the inset rejects")
            .to_string();
        assert!(
            error.contains("too small"),
            "the too-small inset rejection names the condition: {error}"
        );
    }

    #[test]
    fn graph_refuses_non_box_targets() {
        let mut track_decision = RedactionDecision {
            item_id: "d_track".to_string(),
            start_sec: 1.2,
            end_sec: 1.8,
            effect: RedactionEffect::Blur,
            target_kind: RedactionTargetKind::MaskTrack,
            box_norm: None,
            track_id: Some("track_7".to_string()),
            source_tag_refs: Vec::new(),
        };
        let error = build_filter_graph(&[&track_decision], 1.0, 64, 48)
            .expect_err("a mask_track target refuses at graph build")
            .to_string();
        assert!(
            error.contains("mask_track"),
            "the refusal names the target kind: {error}"
        );

        track_decision.target_kind = RedactionTargetKind::Box;
        let error = build_filter_graph(&[&track_decision], 1.0, 64, 48)
            .expect_err("a box target without a box refuses at graph build")
            .to_string();
        assert!(
            error.contains("missing the normalized box"),
            "the refusal names the missing box: {error}"
        );
    }

    #[test]
    fn graph_chains_multiple_items_in_order() {
        let first = RedactionDecision {
            item_id: "d_first".to_string(),
            start_sec: 1.2,
            end_sec: 1.8,
            effect: RedactionEffect::Black,
            target_kind: RedactionTargetKind::Box,
            box_norm: Some([0.0, 0.0, 0.2, 0.2]),
            track_id: None,
            source_tag_refs: Vec::new(),
        };
        let second = RedactionDecision {
            item_id: "d_second".to_string(),
            start_sec: 1.4,
            end_sec: 1.9,
            effect: RedactionEffect::Pixelate,
            target_kind: RedactionTargetKind::Box,
            box_norm: Some([0.5, 0.5, 0.2, 0.2]),
            track_id: None,
            source_tag_refs: Vec::new(),
        };
        let (graph, applied) =
            build_filter_graph(&[&first, &second], 1.0, 64, 48).expect("the chain builds");
        assert!(
            graph.starts_with("[0:v]drawbox=") && graph.contains("[r0];[r0]"),
            "the second item chains on the first's output: {graph}"
        );
        assert!(graph.ends_with("[r1]null[vout]"));
        assert_eq!(
            applied
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["d_first", "d_second"]
        );
    }

    #[test]
    fn scale_box_clamps_rounding_inside_the_frame() {
        let rect = scale_box_to_pixels(&[0.5, 0.5, 0.5, 0.5], 64, 48);
        assert_eq!(
            rect,
            PixelRect {
                x: 32,
                y: 24,
                w: 32,
                h: 24
            }
        );
        let rect = scale_box_to_pixels(&[0.99, 0.99, 0.5, 0.5], 64, 48);
        assert_eq!(rect.x + rect.w, 64);
        assert_eq!(rect.y + rect.h, 48);
        let rect = scale_box_to_pixels(&[0.0, 0.0, 0.0001, 0.0001], 64, 48);
        assert_eq!((rect.w, rect.h), (1, 1), "a tiny box stays one pixel");
    }
}
