use super::*;

pub(super) fn resource_status(
    label: &str,
    state: &ResourceState,
    has_last_good: bool,
    loading_detail: Option<&str>,
) -> Option<(SharedString, ui::Color)> {
    match state {
        ResourceState::Loading => Some((
            loading_detail
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if has_last_good {
                        format!("Refreshing {label} — showing last-good data…")
                    } else {
                        format!("Loading {label}…")
                    }
                })
                .into(),
            ui::Color::Muted,
        )),
        ResourceState::Failed(error) => Some((
            if has_last_good {
                format!("{label} degraded — showing last-good data: {error}")
            } else {
                format!("{label} failed: {error}")
            }
            .into(),
            ui::Color::Error,
        )),
        ResourceState::Idle | ResourceState::Ready => None,
    }
}

pub(super) fn parse_gallery_listing(
    payload: Option<Value>,
    requested_offset: usize,
) -> Result<GalleryListing, String> {
    let payload = payload
        .ok_or_else(|| "gallery_list_assets returned unparsable or failed output.".to_string())?;
    let object = payload
        .as_object()
        .ok_or_else(|| "gallery_list_assets returned a non-object payload.".to_string())?;
    let gallery_id = object
        .get("gallery_id")
        .and_then(Value::as_str)
        .filter(|gallery_id| !gallery_id.is_empty())
        .ok_or_else(|| "gallery_list_assets returned no gallery_id.".to_string())?
        .to_string();
    let total = object
        .get("total")
        .and_then(Value::as_u64)
        .ok_or_else(|| "gallery_list_assets returned no valid total.".to_string())?;
    let offset = object
        .get("offset")
        .and_then(Value::as_u64)
        .and_then(|offset| usize::try_from(offset).ok())
        .ok_or_else(|| "gallery_list_assets returned no valid offset.".to_string())?;
    if offset != requested_offset {
        return Err(format!(
            "gallery_list_assets returned offset {offset} for requested offset {requested_offset}."
        ));
    }
    let limit = object
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|limit| usize::try_from(limit).ok())
        .filter(|limit| *limit > 0)
        .ok_or_else(|| "gallery_list_assets returned no valid limit.".to_string())?;
    let assets = object
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "gallery_list_assets returned no assets array.".to_string())?;
    if assets.len() > limit {
        return Err("gallery_list_assets returned more rows than its limit.".into());
    }
    for (position, asset) in assets.iter().enumerate() {
        let record = asset
            .as_object()
            .ok_or_else(|| format!("gallery_list_assets row {position} is not an object."))?;
        let expected_index = offset
            .checked_add(position)
            .ok_or_else(|| "gallery_list_assets row index overflowed.".to_string())?;
        let index = record
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| format!("gallery_list_assets row {position} has no valid index."))?;
        if index != expected_index {
            return Err(format!(
                "gallery_list_assets row {position} has index {index}, expected {expected_index}."
            ));
        }
        for field in ["id", "path", "media_type"] {
            if record
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(format!(
                    "gallery_list_assets row {position} has no valid {field}."
                ));
            }
        }
    }
    Ok(GalleryListing {
        gallery_id,
        total,
        offset,
        limit,
        assets: assets.clone(),
    })
}

pub(super) fn parse_delete_acknowledgement(
    output: &str,
    expected_id: &str,
) -> Result<usize, String> {
    let payload = hkask_types::tool_response::parse_tool_response(output)
        .ok_or_else(|| "gallery_delete_image returned malformed data".to_string())?;
    if payload.get("deleted").and_then(Value::as_bool) != Some(true)
        || payload.get("image_id").and_then(Value::as_str) != Some(expected_id)
        || payload.get("file_deleted").and_then(Value::as_bool) != Some(false)
        || payload.get("gallery_changed").and_then(Value::as_bool) != Some(true)
    {
        return Err("gallery_delete_image returned an invalid acknowledgement".into());
    }
    payload
        .get("transcripts_detached")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| "gallery_delete_image returned no valid transcript count".to_string())
}

/// Strictly parse an `educt_list_transcripts` response into Detail rows.
/// Every summary must carry a non-empty `id`, a `words_count`, and a
/// `has_word_timings` flag, and the declared `count` must match the rows.
/// A malformed listing is an error, never an empty success — degradation
/// The exact `gallery_annotation_create` request.
pub(super) fn annotation_create_params(
    gallery_asset_id: &str,
    kind: &str,
    value: &str,
) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "kind": kind,
        "value": value,
    })
}

/// The exact `gallery_annotation_update` request — stable identity plus
/// the replacement value.
pub(super) fn annotation_update_params(
    gallery_asset_id: &str,
    annotation_id: &str,
    value: &str,
) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "annotation_id": annotation_id,
        "value": value,
    })
}

/// The exact `gallery_annotation_delete` request.
pub(super) fn annotation_delete_params(
    gallery_asset_id: &str,
    annotation_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "gallery_asset_id": gallery_asset_id,
        "annotation_id": annotation_id,
    })
}

/// Strict-parse `gallery_annotation_list` rows — a malformed row rejects
/// the whole listing, never degrades to an empty success (the house
/// pattern).
pub(super) fn parse_annotation_listing(payload: &Value) -> Result<Vec<PaneAnnotation>, String> {
    let annotations = payload
        .get("annotations")
        .and_then(Value::as_array)
        .ok_or_else(|| "gallery_annotation_list returned no annotations array.".to_string())?;
    let mut rows = Vec::with_capacity(annotations.len());
    for annotation in annotations {
        let object = annotation
            .as_object()
            .ok_or_else(|| "annotation row is not an object.".to_string())?;
        let annotation_id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "annotation row has no id.".to_string())?
            .to_string();
        let kind = object
            .get("tag_type")
            .and_then(Value::as_str)
            .filter(|kind| matches!(*kind, "tag" | "note"))
            .ok_or_else(|| "annotation row has no tag/note kind.".to_string())?
            .to_string();
        let value = object
            .get("value")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "annotation row has no value.".to_string())?
            .to_string();
        let created_at = object
            .get("created_at")
            .and_then(Value::as_str)
            .filter(|created| !created.is_empty())
            .ok_or_else(|| "annotation row has no created_at.".to_string())?
            .to_string();
        rows.push(PaneAnnotation {
            annotation_id,
            kind,
            value,
            created_at,
        });
    }
    Ok(rows)
}

/// surfaces, it does not masquerade as "no transcripts".
pub(super) fn parse_transcript_listing(
    payload: &Value,
) -> Result<Vec<TranscriptSummaryRow>, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "educt_list_transcripts returned no object data.".to_string())?;
    let transcripts = object
        .get("transcripts")
        .and_then(Value::as_array)
        .ok_or_else(|| "educt_list_transcripts returned no transcripts array.".to_string())?;
    let count = object
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| "educt_list_transcripts returned no count.".to_string())?;
    if count != transcripts.len() as u64 {
        return Err(format!(
            "educt_list_transcripts count {count} does not match {} rows.",
            transcripts.len()
        ));
    }
    transcripts
        .iter()
        .map(|row| {
            let object = row
                .as_object()
                .ok_or_else(|| "transcript summary is not an object.".to_string())?;
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "transcript summary has no id.".to_string())?;
            let media_path = object
                .get("media_path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| "transcript summary has no media_path.".to_string())?;
            let words_count = object
                .get("words_count")
                .and_then(Value::as_u64)
                .ok_or_else(|| "transcript summary has no words_count.".to_string())?;
            let has_word_timings = object
                .get("has_word_timings")
                .and_then(Value::as_bool)
                .ok_or_else(|| "transcript summary has no has_word_timings.".to_string())?;
            let duration_secs = object
                .get("audio_duration_secs")
                .and_then(Value::as_f64)
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .ok_or_else(|| "transcript summary has no valid audio_duration_secs.".to_string())?
                .round() as u64;
            let alignment = object
                .get("alignment")
                .and_then(Value::as_str)
                .filter(|alignment| {
                    matches!(
                        *alignment,
                        "original" | "aligned" | "re_anchored" | "unaligned" | "untimed"
                    )
                })
                .ok_or_else(|| {
                    "transcript summary has no valid alignment (original, aligned, re_anchored, unaligned, or untimed)."
                        .to_string()
                })?
                .to_string();
            let gallery_asset_id = object
                .get("gallery_asset_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
            Ok(TranscriptSummaryRow {
                id: id.to_string(),
                media_path: media_path.to_string(),
                words_count,
                duration_secs,
                has_word_timings,
                alignment,
                gallery_asset_id,
            })
        })
        .collect()
}

/// The recordings-browser / Detail row's capability note for a transcript's
/// alignment — None for the hand-annotatable states (the Reduct-normal
/// default: every row is an editable bundle unless flagged), a named gate
/// note for the read-only ones, so the entry surfaces say what a click
/// opens (the F10 alignment flags, 2026-10-27).
pub(super) fn recording_display_name(media_path: &str) -> String {
    let stem = std::path::Path::new(media_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let compact: String = stem.chars().filter(|character| *character != '-').collect();
    if matches!(compact.len(), 32 | 64)
        && compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return "Untitled recording".to_string();
    }
    let display = stem.replace(['-', '_'], " ");
    let display = display.trim();
    if display.is_empty() {
        "Untitled recording".to_string()
    } else {
        display.to_string()
    }
}

