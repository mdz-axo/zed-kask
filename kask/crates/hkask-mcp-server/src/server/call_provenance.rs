//! Per-call inference provenance — the M5 read side (FR-4, falsifiers
//! (b)/(c) of the private-inference requirements D-2): the parent
//! rides the calling thread's D100 mark on the MCP call envelope's
//! `_meta` (`inference_provenance`), and the dispatch scopes it so
//! every handler can read the context it serves — parent-set, never
//! caller-claimed (the tool arguments expose no provenance field; P4).
//!
//! Fail-closed for recall, in the request direction: an ABSENT or
//! unparsable request mark reads PUBLIC — a caller that claims no mark
//! gets no private rows (the strictest read). This is the deliberate
//! mirror of the STORED-mark parser's fail-closed direction
//! (`hkask-types` visibility.rs: a present-but-unknown STORED mark
//! reads private and is excluded from public recall): both directions
//! deny access — they fail closed against different corruption classes
//! (a corrupted stored row must not leak; an unverifiable request
//! claim must not unlock).

use hkask_types::InferenceProvenance;

// The wire key's one home moved to `hkask_types::META_INFERENCE_PROVENANCE`
// when the parent write side (hkask-mcp) landed — both ends of the `_meta`
// channel now share the same constant instead of carrying two spellings
// that could drift apart with no compile-time witness. The re-export keeps
// this module's public path stable (the read-side home stays here).
pub use hkask_types::META_INFERENCE_PROVENANCE;

tokio::task_local! {
    static CALL_PROVENANCE: InferenceProvenance;
}

/// The current tool call's provenance — the fail-closed PUBLIC default
/// outside a dispatch scope (no boundary, a direct test call, or an
/// absent mark): recall filters read this and exclude private rows in
/// public contexts.
pub fn current_call_provenance() -> InferenceProvenance {
    CALL_PROVENANCE
        .try_with(|provenance| *provenance)
        .unwrap_or_default()
}

/// Scope a future under an explicit call provenance — the test seam for
/// the private-context pins (the production scope is set only by the
/// dispatch decorator, from the parent's `_meta`; never by tool code).
pub async fn with_call_provenance<F: Future>(provenance: InferenceProvenance, fut: F) -> F::Output {
    CALL_PROVENANCE.scope(provenance, fut).await
}

/// Parse the parent's request mark: ONLY the exact closed-set private
/// string claims private provenance — absent, null, non-string, or
/// unknown values all read PUBLIC (fail-closed for recall). The closed
/// set itself lives on `InferenceProvenance` (visibility.rs — the
/// stored-mark parser); this is the request-claim parser, not a second
/// parser of stored semantics.
pub fn request_meta_provenance(
    meta: Option<&rmcp::model::RequestMetaObject>,
) -> InferenceProvenance {
    let Some(meta) = meta else {
        return InferenceProvenance::PublicProvider;
    };
    match meta.0.0.get(META_INFERENCE_PROVENANCE) {
        Some(serde_json::Value::String(value))
            if value == InferenceProvenance::PrivateInference.as_str() =>
        {
            InferenceProvenance::PrivateInference
        }
        _ => InferenceProvenance::PublicProvider,
    }
}

/// A `Service<RoleServer>` decorator that scopes the per-call
/// provenance from the tools/call request's `_meta` around the inner
/// service's handling — every kask MCP server gets the M5 read side by
/// construction (wired once in `run_stdio_server`; zero per-server
/// edits). Every method delegates; only the tools/call arm scopes.
pub struct ProvenanceScopedService<S>(pub S);

