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
        let fut: ScopedFuture<'_> =
            match request {
                rmcp::model::ClientRequest::CallToolRequest(call) => {
                    let provenance = request_meta_provenance(call.params.meta.as_ref());
                    Box::pin(CALL_PROVENANCE.scope(
                        provenance,
                        self.0.handle_request(
                            rmcp::model::ClientRequest::CallToolRequest(call),
                            context,
                        ),
                    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, RequestMetaObject};

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