pub(super) fn format_recording_duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

pub(super) fn transcript_range_excerpt(
    state: &TranscriptPaneState,
    start_word: usize,
    end_word: usize,
) -> String {
    let Some(words) = state.words.as_deref() else {
        return "Selected text".to_string();
    };
    if start_word > end_word || end_word >= words.len() {
        return "Selected text".to_string();
    }
    let text = words[start_word..=end_word]
        .iter()
        .map(|word| word.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = text.chars();
    let excerpt: String = chars.by_ref().take(72).collect();
    if chars.next().is_some() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

pub(super) fn transcript_alignment_note(alignment: &str) -> Option<&'static str> {
    match alignment {
        "re_anchored" => Some("Estimated timings · review only"),
        "unaligned" => Some("Timed editing unavailable"),
        _ => None,
    }
}

/// The reel-provenance line for the inspector's Reel tab, from the detail payload's
/// `transcript_render` record: the source transcript this asset was rendered
/// from, with its creation time. Absent or null (most assets are not reels)
/// renders nothing; a malformed record renders nothing rather than breaking
/// the whole detail view.
pub(super) fn parse_reel_provenance(detail: &Value) -> Option<String> {
    let render = detail.get("transcript_render")?.as_object()?;
    let source = render.get("source_transcript_id")?.as_str()?;
    if source.is_empty() {
        return None;
    }
    let created = render
        .get("created_at")
        .and_then(Value::as_str)
        .unwrap_or("unknown time");
    Some(format!("{source} (created {created})"))
}

/// Deterministic label color (D8): the label's FNV-1a hash spreads hues —
/// no palette editor, no settings.
pub(super) fn highlight_label_color(label: &str) -> gpui::Hsla {
    let hash = label.bytes().fold(0x811c_9dc5_u32, |acc, byte| {
        (acc ^ byte as u32).wrapping_mul(0x0100_0193)
    });
    let hue = (hash % 360) as f32 / 360.0;
    gpui::hsla(hue, 0.45, 0.5, 0.35)
}

/// The word-click state machine's outcome (S4 part-2, S4-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WordClickOutcome {
    /// Selection mode is off — the click seeks.
    SeekOnly,
    /// The click set the pending selection's anchor.
    AnchorSet(usize),
    /// The click repeated the anchor — the pending selection cleared.
    AnchorCleared,
    /// The label editor is open — the click is inert (the editor is modal
    /// to its range).
    EditorOpen,
    /// The click completed a range — open the selection popover over
    /// [start, end] (the alignment gate runs before the popover opens; the
    /// popover's Highlight action opens the label editor).
    CompleteRange { start_word: usize, end_word: usize },
}

/// The press-and-release drag outcome (S2) — the one-gesture selection
/// path, active in both interaction modes. Mirrors
/// [`WordClickOutcome`]'s pure, pin-testable shape; the alignment gate
/// applies at the opening site (`word_drag_released`), not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WordDragOutcome {
    /// The label editor is open — the drag is inert (the editor is modal
    /// to its range).
    Inert,
    /// Press and release landed on the same word — the plain click path
    /// seeks (InteractiveText fires its click only for same-range
    /// down/up, so no selection is pending).
    Click,
    /// Press and release spanned words — open the selection popover over
    /// [start, end] (the alignment gate runs before the popover opens; the
    /// popover's Highlight action opens the label editor).
    CompleteRange { start_word: usize, end_word: usize },
}

/// The label editor's open target (S4-3): a completed two-click range
/// awaiting its label, a researcher highlight being renamed (the range and
/// note pass through `educt_annotation_update` unchanged — only the label
/// is replaced), a completed visual-transcript span awaiting its tag name
/// (S5 criterion 2 — dispatched through `layers_store`, kind
/// `researcher`, model `"user"`), or a completed selection awaiting its
/// correction text (the transcript-layer S5 — dispatched through
/// `educt_store_layer`, kind correction, then chained
/// `educt_realign_transcript`).
#[derive(Clone, Debug)]
pub(super) enum HighlightEditKind {
    Create {
        start_word: usize,
        end_word: usize,
    },
    Correction {
        start_word: usize,
        end_word: usize,
    },
    Rename {
        annotation_id: String,
        start_word: usize,
        end_word: usize,
        note: String,
    },
    EventTag {
        start_sec: f64,
        end_sec: f64,
    },
}

/// The open label editor (S4-3): its target and the typed text.
#[derive(Clone, Debug)]
pub(super) struct HighlightEdit {
    pub(super) kind: HighlightEditKind,
    pub(super) buffer: LabelBuffer,
}

/// S3: in-transcript search state — the typed query, the ordered match list
/// over working word indices (each match the inclusive word range it
/// covers), and the current match the Enter cycle sits on. The rescan is
/// debounced; Enter/Shift-Enter cycle and jump. Blank query → no matches →
/// no marks and no count.
#[derive(Clone, Debug, Default)]
pub(super) struct TranscriptSearch {
    pub(super) query: LabelBuffer,
    pub(super) matches: Vec<(usize, usize)>,
    pub(super) current: usize,
}

/// A single-line text buffer with a cursor — the model behind the panel's
/// inline inputs (the label editor and the transcript search). Char-indexed
/// so multi-byte text edits as characters, not bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LabelBuffer {
    pub(super) chars: Vec<char>,
    pub(super) cursor: usize,
}

impl LabelBuffer {
    pub(super) fn from_str(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        Self {
            cursor: chars.len(),
            chars,
        }
    }

    /// The typed text, untrimmed — the render reads it verbatim.
    pub(super) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The trimmed label to dispatch.
    pub(super) fn label(&self) -> String {
        self.text().trim().to_string()
    }

    /// The cursor's char index (the render splits the text there).
    pub(super) fn cursor(&self) -> usize {
        self.cursor.min(self.chars.len())
    }

    pub(super) fn insert(&mut self, ch: char) {
        self.cursor = self.cursor.min(self.chars.len());
        self.chars.insert(self.cursor, ch);
        self.cursor += 1;
    }

    pub(super) fn backspace(&mut self) {
        self.cursor = self.cursor.min(self.chars.len());
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub(super) fn delete_forward(&mut self) {
        self.cursor = self.cursor.min(self.chars.len());
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub(super) fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(super) fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    pub(super) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(super) fn move_end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// Insert pasted text, dropping control characters — the input is
    /// single-line (2026-10-26 review: labels could not be pasted at all).
    pub(super) fn insert_str(&mut self, text: &str) {
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            self.insert(ch);
        }
    }

    /// Apply an editing keystroke (backspace, delete, cursor moves, typed
    /// characters). Enter/Escape are the viewer's concern, not the
    /// buffer's. Returns whether the buffer consumed the key.
    pub(super) fn apply_keystroke(&mut self, keystroke: &Keystroke) -> bool {
        match keystroke.key.as_str() {
            "backspace" => self.backspace(),
            "delete" => self.delete_forward(),
            "left" => self.move_left(),
            "right" => self.move_right(),
            "home" => self.move_home(),
            "end" => self.move_end(),
            _ => match typed_char(keystroke) {
                Some(ch) => self.insert(ch),
                None => return false,
            },
        }
        true
    }
}

/// The character a keystroke types, if any — `key_char` when the platform
/// supplies it (shifted symbols, non-ASCII layouts), the plain key as the
/// fallback (shift uppercases ASCII). Modifier chords (ctrl/alt/cmd/fn)
/// are shortcuts, not typing.
pub(super) fn typed_char(keystroke: &Keystroke) -> Option<char> {
    let modifiers = &keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return None;
    }
    if let Some(key_char) = &keystroke.key_char {
        let mut chars = key_char.chars();
        let first = chars.next()?;
        return if chars.next().is_none() {
            Some(first)
        } else {
            None
        };
    }
    if keystroke.key == "space" {
        return Some(' ');
    }
    let mut chars = keystroke.key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(if modifiers.shift {
        first.to_ascii_uppercase()
    } else {
        first
    })
}

/// The completed selection's tray mode (D5 v2, the transcript-layer S5):
/// which action set a completed word range may offer on this view.
/// Original/aligned views are index-identical to the source, so every
/// tray action is available; a re_anchored view opens the CORRECTION
/// tray — the one action set whose dispatch translates the working
/// selection to source indices through the server (`educt_locate` basis
/// "source"), never a panel-side projection. The unselectable views keep
/// the visible refusal (the panel never re-implements the server's
/// projection — D5: index-identical-only actions stay unavailable on
/// corrected views rather than anchoring working indices to source-word
/// tools).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectionTrayMode {
    /// Highlight / Cut / Copy time / Suggest correction / Include in reel.
    Full,
    /// Suggest correction + Copy time — the translation-backed set.
    CorrectOnly,
}