impl<S> rmcp::Service<rmcp::RoleServer> for ProvenanceScopedService<S>
where
    S: rmcp::Service<rmcp::RoleServer>,
{
    fn handle_request(
        &self,
        request: <rmcp::RoleServer as rmcp::service::ServiceRole>::PeerReq,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<
        Output = Result<<rmcp::RoleServer as rmcp::service::ServiceRole>::Resp, rmcp::ErrorData>,
    > + rmcp::service::MaybeSendFuture
    + '_ {
        // Boxed: the scope wrapper and the direct delegate are different
        // future types — the RPIT needs one.
        type ScopedFuture<'a> = std::pin::Pin<
            Box<
                dyn Future<
                        Output = Result<
                            <rmcp::RoleServer as rmcp::service::ServiceRole>::Resp,
                            rmcp::ErrorData,
                        >,
                    > + Send
                    + 'a,
            >,
        >;
        let fut: ScopedFuture<'_> = match request {
            rmcp::model::ClientRequest::CallToolRequest(call) => {
                let provenance = request_meta_provenance(call.params.meta.as_ref());
                // CU-27: the response passes through kind_rmcp_decode_rejection
                // so rmcp's pre-handler argument-decode rejection carries
                // structuredContent {error, kind: invalid_argument} on the
                // wire — the same classification the editor's ledger reads
                // from the text (error_kind_from_display), made visible to
                // every structuredContent.kind consumer.
                Box::pin(async move {
                    CALL_PROVENANCE
                        .scope(
                            provenance,
                            self.0.handle_request(
                                rmcp::model::ClientRequest::CallToolRequest(call),
                                context,
                            ),
                        )
                        .await
                        .map(kind_rmcp_decode_rejection)
                })
            }
            other => Box::pin(self.0.handle_request(other, context)),
        };
        fut
    }

    fn handle_notification(
        &self,
        notification: <rmcp::RoleServer as rmcp::service::ServiceRole>::PeerNot,
        context: rmcp::service::NotificationContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<(), rmcp::ErrorData>> + rmcp::service::MaybeSendFuture + '_
    {
        self.0.handle_notification(notification, context)
    }

    fn get_info(&self) -> <rmcp::RoleServer as rmcp::service::ServiceRole>::Info {
        self.0.get_info()
    }

    fn supported_protocol_versions(
        &self,
    ) -> std::borrow::Cow<'static, [rmcp::model::ProtocolVersion]> {
        self.0.supported_protocol_versions()
    }
}

/// rmcp's macro-layer argument-decode rejection, produced before a kask
/// handler runs: a request whose arguments fail to deserialize into the
/// tool's parameter type (wrong type, unknown field, missing required
/// field) reaches the caller as `isError: true` with a text message and NO
/// structuredContent — no error kind on the wire (mcp-tool-review CU-27,
/// 2026-11-16; the same platform defect measured on nostr-relay as NR-07,
/// experimentation as EX-17 item 6, research as S-11).
///
/// The prefix is a boundary contract pinned to rmcp's generated text
/// ("failed to deserialize parameters:" — the const in rmcp's router,
/// `handler/server/router/tool.rs`), the same contract
/// `hkask_types::tool_response::error_kind_from_display` classifies by on
/// the editor side. The decorator below makes the server wire carry what
/// the editor already reads: the rejection gains
/// `structuredContent = {"error", "kind": "invalid_argument"}`, so a
/// consumer reading structuredContent.kind sees the kind instead of
/// string-matching the message.
pub const RMCP_ARGUMENT_DESERIALIZATION_PREFIX: &str = "failed to deserialize parameters:";

