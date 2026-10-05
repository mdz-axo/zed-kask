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
/// panic, a canned error, or a canned result); `media_generate` is the
/// surface under test and is emitted only when supplied — otherwise the
/// stub keeps the trait default.
macro_rules! media_inference_stub {
    (
        $name:ident,
        generate($gen_self:ident, $prompt:ident): $generate:block
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

            $(fn media_generate<'a>(
                &'a $media_self,
                $op: &str,
                $media_params: &hkask_types::MediaGenerateParams,
            ) -> hkask_types::MediaFuture<'a> $media_generate)?
        }
    };
}

pub(crate) use media_inference_stub;