pub(super) fn selection_tray_mode(alignment: &str) -> Result<SelectionTrayMode, &'static str> {
    match alignment {
        "original" | "aligned" => Ok(SelectionTrayMode::Full),
        "re_anchored" => Ok(SelectionTrayMode::CorrectOnly),
        "unaligned" => Err(
            "Direct selection needs a timing-aligned transcript. Re-transcribe or realign this correction first.",
        ),
        "untimed" => {
            Err("Direct selection needs word-level timings; this transcript is text only.")
        }
        _ => Err("Direct selection is unavailable for this transcript."),
    }
}

/// The authoring-path reason every researcher correction carries (S5) —
/// the correction edit's bookkeeping trail, naming where the layer came
/// from (the layer's own provenance names the authoring path: model
/// "user" + the researcher_correction template).
pub(super) const CORRECTION_REASON: &str =
    "researcher correction from the transcript workspace selection";

/// Convert a pane correction edit to the server's wire type — one home
/// for the field mapping, so the merge and the store request cannot
/// disagree.
pub(super) fn server_correction_edit(
    edit: &PaneCorrectionEdit,
) -> hkask_media::transcript_layers::CorrectionEdit {
    hkask_media::transcript_layers::CorrectionEdit {
        start_word: edit.start_word,
        end_word: edit.end_word,
        replacement: edit.replacement.clone(),
        reason: edit.reason.clone(),
    }
}

/// Convert the server's wire edit back to the pane type (the merge
/// output).
pub(super) fn pane_correction_edit(
    edit: &hkask_media::transcript_layers::CorrectionEdit,
) -> PaneCorrectionEdit {
    PaneCorrectionEdit {
        start_word: edit.start_word,
        end_word: edit.end_word,
        replacement: edit.replacement.clone(),
        reason: edit.reason.clone(),
    }
}

/// The correction store request (S5): the server's own layer types
/// serialize the layer — the schema cannot drift from the contract —
/// under the researcher authoring path. `created_at` is injected so the
/// fixture pins the wire shape byte-for-byte.
pub(super) fn correction_store_params(
    transcript_id: &str,
    edits: &[PaneCorrectionEdit],
    created_at: &str,
) -> Result<serde_json::Value, String> {
    let layer = hkask_media::transcript_layers::TranscriptLayer::Correction(
        hkask_media::transcript_layers::CorrectionLayer {
            provenance: hkask_media::transcript_layers::LayerProvenance {
                model: "user".to_string(),
                prompt_template: "researcher_correction".to_string(),
                created_at: created_at.to_string(),
            },
            edits: edits.iter().map(server_correction_edit).collect(),
        },
    );
    let layer_value = serde_json::to_value(&layer)
        .map_err(|error| format!("the correction layer failed to serialize: {error}"))?;
    Ok(serde_json::json!({
        "transcript_id": transcript_id,
        "layer": layer_value,
    }))
}

/// The realign request (S5): names the stored correction layer.
pub(super) fn correction_realign_params(transcript_id: &str, layer_id: &str) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "layer_id": layer_id,
    })
}

/// The locate request (S5): the selection's rendered quote under the
/// source basis — the server converts through the re-anchored index map.
pub(super) fn correction_locate_params(transcript_id: &str, quote: &str) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "text": quote,
        "basis": "source",
    })
}

/// Parse the locate response for the selection's SOURCE range: the
/// candidate whose WORKING range is exactly the selection — locate
/// returns every match with its working range alongside the converted
/// source range, so the translation is deterministic, never a guess.
pub(super) fn parse_correction_source_range(
    response: &str,
    start_word: usize,
    end_word: usize,
) -> Result<(usize, usize), String> {
    let value = hkask_types::tool_response::parse_tool_response(response)
        .ok_or_else(|| "educt_locate returned no parsable data.".to_string())?;
    let ranges = value
        .get("ranges")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "educt_locate returned no ranges array.".to_string())?;
    let candidate = ranges
        .iter()
        .find(|range| {
            range.get("working_start_word").and_then(Value::as_u64) == Some(start_word as u64)
                && range.get("working_end_word").and_then(Value::as_u64) == Some(end_word as u64)
        })
        .ok_or_else(|| {
            "The selection no longer matches the transcript — reload the workspace and reselect."
                .to_string()
        })?;
    let source_start = candidate
        .get("start_word")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "the located range has no source start_word.".to_string())?;
    let source_end = candidate
        .get("end_word")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "the located range has no source end_word.".to_string())?;
    Ok((source_start, source_end))
}

/// The create-confirm label (S4-3): the typed text, or the default label
/// when the editor is confirmed empty (the two-click + Enter fast path).
pub(super) fn resolve_create_label(typed: &str) -> String {
    let trimmed = typed.trim();
    if trimmed.is_empty() {
        "Highlight".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The exact `educt_annotation_create` request for a pane-created
/// highlight: the label editor's text and a provenance-honest note.
pub(super) fn highlight_create_params(
    transcript_id: &str,
    start_word: usize,
    end_word: usize,
    label: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": label,
        "note": "two-click selection from the transcript pane",
    })
}

/// The exact `educt_annotation_create` request for a pane strike (R2):
/// kind strike with blank label and note — the server enforces the
/// unannotated-cut invariant; the pane asks for it explicitly.
pub(super) fn strike_create_params(
    transcript_id: &str,
    start_word: usize,
    end_word: usize,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": "",
        "note": "",
        "kind": "strike",
    })
}

/// Compose the pane's text marks into the sorted, non-overlapping run list
/// gpui's `with_highlights` consumes. Its cursor runs forward only, so an
/// out-of-order or overlapping mark would style text past its range — a
/// struck word inside a highlight could lose its line-through, and the
/// follow-along mark inside any overlay could shift styling past its
/// word. Overlapping marks merge field-wise (the later mark's set fields
/// win), so a struck word inside a highlight keeps BOTH the background
/// and the line-through. The cost is bounded by the marked envelope, not
/// the block length.
pub(super) fn resolved_highlight_runs(
    marks: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if marks.is_empty() {
        return Vec::new();
    }
    let envelope_start = marks
        .iter()
        .map(|(range, _)| range.start)
        .min()
        .unwrap_or(0);
    let envelope_end = marks.iter().map(|(range, _)| range.end).max().unwrap_or(0);
    let mut cells: Vec<Option<HighlightStyle>> =
        vec![None; envelope_end.saturating_sub(envelope_start)];
    for (range, style) in marks {
        let start = range.start.max(envelope_start);
        let end = range.end.min(envelope_end);
        for cell in &mut cells[start - envelope_start..end - envelope_start] {
            *cell = Some(match cell.take() {
                None => style,
                Some(previous) => previous.highlight(style),
            });
        }
    }
    let mut runs = Vec::new();
    let mut open: Option<(usize, HighlightStyle)> = None;
    for (offset, cell) in cells.into_iter().enumerate() {
        let index = offset + envelope_start;
        match (open, cell) {
            (None, Some(style)) => open = Some((index, style)),
            (Some((start, style)), Some(next)) if style != next => {
                runs.push((start..index, style));
                open = Some((index, next));
            }
            (Some((start, style)), None) => {
                runs.push((start..index, style));
                open = None;
            }
            _ => {}
        }
    }
    if let Some((start, style)) = open {
        runs.push((start..envelope_end, style));
    }
    runs
}

/// The char range within `block` covering the word range
/// `start_word..=end_word` — None when the range misses the block or its
/// clamped local indices fall outside the word map. The block owns the
/// global→local mapping (D5); callers never re-implement it.
pub(super) fn block_word_char_range(
    block: &PaneBlock,
    start_word: usize,
    end_word: usize,
) -> Option<Range<usize>> {
    let block_first = block
        .word_starts
        .first()
        .map(|(index, _)| *index)
        .unwrap_or(0);
    let block_last = block
        .word_starts
        .last()
        .map(|(index, _)| *index)
        .unwrap_or(0);
    if start_word > block_last || end_word < block_first {
        return None;
    }
    let first = block
        .word_char_ranges
        .get(start_word.max(block_first) - block_first)?;
    let last = block
        .word_char_ranges
        .get(end_word.min(block_last) - block_first)?;
    Some(first.start..last.end)
}

/// The exact `educt_annotation_update` request for a pane rename (S4-3):
/// stable identity plus the replacement label over the unchanged range
/// and note.
pub(super) fn highlight_update_params(
    transcript_id: &str,
    annotation_id: &str,
    start_word: usize,
    end_word: usize,
    label: &str,
    note: &str,
) -> serde_json::Value {
    serde_json::json!({
        "transcript_id": transcript_id,
        "annotation_id": annotation_id,
        "start_word": start_word,
        "end_word": end_word,
        "label": label,
        "note": note,
    })
}

/// The pane's three reel compose modes (R3): assemble keeps the toggled-in
/// researcher highlights, reduce cuts the strikes out of the whole
/// transcript, and mixed keeps the toggled-in highlights minus the strikes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReelComposeMode {
    Assemble,
    Reduce,
    Mixed,
}

