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
