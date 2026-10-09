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

/// Generate the `InferencePort` impl for a media test stub. `generate` is
/// the text surface — most media stubs never exercise it (one body:
/// panic, a canned error, or a canned result); `list_models` and
/// `generate_vision` are the vision surfaces (the gallery analysis path
/// resolves a vision model through `list_models` and analyzes through
/// `generate_vision`); `media_generate` is the media-generation surface.
/// Every arm after `generate` is emitted only when supplied — otherwise
/// the stub keeps the trait default for that method.
macro_rules! media_inference_stub {
    (
        $name:ident,
        generate($gen_self:ident, $prompt:ident): $generate:block
        $(, list_models($lm_self:ident): $list_models:block)?
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

            $(fn list_models(
                &$lm_self,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                Vec<hkask_types::ports::ModelEntry>,
                                hkask_types::InferenceError,
                            >,
                        > + Send
                        + '_,
                >,
            > $list_models)?

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
