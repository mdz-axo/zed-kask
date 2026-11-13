//! Shared test support for the media crate's inline test modules.
//!
//! The `InferencePort` stub family — ten impls across three modules when
//! this landed (`tool_behavior_tests`, `tools::jobs::tests`,
//! `tools::workflows::tests`) — repeated the full `generate` signature
//! boilerplate per stub. The macro single-sources the signatures; each
//! stub supplies only the bodies that make it distinct.
//!
//! Parameter names are taken from the invocation — macro hygiene: a body
//! pasted from the call site can only reference bindings the call site
//! named, so a body that uses the prompt (or ignores it) passes the name
//! it wants to bind (the research server's batch-3 pattern).

use crate::transcript::TimedWord;
use crate::transcript_layers::{EdlLayer, LayerProvenance, TranscriptLayer};
use crate::transcript_select::{EdlEntry, EdlOp, WordRange};

/// Generate the `InferencePort` impl for a media test stub. `generate` is
/// the text surface — most media stubs never exercise it (one body:
/// panic, a canned error, or a canned result); `generate_vision` is the
/// vision surface; `media_generate` is the media-generation surface.
/// Every arm after `generate` is emitted only when supplied — otherwise
/// the stub keeps the trait default for that method.
macro_rules! media_inference_stub {
    (
        $name:ident,
        generate($gen_self:ident, $prompt:ident): $generate:block
        $(, generate_vision($gv_self:ident, $gv_prompt:ident, $gv_images:ident): $generate_vision:block)?
        $(, media_generate($media_self:ident, $op:ident, $media_params:ident): $media_generate:block)?
        $(,)?
    ) => {
        impl hkask_types::ports::InferencePort for $name {
            fn generate(
                &$gen_self,
                $prompt: &str,
                _: &hkask_types::template::LLMParameters,
                _: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                        > + Send
                        + '_,
                >,
            > $generate

            $(fn generate_vision(
                &$gv_self,
                $gv_prompt: &str,
                $gv_images: &[String],
                _: &hkask_types::template::LLMParameters,
                _: Option<&str>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                        > + Send
                        + '_,
                >,
            > $generate_vision)?

            $(fn media_generate<'a>(
                &'a $media_self,
                $op: &str,
                $media_params: &hkask_types::MediaGenerateParams,
            ) -> hkask_types::MediaFuture<'a> $media_generate)?
        }
    };
}

pub(crate) use media_inference_stub;

/// Decode JSON arguments into a handler's parameters exactly as the tool
/// boundary does, so a pin exercises what a caller can send: a misspelled
/// key, a number beyond the field's range, an absent optional.
pub fn wire<T: serde::de::DeserializeOwned>(
    arguments: serde_json::Value,
) -> Result<rmcp::handler::server::wrapper::Parameters<T>, serde_json::Error> {
    serde_json::from_value(arguments).map(rmcp::handler::server::wrapper::Parameters)
}

/// The live input schema of one registered tool, as `tools/list` serves it.
pub fn live_input_schema(tool: &str) -> serde_json::Value {
    let tools = crate::MediaServer::combined_router().list_all();
    let entry = tools
        .iter()
        .find(|entry| entry.name == tool)
        .unwrap_or_else(|| panic!("{tool} is not a registered tool"));
    serde_json::to_value(&entry.input_schema).expect("input schema serializes")
}

/// Write a real one-second recording with ffmpeg: a tone for `.wav`, a
/// 64x48 h264 video with no audio otherwise. Extra ffmpeg output options
/// go in `options`. The video and audio tools probe a source before they
/// touch it, so a test that hands them placeholder bytes is refused as
/// "not a media file" before the code under test runs.
pub async fn media_fixture(
    path: &std::path::Path,
    options: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let source: &[&str] = if path.extension().and_then(|ext| ext.to_str()) == Some("wav") {
        &["-f", "lavfi", "-i", "sine=frequency=440:duration=1"]
    } else {
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=64x48:rate=5",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ]
    };
    let output = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(source)
        .args(options)
        .arg(path)
        .output()
        .await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(())
}

/// The read-only organize request the gallery lifecycle tests share —
/// each test rebuilt the same closure (or inline block) to capture its
/// directory before the helper (2026-10-09 ratchet pass).
pub fn read_only_organize_request(
    path: &std::path::Path,
) -> rmcp::handler::server::wrapper::Parameters<crate::types::GalleryOrganizeRequest> {
    rmcp::handler::server::wrapper::Parameters(crate::types::GalleryOrganizeRequest {
        path: path.to_string_lossy().into_owned(),
        mode: "read-only".into(),
        recursive: true,
        auto_analyze: false,
    })
}

/// Words at 1-second boundaries: word i spans [i*1000, i*1000+500] ms.
/// Shared by the transcript layers, export and select test modules —
/// each rebuilt the same fixture (2026-10-12 ratchet pass).
pub fn timed_words(texts: &[&str]) -> Vec<TimedWord> {
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| TimedWord {
            word: text.to_string(),
            start_ms: index as u64 * 1000,
            end_ms: index as u64 * 1000 + 500,
            confidence: None,
        })
        .collect()
}

/// The overlapping-keeps EDL fixture the layers and store tests share:
/// keep [0,3] and keep [2,4] overlap at word 2. The layers test pins
/// `validate` rejecting it via the selection algebra; the store test
/// pins the same rejection at store time. Provenance is the caller's —
/// each module keeps its own local helper.
pub fn overlapping_keeps_edl(provenance: LayerProvenance) -> TranscriptLayer {
    TranscriptLayer::Edl(EdlLayer {
        provenance,
        ops: vec![
            EdlEntry {
                range: WordRange::new(0, 3),
                op: EdlOp::Keep,
            },
            EdlEntry {
                range: WordRange::new(2, 4),
                op: EdlOp::Keep,
            },
        ],
    })
}

/// A JPEG carrying one EXIF field: DateTimeOriginal, in the Exif sub-IFD
/// where cameras write it. Built by hand: the image crate writes no EXIF.
pub fn jpeg_with_capture_date(path: &std::path::Path, date: &str) {
    assert_eq!(date.len(), 19, "EXIF dates are YYYY:MM:DD hh:mm:ss");
    image::RgbImage::new(4, 4).save(path).expect("jpeg encodes");
    let jpeg = std::fs::read(path).expect("jpeg reads back");
    let mut tiff: Vec<u8> = Vec::new();
    tiff.extend_from_slice(b"II\x2a\x00\x08\x00\x00\x00"); // little-endian, IFD0 at 8
    tiff.extend_from_slice(&1u16.to_le_bytes()); // IFD0: one entry
    tiff.extend_from_slice(&0x8769u16.to_le_bytes()); // ExifOffset
    tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes()); // Exif IFD at 26
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    tiff.extend_from_slice(&1u16.to_le_bytes()); // Exif IFD: one entry
    tiff.extend_from_slice(&0x9003u16.to_le_bytes()); // DateTimeOriginal
    tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    tiff.extend_from_slice(&20u32.to_le_bytes());
    tiff.extend_from_slice(&44u32.to_le_bytes()); // value at 44
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(date.as_bytes());
    tiff.push(0);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[2..]);
    std::fs::write(path, out).expect("jpeg with EXIF written");
}