/// Attach the typed kind to a decode-time rejection. A `CallToolResult`
/// that is an error, carries no structuredContent yet, and whose first
/// text content starts with rmcp's decode-rejection prefix is given
/// `{"error": <text>, "kind": "invalid_argument"}`. Everything else —
/// handler refusals (which already carry `{error, kind}`), successes, and
/// non-decode text errors — passes through unchanged.
pub fn kind_rmcp_decode_rejection(result: rmcp::model::ServerResult) -> rmcp::model::ServerResult {
    let rmcp::model::ServerResult::CallToolResult(mut result) = result else {
        return result;
    };
    if result.structured_content.is_some() || result.is_error != Some(true) {
        return rmcp::model::ServerResult::CallToolResult(result);
    }
    let text = result
        .content
        .iter()
        .find_map(|block| match block {
            rmcp::model::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .unwrap_or("");
    if !text.starts_with(RMCP_ARGUMENT_DESERIALIZATION_PREFIX) {
        return rmcp::model::ServerResult::CallToolResult(result);
    }
    result.structured_content = Some(serde_json::json!({
        "error": text,
        "kind": "invalid_argument",
    }));
    rmcp::model::ServerResult::CallToolResult(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, RequestMetaObject};

    /// CU-27: the wire kind the consumer reads — a decode-time rejection
    /// carries structuredContent.kind = invalid_argument naming the field
    /// in the error text; the text content is unchanged.
    #[test]
    fn decode_rejection_gains_the_invalid_argument_kind() {
        let mut result = CallToolResult::default();
        result.is_error = Some(true);
        result.content = vec![ContentBlock::text(
            "failed to deserialize parameters: missing field `entity`",
        )];
        let result = rmcp::model::ServerResult::CallToolResult(result);
        let rmcp::model::ServerResult::CallToolResult(kinded) = kind_rmcp_decode_rejection(result)
        else {
            panic!("the result stays a CallToolResult");
        };
        let structured = kinded.structured_content.expect("the kind rides the wire");
        assert_eq!(structured["kind"], "invalid_argument");
        assert!(
            structured["error"]
                .as_str()
                .is_some_and(|text| text.contains("missing field `entity`")),
            "the error text names the field: {structured}"
        );
    }

    /// A handler refusal already carries {error, kind} — untouched (never
    /// overwritten, never duplicated).
    #[test]
    fn a_typed_handler_refusal_passes_through_unchanged() {
        let mut result = CallToolResult::default();
        result.is_error = Some(true);
        result.content = vec![ContentBlock::text("max_age_days must be positive")];
        result.structured_content = Some(serde_json::json!({
            "error": "max_age_days must be positive",
            "kind": "invalid_argument",
        }));
        let result = rmcp::model::ServerResult::CallToolResult(result);
        let rmcp::model::ServerResult::CallToolResult(passed) = kind_rmcp_decode_rejection(result)
        else {
            panic!("the result stays a CallToolResult");
        };
        assert_eq!(
            passed.structured_content,
            Some(serde_json::json!({
                "error": "max_age_days must be positive",
                "kind": "invalid_argument",
            }))
        );
    }

    /// A success, an unkinded non-decode error, and an is_error decode
    /// rejection whose text lacks the prefix all pass through untouched.
    #[test]
    fn non_decode_shapes_pass_through_unchanged() {
        let mut success = CallToolResult::default();
        success.is_error = None;
        success.content = vec![ContentBlock::text("ok")];
        let success = rmcp::model::ServerResult::CallToolResult(success);
        let rmcp::model::ServerResult::CallToolResult(passed) = kind_rmcp_decode_rejection(success)
        else {
            panic!("the result stays a CallToolResult");
        };
        assert!(passed.structured_content.is_none());

        let mut plain_error = CallToolResult::default();
        plain_error.is_error = Some(true);
        plain_error.content = vec![ContentBlock::text("storage is down")];
        let plain_error = rmcp::model::ServerResult::CallToolResult(plain_error);
        let rmcp::model::ServerResult::CallToolResult(passed) =
            kind_rmcp_decode_rejection(plain_error)
        else {
            panic!("the result stays a CallToolResult");
        };
        assert!(passed.structured_content.is_none());
    }

    fn meta(json: serde_json::Value) -> Option<RequestMetaObject> {
        let map = match json {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        Some(RequestMetaObject(rmcp::model::MetaObject(map)))
    }

    #[test]
    fn absent_or_unknown_request_marks_read_public() {
        // Fail-closed for recall: nothing but the exact private string
        // claims private provenance.
        assert_eq!(
            request_meta_provenance(None),
            InferenceProvenance::PublicProvider
        );
        for value in [
            serde_json::json!({}),
            serde_json::json!({"inference_provenance": serde_json::Value::Null}),
            serde_json::json!({"inference_provenance": "public_provider"}),
            serde_json::json!({"inference_provenance": "garbage"}),
            serde_json::json!({"inference_provenance": 42}),
        ] {
            let meta = meta(value);
            assert_eq!(
                request_meta_provenance(meta.as_ref()),
                InferenceProvenance::PublicProvider,
                "only the exact private string claims private"
            );
        }
    }

    #[test]
    fn the_exact_private_string_claims_private() {
        let meta = meta(serde_json::json!({"inference_provenance": "private_inference"}));
        assert_eq!(
            request_meta_provenance(meta.as_ref()),
            InferenceProvenance::PrivateInference
        );
    }

    #[tokio::test]
    async fn the_scope_reaches_handlers_and_absence_reads_public() {
        // Outside any scope: the fail-closed public default.
        assert_eq!(
            current_call_provenance(),
            InferenceProvenance::PublicProvider
        );
        // Inside a private scope: the handler sees private.
        CALL_PROVENANCE
            .scope(InferenceProvenance::PrivateInference, async {
                assert_eq!(
                    current_call_provenance(),
                    InferenceProvenance::PrivateInference
                );
            })
            .await;
        // A nested public scope (the dispatch arm for an absent mark):
        // the handler sees public.
        CALL_PROVENANCE
            .scope(InferenceProvenance::PublicProvider, async {
                assert_eq!(
                    current_call_provenance(),
                    InferenceProvenance::PublicProvider
                );
            })
            .await;
    }

    #[test]
    fn the_call_params_shape_parses_from_wire_json() {
        // The exact wire shape the parent writes: params.meta._meta —
        // the params struct round-trips the meta through serde, so the
        // parse works against the deserialized request, not a hand-built
        // object.
        let params: CallToolRequestParams = serde_json::from_value(serde_json::json!({
            "name": "curator_memory_recall",
            "arguments": {"entity": "x"},
            "_meta": {"inference_provenance": "private_inference"}
        }))
        .expect("the wire params parse");
        assert_eq!(
            request_meta_provenance(params.meta.as_ref()),
            InferenceProvenance::PrivateInference
        );
    }
}