impl ReelComposeMode {
    /// The compose affordance's element-id slug.
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Assemble => "assemble",
            Self::Reduce => "reduce",
            Self::Mixed => "mixed",
        }
    }

    /// The compose affordance's label.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Assemble => "From included highlights",
            Self::Reduce => "From the recording with cuts applied",
            Self::Mixed => "From included highlights with cuts applied",
        }
    }

    /// The nothing-to-compose status for the mode.
    pub(super) fn empty_status(self) -> &'static str {
        match self {
            Self::Assemble => "No included highlights — include at least one highlight first.",
            Self::Reduce => "No cuts to apply — cut a selection first.",
            Self::Mixed => "Include highlights and cut selections before creating this reel.",
        }
    }

    /// The in-flight edit progress for the mode.
    pub(super) fn progress(self) -> &'static str {
        match self {
            Self::Assemble => "Creating reel from included highlights…",
            Self::Reduce => "Creating reel with cuts applied…",
            Self::Mixed => "Creating reel from included highlights with cuts applied…",
        }
    }
}

/// S6: the composed reel's ordered keep ranges — ONE home for the op walk
/// the compose request and the preview plan share. For each included clip in
/// composer order: the clip's source range minus its own in-reel strikes and
/// minus every word an earlier included clip already played (the selection
/// algebra admits each source span once — overlapping Keep ops are rejected
/// — so a later clip that overlaps an earlier one repeats only its unplayed
/// remainder). Both subtractions run through the server's published helpers
/// (union_ranges + subtract_cuts_from_range — the algebra's own difference
/// math, never a panel fork; the panel's former union_word_ranges mirror is
/// deleted).
pub(super) fn composed_keep_ranges(
    clips: &[ReelClip],
) -> Vec<hkask_media::transcript_select::WordRange> {
    use hkask_media::transcript_select::{WordRange, subtract_cuts_from_range, union_ranges};
    let mut played: Vec<WordRange> = Vec::new();
    let mut keeps: Vec<WordRange> = Vec::new();
    for clip in clips.iter().filter(|clip| clip.include) {
        let base = WordRange::new(clip.start_word, clip.end_word);
        let mut cuts: Vec<WordRange> = clip
            .strikes
            .iter()
            .map(|(start, end)| WordRange::new(*start, *end))
            .collect();
        cuts.extend(played.iter().copied());
        let pieces = subtract_cuts_from_range(base, &union_ranges(&cuts));
        played.extend(pieces.iter().copied());
        keeps.extend(pieces);
    }
    keeps
}

/// The pane's source bundle words as the server's timed-word wire type (S6)
/// — one home for the field mapping, so every word→time path (the preview
/// plan, the duration readout) maps identically to the render path.
pub(super) fn source_timed_words(
    state: &TranscriptPaneState,
) -> Vec<hkask_media::transcript::TimedWord> {
    state
        .source_words
        .iter()
        .map(|word| hkask_media::transcript::TimedWord {
            word: word.word.clone(),
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            confidence: None,
        })
        .collect()
}

/// The preview's span plan (S6): the composed clip list mapped to media
/// spans through the render path's own function over the stored SOURCE
/// bundle words (`edl_to_clip_plan` — the word→time mapping the render
/// uses), so preview and rendered reel agree on `re_anchored` views, where
/// the working view's timings are interpolated estimates over different
/// indices. Honest refusals, never silent: no included clips, or a source
/// with no word timings.
pub(super) fn reel_preview_plan(
    state: &TranscriptPaneState,
    clips: &[ReelClip],
) -> Result<Vec<(u64, u64)>, &'static str> {
    let keep_ranges = composed_keep_ranges(clips);
    if keep_ranges.is_empty() {
        return Err("Nothing to preview — include at least one clip in the reel first.");
    }
    let words = source_timed_words(state);
    if words.is_empty() {
        return Err("Reel preview needs the recording's source word timings.");
    }
    let edl = hkask_media::transcript_select::Edl {
        ops: keep_ranges
            .into_iter()
            .map(|range| hkask_media::transcript_select::EdlEntry {
                range,
                op: hkask_media::transcript_select::EdlOp::Keep,
            })
            .collect(),
    };
    hkask_media::transcript_select::edl_to_clip_plan(&words, &edl)
        .map_err(|_| "Reel preview needs the recording's source word timings.")
}

/// One clip's composed duration over the source bundle words (S6's duration
/// readout): the time-span total of the clip's strike-subtracted pieces —
/// what the reel will actually play for this clip. `None` when the source
/// carries no word timings (the readout degrades honestly).
pub(super) fn reel_clip_duration_ms(state: &TranscriptPaneState, clip: &ReelClip) -> Option<u64> {
    use hkask_media::transcript_select::{
        WordRange, subtract_cuts_from_range, union_ranges, word_range_to_time_range,
    };
    let words = source_timed_words(state);
    if words.is_empty() {
        return None;
    }
    let cuts: Vec<WordRange> = clip
        .strikes
        .iter()
        .map(|(start, end)| WordRange::new(*start, *end))
        .collect();
    let pieces = subtract_cuts_from_range(
        WordRange::new(clip.start_word, clip.end_word),
        &union_ranges(&cuts),
    );
    let mut total = 0;
    for piece in pieces {
        let (start_ms, end_ms) = word_range_to_time_range(&words, piece).ok()?;
        total += end_ms - start_ms;
    }
    Some(total)
}

/// The event-click state machine's outcome (S5, mirroring the word pane's
/// S4 part-2 model).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum EventClickOutcome {
    /// Selection mode is off — the click seeks.
    SeekOnly,
    /// The click set the pending span's anchor.
    AnchorSet(usize),
    /// The click repeated the anchor — the pending selection cleared.
    AnchorCleared,
    /// The label editor is open — the click is inert.
    EditorOpen,
    /// The click completed a span — [start_sec, end_sec] in source-PTS
    /// seconds, pending for both authoring flows.
    SpanSelected { start_sec: f64, end_sec: f64 },
    /// The two clicked events cover no time (or a stale anchor no longer
    /// resolves) — the pending selection clears with a named refusal.
    SpanRejected,
}

/// The redaction chain's success payload: the raw render response text
/// (for display-hint ingestion — the rendered asset lands as a playable
/// panel asset) plus its strict-parsed review/lineage summary.
pub(super) struct RedactionRenderOutcome {
    pub(super) render_text: String,
    pub(super) summary: RedactionRenderSummary,
}

/// The exact `layers_store` request for a select-to-authored researcher
/// tag (S5 criterion 2): kind `researcher`, provenance model `"user"` — a
/// human act, never machine-authored (design §3) — one item over the
/// selected span whose searchable text keys the label for `layers_search`.
pub(super) fn researcher_tag_params(
    media_path: &str,
    gallery_asset_id: Option<&str>,
    start_sec: f64,
    end_sec: f64,
    label: &str,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "media_path": media_path,
        "kind": "researcher",
        "label": label,
        "provenance": {
            "model": "user",
            "prompt_template": "media-panel:select-to-author",
        },
        "items": [{
            "start_sec": start_sec,
            "end_sec": end_sec,
            "searchable_text": format!("tag: {label}"),
            "payload": { "label": label },
        }],
    });
    if let Some(asset_id) = gallery_asset_id {
        params["gallery_asset_id"] = serde_json::json!(asset_id);
    }
    params
}

/// The exact `layers_store` request for an approved redaction decision
/// (S5 criterion 3, the S3 contract): kind `researcher`, model `"user"`,
/// one APPROVED item carrying the redaction vocabulary — op redact,
/// status approved, effect, the armed target's box, and the source-tag
/// lineage — over the selected span. Any unapproved item fails the whole
/// layer's render, so the panel authors approved decisions only.
pub(super) fn redaction_decision_params(
    media_path: &str,
    gallery_asset_id: Option<&str>,
    target: &RedactionTarget,
    span: (f64, f64),
    effect: PaneRedactionEffect,
) -> serde_json::Value {
    let [x, y, w, h] = target.box_norm;
    let mut params = serde_json::json!({
        "media_path": media_path,
        "kind": "researcher",
        "label": "redaction decisions",
        "provenance": {
            "model": "user",
            "prompt_template": "media-panel:select-to-author",
        },
        "items": [{
            "start_sec": span.0,
            "end_sec": span.1,
            "searchable_text": format!("redact: {} ({})", target.label, effect.as_str()),
            "payload": {
                "op": "redact",
                "status": "approved",
                "effect": effect.as_str(),
                "target": {
                    "kind": "box",
                    "box": { "x": x, "y": y, "w": w, "h": h },
                },
                "source_tag_refs": [target.source_ref],
            },
        }],
    });
    if let Some(asset_id) = gallery_asset_id {
        params["gallery_asset_id"] = serde_json::json!(asset_id);
    }
    params
}

/// Strict-parse a `layers_transcript` response (S1's reading surface) into
/// the event log the panel renders: status, the time-ordered events, the
/// total, and the kinds present. Contract fields reject the whole payload
/// (a malformed event log never degrades to a partial success); the
/// optional object box is parsed tolerantly because payloads are arbitrary
/// JSON by contract (design §3) — a malformed box renders its row without
/// the target affordance instead of rejecting the log.
pub(super) fn parse_visual_events(payload: &Value) -> Result<VisualEventsState, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "layers_transcript returned no object data.".to_string())?;
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "layers_transcript returned no status.".to_string())?;
    let note = object
        .get("note")
        .and_then(Value::as_str)
        .map(str::to_string);
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_transcript returned no events array.".to_string())?;
    match status {
        "no_layers" => {
            if !events.is_empty() {
                return Err(
                    "layers_transcript reported no_layers with nonempty events.".to_string()
                );
            }
            Ok(VisualEventsState {
                status: status.to_string(),
                events: Vec::new(),
                events_total: 0,
                kinds_present: Vec::new(),
                note,
            })
        }
        "completed" => {
            let mut rows = Vec::with_capacity(events.len());
            for event in events {
                let event = event
                    .as_object()
                    .ok_or_else(|| "visual-transcript event is not an object.".to_string())?;
                let start_sec = event
                    .get("start_sec")
                    .and_then(Value::as_f64)
                    .filter(|secs| secs.is_finite() && *secs >= 0.0)
                    .ok_or_else(|| {
                        "visual-transcript event has no nonnegative start_sec.".to_string()
                    })?;
                let end_sec =
                    match event.get("end_sec") {
                        None | Some(&Value::Null) => None,
                        Some(value) => {
                            let end = value.as_f64().filter(|secs| secs.is_finite()).ok_or_else(
                                || "visual-transcript event end_sec is not a number.".to_string(),
                            )?;
                            if end < start_sec {
                                return Err(
                                    "visual-transcript event end_sec precedes its start_sec."
                                        .to_string(),
                                );
                            }
                            Some(end)
                        }
                    };
                let kind = event
                    .get("kind")
                    .and_then(Value::as_str)
                    .filter(|kind| !kind.trim().is_empty())
                    .ok_or_else(|| "visual-transcript event has no kind.".to_string())?
                    .to_string();
                let layer_id = event
                    .get("layer_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "visual-transcript event has no layer_id.".to_string())?
                    .to_string();
                let item_id = event
                    .get("item_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "visual-transcript event has no item_id.".to_string())?
                    .to_string();
                let searchable_text = event
                    .get("searchable_text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| {
                        "visual-transcript event has blank searchable_text.".to_string()
                    })?
                    .to_string();
                let provenance = event
                    .get("provenance")
                    .and_then(Value::as_object)
                    .ok_or_else(|| "visual-transcript event has no provenance.".to_string())?;
                let model = provenance
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                    .ok_or_else(|| "visual-transcript event provenance has no model.".to_string())?
                    .to_string();
                // The S1 refinement's read-time corroboration: the sensory
                // badges of every overlapping event plus its own. The server
                // always emits `evidence`; a missing array is a contract
                // break, not a silent empty.
                let evidence = event
                    .get("evidence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "visual-transcript event has no evidence array.".to_string())?
                    .iter()
                    .map(|badge| {
                        badge
                            .as_str()
                            .filter(|badge| !badge.is_empty())
                            .map(str::to_string)
                            .ok_or_else(|| {
                                "visual-transcript event evidence entry is not a nonempty string."
                                    .to_string()
                            })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                // The optional object payload: a well-formed normalized box
                // arms the redaction target; anything else renders without
                // the affordance (payloads are arbitrary JSON by contract).
                let payload = event.get("payload");
                let (box_norm, object_label) = match payload {
                    None | Some(Value::Null) => (None, None),
                    Some(value) => {
                        let object_label = value
                            .get("label")
                            .and_then(Value::as_str)
                            .filter(|label| !label.trim().is_empty())
                            .map(str::to_string);
                        let r#box = value.get("box").and_then(Value::as_object);
                        let box_norm = r#box.and_then(|r#box| {
                            let mut values = [0.0; 4];
                            for (slot, key) in values.iter_mut().zip(["x", "y", "w", "h"]) {
                                *slot = r#box
                                    .get(key)
                                    .and_then(Value::as_f64)
                                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))?;
                            }
                            let [x, y, w, h] = values;
                            (w > 0.0 && h > 0.0 && x + w <= 1.0 && y + h <= 1.0).then_some(values)
                        });
                        (box_norm, object_label)
                    }
                };
                rows.push(PaneEventRow {
                    start_sec,
                    end_sec,
                    kind,
                    layer_id,
                    item_id,
                    searchable_text,
                    model,
                    box_norm,
                    object_label,
                    evidence,
                });
            }
            let events_total = object
                .get("events_total")
                .and_then(Value::as_u64)
                .ok_or_else(|| "layers_transcript returned no events_total.".to_string())?
                as usize;
            let kinds_present = object
                .get("kinds_present")
                .and_then(Value::as_array)
                .ok_or_else(|| "layers_transcript returned no kinds_present.".to_string())?
                .iter()
                .map(|kind| {
                    kind.as_str()
                        .filter(|kind| !kind.is_empty())
                        .map(str::to_string)
                        .ok_or_else(|| "kinds_present entry is not a kind string.".to_string())
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(VisualEventsState {
                status: status.to_string(),
                events: rows,
                events_total,
                kinds_present,
                note,
            })
        }
        other => Err(format!(
            "layers_transcript returned unknown status {other:?}."
        )),
    }
}

/// Strict-parse a `layers_render_redactions` response (S3's result JSON)
/// into the review/lineage summary the events tail renders: the render
/// plan's segments[] (mode + range + item count) and applied_items[] (what
/// was drawn where). A malformed result rejects visibly.
pub(super) fn parse_redaction_render(payload: &Value) -> Result<RedactionRenderSummary, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "layers_render_redactions returned no object data.".to_string())?;
    if object.get("status").and_then(Value::as_str) != Some("rendered") {
        return Err(format!(
            "layers_render_redactions returned status {:?}, expected \"rendered\".",
            object.get("status").and_then(Value::as_str)
        ));
    }
    let decision_layer_id = object
        .get("decision_layer_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "layers_render_redactions returned no decision_layer_id.".to_string())?
        .to_string();
    let decisions_rendered = object
        .get("decisions_rendered")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no decisions_rendered.".to_string())?
        as usize;
    let reencoded_segments = object
        .get("reencoded_segments")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no reencoded_segments.".to_string())?
        as usize;
    let stream_copied_segments = object
        .get("stream_copied_segments")
        .and_then(Value::as_u64)
        .ok_or_else(|| "layers_render_redactions returned no stream_copied_segments.".to_string())?
        as usize;
    let segments = object
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_render_redactions returned no segments.".to_string())?
        .iter()
        .map(|segment| {
            let mode = segment
                .get("mode")
                .and_then(Value::as_str)
                .filter(|mode| matches!(*mode, "stream_copy" | "reencode"))
                .ok_or_else(|| "render segment has an unknown mode.".to_string())?
                .to_string();
            let start_sec = segment
                .get("start_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite() && *secs >= 0.0)
                .ok_or_else(|| "render segment has no start_sec.".to_string())?;
            let end_sec = segment
                .get("end_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite() && *secs > start_sec)
                .ok_or_else(|| "render segment has no end beyond its start.".to_string())?;
            let items = segment
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .ok_or_else(|| "render segment has no items array.".to_string())?;
            Ok(PaneRenderSegment {
                mode,
                start_sec,
                end_sec,
                items,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let applied = object
        .get("applied_items")
        .and_then(Value::as_array)
        .ok_or_else(|| "layers_render_redactions returned no applied_items.".to_string())?
        .iter()
        .map(|item| {
            let item_id = item
                .get("item_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "applied item has no item_id.".to_string())?
                .to_string();
            let effect = item
                .get("effect")
                .and_then(Value::as_str)
                .filter(|effect| !effect.is_empty())
                .ok_or_else(|| "applied item has no effect.".to_string())?
                .to_string();
            let pixel = |key: &str| {
                item.get(key)
                    .and_then(Value::as_u64)
                    .filter(|v| *v <= u32::MAX as u64)
                    .map(|v| v as u32)
                    .ok_or_else(|| format!("applied item has no {key}."))
            };
            let local_start_sec = item
                .get("local_start_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite())
                .ok_or_else(|| "applied item has no local_start_sec.".to_string())?;
            let local_end_sec = item
                .get("local_end_sec")
                .and_then(Value::as_f64)
                .filter(|secs| secs.is_finite())
                .ok_or_else(|| "applied item has no local_end_sec.".to_string())?;
            Ok(PaneAppliedRedaction {
                item_id,
                effect,
                pixel_x: pixel("pixel_x")?,
                pixel_y: pixel("pixel_y")?,
                pixel_w: pixel("pixel_w")?,
                pixel_h: pixel("pixel_h")?,
                local_start_sec,
                local_end_sec,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RedactionRenderSummary {
        decision_layer_id,
        decisions_rendered,
        reencoded_segments,
        stream_copied_segments,
        segments,
        applied,
    })
}

/// The exact `educt_store_layer` request for a pane-composed reel (R3, S6
/// ordered form): the EDL built directly from the pane's ordered clip list
/// — Keep ops from the included clips in composer order, with in-reel
/// strikes baked into the keeps through the server's own difference math
/// (a composer EDL carries no Cut ops in the assemble form; a per-clip
/// strike as a Cut would strike EVERY clip overlapping the range), and Cut
/// ops from every whole-recording strike per the mode. Assemble keeps
/// only, reduce cuts only, mixed carries both; `None` when the mode has
/// nothing to compose. Provenance carries the pane_reel_compose template —
/// never researcher_annotation — so a composed EDL can never masquerade
/// as a researcher strike when the pane re-parses its own transcript.
pub(super) fn compose_reel_params(
    state: &TranscriptPaneState,
    clips: &[ReelClip],
    mode: ReelComposeMode,
) -> Option<serde_json::Value> {
    let keeps = match mode {
        ReelComposeMode::Reduce => Vec::new(),
        ReelComposeMode::Assemble | ReelComposeMode::Mixed => composed_keep_ranges(clips),
    };
    let cuts: Vec<(usize, usize)> = match mode {
        ReelComposeMode::Assemble => Vec::new(),
        ReelComposeMode::Reduce | ReelComposeMode::Mixed => state
            .strikes
            .iter()
            .map(|strike| (strike.start_word, strike.end_word))
            .collect(),
    };
    // Mixed is "keeps minus strikes": with nothing included there is
    // nothing to keep, so it refuses with its empty status rather than
    // silently composing the reduce reel (cuts-only ops read as the whole
    // transcript under the algebra's subtractive base).
    if mode == ReelComposeMode::Mixed && keeps.is_empty() {
        return None;
    }
    let mut ops = Vec::new();
    for range in keeps {
        ops.push(serde_json::json!({
            "op": "keep",
            "range": {"start_word": range.start_word, "end_word": range.end_word},
        }));
    }
    for (start_word, end_word) in cuts {
        ops.push(serde_json::json!({
            "op": "cut",
            "range": {"start_word": start_word, "end_word": end_word},
        }));
    }
    if ops.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "transcript_id": state.transcript_id,
        "layer": {
            "kind": "edl",
            "provenance": {
                "model": "user",
                "prompt_template": "pane_reel_compose",
                "created_at": hkask_types::time::now_rfc3339(),
            },
            "ops": ops,
        }
    }))
}

/// The working word containing the playback position — the last word whose
/// start_ms is at or before it (S3c's follow-along index math). `None`
/// before the first word or for an empty view.
pub(super) fn current_word_index(words: &[PaneWord], position_ms: u64) -> Option<usize> {
    words.iter().rposition(|word| word.start_ms <= position_ms)
}

/// The list item index of the block containing `word` — the same boundary
/// walk `build_pane_blocks` performs over the paragraph breaks, so the
/// follow-along can map its mark to a list item without rebuilding blocks
/// (S3d). Pinned against the builder by
/// `block_index_for_word_matches_block_boundaries`.
pub(super) fn block_index_for_word(state: &TranscriptPaneState, word: usize) -> Option<usize> {
    let words_len = state.words.as_ref()?.len();
    if word >= words_len {
        return None;
    }
    let breaks = state.paragraph_breaks.as_deref().unwrap_or(&[]);
    let mut start = 0usize;
    let mut block = 0usize;
    for &break_after in breaks {
        if break_after >= words_len || break_after < start {
            continue;
        }
        if word <= break_after {
            return Some(block);
        }
        start = break_after + 1;
        block += 1;
    }
    Some(block)
}

/// The ordered match list for the in-transcript search (S3): every
/// occurrence of the trimmed query over the working words joined by single
/// spaces (the same join the pane blocks render), mapped to the inclusive
/// word range each occurrence covers — so a phrase spanning words marks and
/// jumps as one range. The case rule matches the server's
/// `educt_search_transcripts` lexical mode (ASCII case-insensitive,
/// non-ASCII exact — `to_ascii_lowercase` preserves byte offsets, so the
/// lowered text's positions map back to the original) so the panel and the
/// server agree on what a match is. Occurrences never overlap (the scan
/// resumes past each hit); a blank query or wordless view matches nothing.
pub(super) fn transcript_search_matches(words: &[PaneWord], query: &str) -> Vec<(usize, usize)> {
    let query = query.trim();
    if query.is_empty() || words.is_empty() {
        return Vec::new();
    }
    let mut joined = String::new();
    let mut word_starts: Vec<usize> = Vec::with_capacity(words.len());
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            joined.push(' ');
        }
        word_starts.push(joined.len());
        joined.push_str(&word.word);
    }
    let haystack = joined.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(found) = haystack[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        // Byte offsets map back through the per-word starts: the last word
        // starting at or before the hit starts, and the last word starting
        // before the hit ends (the trimmed query never ends on a space).
        let start_word = word_starts.partition_point(|&offset| offset <= start) - 1;
        let end_word = word_starts.partition_point(|&offset| offset < end) - 1;
        matches.push((start_word, end_word));
        from = end;
    }
    matches
}

/// The search input's status label (S3): the current match's position over
/// the total, the honest zero-match state, or the word-timing degradation
/// note for text-only views — `None` while the query is blank (nothing to
/// report, no noise).
pub(super) fn transcript_search_status(
    search: &TranscriptSearch,
    has_words: bool,
) -> Option<String> {
    if search.query.text().trim().is_empty() {
        return None;
    }
    if !has_words {
        return Some("Search needs a word-timed view".into());
    }
    if search.matches.is_empty() {
        return Some("No matches".into());
    }
    Some(format!("{}/{}", search.current + 1, search.matches.len()))
}

/// Build the pane's paragraph blocks: paragraphs over the working words
/// (breaks honored; out-of-range breaks skipped), each labeled with the
/// speaker covering its first word. Text-only views (no words) yield a
/// single unlabeled block of the working text. The char ranges and word
/// starts are the click-to-seek and follow-along mapping tables.
pub(super) fn build_pane_blocks(state: &TranscriptPaneState) -> Vec<PaneBlock> {
    let Some(words) = state.words.as_deref() else {
        return vec![PaneBlock {
            speaker: None,
            text: state.working_text.clone(),
            word_char_ranges: Vec::new(),
            word_starts: Vec::new(),
        }];
    };
    let breaks = state.paragraph_breaks.as_deref().unwrap_or(&[]);
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut start = 0usize;
    for &break_after in breaks {
        if break_after >= words.len() || break_after < start {
            continue;
        }
        ranges.push(start..break_after + 1);
        start = break_after + 1;
    }
    if start < words.len() {
        ranges.push(start..words.len());
    }
    if ranges.is_empty() {
        ranges.push(0..words.len());
    }
    ranges
        .into_iter()
        .map(|range| {
            let block_start = range.start;
            let mut text = String::new();
            let mut cursor = 0usize;
            let mut word_char_ranges = Vec::with_capacity(range.len());
            let mut word_starts = Vec::with_capacity(range.len());
            for global_index in range {
                let word = &words[global_index];
                if !text.is_empty() {
                    text.push(' ');
                    cursor += 1;
                }
                let char_count = word.word.chars().count();
                word_char_ranges.push(cursor..cursor + char_count);
                word_starts.push((global_index, word.start_ms));
                text.push_str(&word.word);
                cursor += char_count;
            }
            PaneBlock {
                speaker: speaker_at_word(state.speaker_spans.as_deref(), block_start),
                text,
                word_char_ranges,
                word_starts,
            }
        })
        .collect()
}

/// The media block body for a transcript's media path — the same
/// display-hint shape the viz-core registry keys shared widgets by, so
/// the transcript workspace's player is the SAME instance the asset
/// workspace's player and inline renders use (D1: one player per media
/// identity). Audio containers take the audio player; everything else is
/// a video source.
pub(super) fn transcript_media_body(media_path: &str) -> String {
    let extension = media_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = if matches!(
        extension.as_str(),
        "mp3" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "flac" | "wma"
    ) {
        "audio"
    } else {
        "video"
    };
    serde_json::json!({ "kind": kind, "src": media_path }).to_string()
}

/// The speaker covering a working word index, if a speaker layer exists.
pub(super) fn speaker_at_word(
    spans: Option<&[PaneSpeakerSpan]>,
    word_index: usize,
) -> Option<String> {
    spans?
        .iter()
        .find(|span| span.start_word <= word_index && word_index <= span.end_word)
        .map(|span| span.speaker.clone())
}

/// The newest layer of `kind` from a get_transcript layer list, as its raw
/// layer object — deterministic (created_at, then id), mirroring the
/// server's own select_layer_of_kind tie-break.
pub(super) fn latest_layer_payload<'a>(layers: &'a [Value], kind: &str) -> Option<&'a Value> {
    layers
        .iter()
        .filter_map(|record| {
            let layer = record.get("layer")?;
            if layer.get("kind").and_then(Value::as_str) == Some(kind) {
                let created_at = record.get("created_at").and_then(Value::as_str)?;
                let record_id = record.get("id").and_then(Value::as_str)?;
                Some((created_at, record_id, layer))
            } else {
                None
            }
        })
        .max_by(|left, right| left.0.cmp(right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, layer)| layer)
}

/// Strictly parse an `educt_get_transcript` payload into the workspace
/// state. Consumes only what the pane renders — summary id/media path, the
/// working view (text + words + alignment), the latest paragraph layer's
/// breaks, the latest speaker layer's spans, and the layer rows for the
/// strip — and rejects a malformed payload outright: a half view is worse
/// than a visible error.
pub(super) fn parse_transcript_payload(payload: &Value) -> Result<TranscriptPaneState, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "educt_get_transcript returned no object data.".to_string())?;
    let summary = object
        .get("summary")
        .and_then(Value::as_object)
        .ok_or_else(|| "educt_get_transcript returned no summary object.".to_string())?;
    let transcript_id = summary
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "transcript summary has no id.".to_string())?
        .to_string();
    let media_path = summary
        .get("media_path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "transcript summary has no media_path.".to_string())?
        .to_string();
    // The recording this transcript is stored against, when it has one
    // (R3): the workspace keys its media to the RECORDING — the same
    // asset the asset workspace's player renders — and falls to the
    // transcript's stored media_path only when the recording left the
    // gallery index.
    let gallery_asset_id = summary
        .get("gallery_asset_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let working = object
        .get("working_transcript")
        .and_then(Value::as_object)
        .ok_or_else(|| "educt_get_transcript returned no working_transcript.".to_string())?;
    let working_text = working
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "working transcript has no text.".to_string())?
        .to_string();
    let alignment = working
        .get("alignment")
        .and_then(Value::as_str)
        .filter(|alignment| {
            matches!(
                *alignment,
                "original" | "aligned" | "re_anchored" | "unaligned" | "untimed"
            )
        })
        .ok_or_else(|| "working transcript has no valid alignment.".to_string())?
        .to_string();
    let alignment_note = working
        .get("alignment_error")
        .and_then(Value::as_str)
        .map(str::to_string);
    let words = if matches!(alignment.as_str(), "original" | "aligned" | "re_anchored") {
        let words = working
            .get("words")
            .and_then(Value::as_array)
            .filter(|words| !words.is_empty())
            .ok_or_else(|| {
                format!("working transcript alignment {alignment} requires timed words.")
            })?;
        Some(
            words
                .iter()
                .map(|word| {
                    let object = word
                        .as_object()
                        .ok_or_else(|| "working word is not an object.".to_string())?;
                    Ok(PaneWord {
                        word: object
                            .get("word")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "working word has no text.".to_string())?
                            .to_string(),
                        start_ms: object
                            .get("start_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "working word has no start_ms.".to_string())?,
                        end_ms: object
                            .get("end_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "working word has no end_ms.".to_string())?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    } else {
        None
    };
    let layers_array = object
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(|| "educt_get_transcript returned no layers array.".to_string())?;
    let mut layers = Vec::with_capacity(layers_array.len());
    let mut highlights = Vec::new();
    let mut strikes = Vec::new();
    for layer in layers_array {
        let object = layer
            .as_object()
            .ok_or_else(|| "layer record is not an object.".to_string())?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "layer record has no id.".to_string())?
            .to_string();
        let layer_object = object
            .get("layer")
            .and_then(Value::as_object)
            .ok_or_else(|| "layer record has no layer object.".to_string())?;
        let kind = layer_object
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty())
            .ok_or_else(|| "layer record has no kind.".to_string())?
            .to_string();
        if kind == "highlight" {
            // The pane's researcher affordances mirror the server's
            // mutate_user_highlight guard exactly: model "user" AND the
            // researcher_annotation template. Other user-authored highlight
            // layers — the legacy workspace_reel_gather compose copies — are pane
            // bookkeeping, never annotations: their entries are excluded
            // (2026-10-26 review: they re-entered the list as researcher
            // rows whose rename/delete the server always refuses, and every
            // compose added another copy).
            let provenance = layer_object.get("provenance");
            let researcher_supplied = provenance
                .and_then(|provenance| provenance.get("model"))
                .and_then(Value::as_str)
                == Some("user")
                && provenance
                    .and_then(|provenance| provenance.get("prompt_template"))
                    .and_then(Value::as_str)
                    == Some("researcher_annotation");
            let is_gather_layer = provenance
                .and_then(|provenance| provenance.get("prompt_template"))
                .and_then(Value::as_str)
                == Some("workspace_reel_gather");
            if is_gather_layer {
                // Gathered copies never render — the originals are already
                // in the list.
                let created_at = object
                    .get("created_at")
                    .and_then(Value::as_str)
                    .filter(|created| !created.is_empty())
                    .ok_or_else(|| "layer record has no created_at.".to_string())?
                    .to_string();
                layers.push(PaneLayerRow {
                    id,
                    kind,
                    created_at,
                });
                continue;
            }
            let entries = layer_object
                .get("highlights")
                .and_then(Value::as_array)
                .ok_or_else(|| "highlight layer has no highlights array.".to_string())?;
            for entry in entries {
                let entry = entry
                    .as_object()
                    .ok_or_else(|| "highlight entry is not an object.".to_string())?;
                let start_word = entry
                    .get("start_word")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| "highlight entry has no start_word.".to_string())?;
                let end_word = entry
                    .get("end_word")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| "highlight entry has no end_word.".to_string())?;
                let label = entry
                    .get("label")
                    .and_then(Value::as_str)
                    .filter(|label| !label.is_empty())
                    .ok_or_else(|| "highlight entry has no label.".to_string())?
                    .to_string();
                let note = entry
                    .get("note")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "highlight entry has no note.".to_string())?
                    .to_string();
                highlights.push(PaneHighlight {
                    start_word,
                    end_word,
                    label,
                    note,
                    annotation_id: id.clone(),
                    researcher_supplied,
                });
            }
        } else if kind == "edl" {
            // Strikes (R2): researcher-authored single-Cut EDLs. The pane
            // mirrors the server's strike identity exactly (model "user"
            // AND the researcher_annotation template AND exactly one Cut
            // op), so multi-op compose EDLs and model EDLs stay in the
            // Layers strip but never render as strikes.
            let provenance = layer_object.get("provenance");
            let researcher_supplied = provenance
                .and_then(|provenance| provenance.get("model"))
                .and_then(Value::as_str)
                == Some("user")
                && provenance
                    .and_then(|provenance| provenance.get("prompt_template"))
                    .and_then(Value::as_str)
                    == Some("researcher_annotation");
            if researcher_supplied {
                let ops = layer_object
                    .get("ops")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "edl layer has no ops array.".to_string())?;
                if ops.len() == 1 {
                    let entry = ops
                        .first()
                        .and_then(Value::as_object)
                        .ok_or_else(|| "edl op is not an object.".to_string())?;
                    let op = entry
                        .get("op")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "edl op has no op.".to_string())?;
                    if op == "cut" {
                        let range = entry
                            .get("range")
                            .and_then(Value::as_object)
                            .ok_or_else(|| "edl cut op has no range.".to_string())?;
                        let start_word = range
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "edl cut range has no start_word.".to_string())?;
                        let end_word = range
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "edl cut range has no end_word.".to_string())?;
                        strikes.push(PaneStrike {
                            start_word,
                            end_word,
                            annotation_id: id.clone(),
                        });
                    }
                }
            }
        }
        let created_at = object
            .get("created_at")
            .and_then(Value::as_str)
            .filter(|created| !created.is_empty())
            .ok_or_else(|| "layer record has no created_at.".to_string())?
            .to_string();
        layers.push(PaneLayerRow {
            id,
            kind,
            created_at,
        });
    }
    let paragraph_breaks = latest_layer_payload(layers_array, "paragraph")
        .map(|layer| {
            layer
                .get("breaks_after")
                .and_then(Value::as_array)
                .ok_or_else(|| "paragraph layer has no breaks_after.".to_string())?
                .iter()
                .map(|break_after| {
                    break_after
                        .as_u64()
                        .and_then(|index| usize::try_from(index).ok())
                        .ok_or_else(|| "paragraph break is not a word index.".to_string())
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    let speaker_spans = latest_layer_payload(layers_array, "speaker")
        .map(|layer| {
            let spans = layer
                .get("spans")
                .and_then(Value::as_array)
                .ok_or_else(|| "speaker layer has no spans.".to_string())?;
            spans
                .iter()
                .map(|span| {
                    let object = span
                        .as_object()
                        .ok_or_else(|| "speaker span is not an object.".to_string())?;
                    Ok(PaneSpeakerSpan {
                        start_word: object
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "speaker span has no start_word.".to_string())?,
                        end_word: object
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "speaker span has no end_word.".to_string())?,
                        speaker: object
                            .get("speaker")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "speaker span has no speaker.".to_string())?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    // Review flags (task 1a): optional and tolerant — the running editor
    // may predate the server field, so its absence parses as None; when
    // present, the view parses strictly.
    let review = object.get("review").map(parse_pane_review).transpose()?;
    // The live correction layer's edits (S5): the working view names the
    // layer by correction_layer_id; an absent id parses as no prior
    // edits; a named layer missing from the records rejects the payload
    // rather than silently dropping prior corrections (the silent-revert
    // guard).
    let correction_layer_id = working
        .get("correction_layer_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let correction_edits = match &correction_layer_id {
        None => Vec::new(),
        Some(live_id) => {
            let record = layers_array
                .iter()
                .find(|layer| layer.get("id").and_then(Value::as_str) == Some(live_id.as_str()))
                .ok_or_else(|| {
                    format!(
                        "the working view names correction layer {live_id}, but the \
                         layers list has no such record."
                    )
                })?;
            let layer_object = record
                .get("layer")
                .and_then(Value::as_object)
                .ok_or_else(|| "the correction layer record has no layer object.".to_string())?;
            if layer_object.get("kind").and_then(Value::as_str) != Some("correction") {
                return Err(format!("layer {live_id} is not a correction layer."));
            }
            let edits = layer_object
                .get("edits")
                .and_then(Value::as_array)
                .ok_or_else(|| "the correction layer has no edits array.".to_string())?;
            edits
                .iter()
                .map(|edit| {
                    let object = edit
                        .as_object()
                        .ok_or_else(|| "correction edit is not an object.".to_string())?;
                    Ok(PaneCorrectionEdit {
                        start_word: object
                            .get("start_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "correction edit has no start_word.".to_string())?,
                        end_word: object
                            .get("end_word")
                            .and_then(Value::as_u64)
                            .and_then(|index| usize::try_from(index).ok())
                            .ok_or_else(|| "correction edit has no end_word.".to_string())?,
                        replacement: object
                            .get("replacement")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "correction edit has no replacement.".to_string())?
                            .to_string(),
                        reason: object
                            .get("reason")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "correction edit has no reason.".to_string())?
                            .to_string(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        }
    };
    // The immutable source bundle's word timings (S6): the render path's
    // own word→time input (`educt_get_transcript` carries the stored bundle
    // under "transcript"). Absent when the stored bundle has no word
    // timings (a text-only source) — the preview refuses honestly then.
    let source_words = object
        .get("transcript")
        .and_then(Value::as_object)
        .and_then(|bundle| bundle.get("words"))
        .and_then(Value::as_array)
        .map(|words| {
            words
                .iter()
                .map(|word| {
                    let entry = word
                        .as_object()
                        .ok_or_else(|| "source bundle word is not an object.".to_string())?;
                    Ok(PaneWord {
                        word: entry
                            .get("word")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "source bundle word has no text.".to_string())?
                            .to_string(),
                        start_ms: entry
                            .get("start_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "source bundle word has no start_ms.".to_string())?,
                        end_ms: entry
                            .get("end_ms")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| "source bundle word has no end_ms.".to_string())?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(TranscriptPaneState {
        transcript_id,
        media_path,
        gallery_asset_id,
        alignment,
        alignment_note,
        words,
        working_text,
        paragraph_breaks,
        speaker_spans,
        review,
        layers,
        highlights,
        strikes,
        correction_edits,
        source_words,
    })
}

/// Strict-parse the derived review view (task 1a). Only invoked when the
/// payload carries a "review" object; a malformed one rejects the whole
/// payload — never a half view.
pub(super) fn parse_pane_review(payload: &Value) -> Result<PaneReview, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "review is not an object.".to_string())?;
    let threshold = object
        .get("threshold")
        .and_then(Value::as_f64)
        .ok_or_else(|| "review has no threshold.".to_string())?;
    let blocks_array = object
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| "review has no blocks array.".to_string())?;
    let mut blocks = Vec::with_capacity(blocks_array.len());
    for block in blocks_array {
        let object = block
            .as_object()
            .ok_or_else(|| "review block is not an object.".to_string())?;
        let start_word = object
            .get("start_word")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| "review block has no start_word.".to_string())?;
        let end_word = object
            .get("end_word")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or_else(|| "review block has no end_word.".to_string())?;
        let min_confidence = match object.get("min_confidence") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_f64()
                    .ok_or_else(|| "review block min_confidence is not a number.".to_string())?,
            ),
        };
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| "review block has no status.".to_string())?;
        let status = PaneReviewStatus::parse(status)?;
        blocks.push(PaneReviewFlag {
            start_word,
            end_word,
            min_confidence,
            status,
        });
    }
    let note = object
        .get("note")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(PaneReview {
        threshold,
        blocks,
        note,
    })
}

pub(super) fn parse_job_listing(output: &str) -> Result<JobListing, String> {
    parse_job_list_response(output).map_err(|error| format!("job_list failed: {error}"))
}

pub(super) fn short_job_id(id: &str) -> SharedString {
    id.chars().take(8).collect::<String>().into()
}

pub(super) fn job_is_nonterminal(job: &JobRecord) -> bool {
    NONTERMINAL_JOB_STATUSES.contains(&job.status.as_str())
}

/// The ambient verb for a live research operation's badge — short enough
/// for a row chip, never error-styled.
pub(super) fn job_op_verb(op: &str) -> &'static str {
    match op {
        "transcribe" => "Transcribing…",
        "audio_trim" => "Trimming…",
        "audio_concat" | "video_concat" => "Joining…",
        "video_clip" => "Clipping…",
        _ => "Working…",
    }
}

/// Order the live set newest-first and drop duplicate ids — a job can
/// appear in both the page and a probe snapshot within one tick. Stable
/// sort: equal `created_at` keeps insertion order (page before probes).
pub(super) fn dedupe_jobs_newest_first(jobs: &mut Vec<JobRecord>) {
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut seen = HashSet::new();
    jobs.retain(|job| seen.insert(job.id.clone()));
}

/// Render a scalar JSON value as display text (None for objects/arrays).
pub(super) fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parse a fenced ```media display hint into a `MediaAsset` — the block body
/// (JSON between the fences) plus the kind/src fields for labels.
pub(super) fn asset_from_hint(hint: &str, tool: &SharedString) -> Option<MediaAsset> {
    let trimmed = hint.trim();
    let after_fence = trimmed.strip_prefix("```media")?;
    let body = after_fence.trim().strip_suffix("```")?.trim().to_string();
    let payload = hkask_media_widget::MediaBlockBody::parse(&body).ok()?;
    payload.to_media_ref().ok()?;
    let kind = payload.kind;
    let src = payload.src;
    let gallery_asset_id = payload.gallery_asset_id;
    Some(MediaAsset {
        body,
        src,
        kind,
        tool: tool.clone(),
        gallery_index: None,
        gallery_asset_id,
    })
}

/// Reconcile the asset list against one `gallery_list_assets` page.
/// Records merge by src: a conversation-surfaced asset for the same file
/// gains its gallery index and stable id; new gallery assets append. Then
/// removals: the page vouches for global indexes `[offset, offset + len)`
/// (the server's `index` is `offset + i` — the `image_index` other gallery
/// tools accept), so an indexed asset inside that window whose src is
/// absent from the page was deleted from the gallery — drop it. When the
/// payload's `total` is known, an indexed asset at or beyond `total` is
/// past the end of the active listing — the terminal page shrank and the
/// tail is exhausted — drop it too (an empty last page removes nothing on
/// its own, but `total` bounds it). Unindexed (conversation-surfaced)
/// assets and assets whose index falls outside the page are untouched:
/// the page says nothing about them.
pub(super) fn merge_gallery_records(
    assets: &mut Vec<MediaAsset>,
    records: &[Value],
    offset: usize,
    total: Option<u64>,
) {
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
        let gallery_asset_id = record
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string);
        if let Some(existing) = assets.iter_mut().find(|existing| existing.src == src) {
            existing.gallery_index = gallery_index;
            if let Some(asset_id) = gallery_asset_id.as_deref()
                && let Ok(mut body) = serde_json::from_str::<Value>(&existing.body)
                && let Some(object) = body.as_object_mut()
            {
                object.insert(
                    "gallery_asset_id".to_string(),
                    Value::String(asset_id.to_string()),
                );
                existing.body = body.to_string();
            }
            existing.gallery_asset_id = gallery_asset_id;
        } else {
            let mut body = serde_json::json!({"kind": kind, "src": src});
            if let Some(asset_id) = gallery_asset_id.as_deref() {
                body["gallery_asset_id"] = Value::String(asset_id.to_string());
            }
            assets.push(MediaAsset {
                body: body.to_string(),
                src: src.to_string(),
                kind,
                tool: "gallery".into(),
                gallery_index,
                gallery_asset_id,
            });
        }
    }
    let window = offset..offset + records.len();
    let active_bound = total.map(|total| total as usize);
    assets.retain(|asset| match asset.gallery_index {
        Some(index) => {
            let in_window = window.contains(&index);
            let beyond_total = active_bound.is_some_and(|bound| index >= bound);
            !(in_window || beyond_total) || listed_srcs.contains(asset.src.as_str())
        }
        None => true,
    });
}

/// Read the producer-authored gallery mutation fact from a completed tool
/// result. Tool names are routing labels, not evidence that state changed.
pub(super) fn tool_result_changed_gallery(output: &Value) -> bool {
    let payload = match output {
        Value::String(text) => hkask_types::tool_response::parse_tool_response(text),
        value => {
            let payload = hkask_types::tool_response::unwrap_tool_envelope(value.clone());
            if hkask_types::tool_response::parse_tool_error_value(&payload).is_some() {
                None
            } else {
                Some(payload)
            }
        }
    };
    payload
        .as_ref()
        .and_then(|payload| payload.get("gallery_changed"))
        .and_then(Value::as_bool)
        == Some(true)
}
