#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Server — MCP server utilities and startup verification.
//!
//! Provides the lightweight layer that all hKask MCP servers depend on:
//! - Server scaffolding (McpToolError, ServerContext, CredentialRequirement, run_stdio_server)
//! - URL validation, identifier validation, HTTP helpers
//! - Macros for tool-surface pins, ontology annotation, and server construction

pub(crate) mod security;
pub mod server;

// NOTE: The canonical MCP server registry lives in
// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS` (id + binary + description).
// Do NOT re-introduce a parallel list here — it drifts (the previous
// `BUILTIN_SERVERS` used id `"kanban"` while the canonical list uses
// `"kata-kanban"`, and the two contradicted each other silently).

pub use server::{
    CredentialRequirement, McpError, ServerContext, ToolContext, execute_tool, parse_env_warn,
    resolve_credential, resolve_db_passphrase, run_stdio_server, validate_identifier,
    validate_path, validate_tool_url_permissive, validate_tool_url_with_dns,
};
pub use server::{
    MAX_READ_BYTES, contain_for_read, contain_for_write, ensure_existing_dir, ensure_existing_file,
    map_database_error, map_infra_error, map_io_error, map_join_error, map_memory_store_error,
    read_capped, serpapi_get_json, url_encode_query_value, write_contained,
};

/// Run an MCP server with stdio transport.
///
/// This is the canonical entry point for all hKask MCP servers.
/// Each server's `main.rs` should call this directly.
#[must_use = "result must be used"]
pub async fn run_server<S, F>(
    name: &str,
    version: &str,
    factory: F,
    credentials: Vec<CredentialRequirement>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer>,
    S: rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    run_stdio_server(name, version, factory, credentials).await
}

/// Pin a server's registered tool surface with one macro call: the exact
/// tool count AND the build.rs-generated `TOOL_NAMES` set against the live
/// router. The count test catches additions/removals; the name test catches
/// same-count renames (which the count alone cannot) — together they make
/// silent registration drift fail a test instead of degrading to
/// tool-not-found at dispatch.
///
/// The invoking test module must bring `TOOL_NAMES` into scope: either a
/// crate-root `include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"))`
/// re-exported through the module's `use super::*`, or a module-local
/// include directly inside the test module. Unqualified `TOOL_NAMES` below
/// resolves at the call site either way.
///
/// `$test_name` keeps each server's historical count-test name (docs cite
/// them); the name test is fixed-shape everywhere.
///
/// # Example
/// ```ignore
/// #[cfg(test)]
/// mod tool_surface_tests {
///     use super::*;
///     include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
///     hkask_mcp_server::tool_surface_pin!(
///         TrainingServer::combined_router(),
///         "combined_router",
///         9,
///         tool_surface_is_exactly_9_registered_tools,
///     );
/// }
/// ```
#[macro_export]
macro_rules! tool_surface_pin {
    ($router:expr, $router_name:literal, $expected:literal, $test_name:ident $(,)?) => {
        #[test]
        fn $test_name() {
            let n = $router.list_all().len();
            assert_eq!(
                n, $expected,
                "{} registered tool surface changed; got {}",
                $router_name, n
            );
        }

        #[test]
        fn tool_names_match_live_router() {
            let mut live: Vec<String> = $router
                .list_all()
                .iter()
                .map(|tool| tool.name.to_string())
                .collect();
            live.sort();
            let mut generated: Vec<&str> = TOOL_NAMES.to_vec();
            generated.sort();
            assert_eq!(
                generated,
                live.iter().map(String::as_str).collect::<Vec<_>>(),
                concat!(
                    "TOOL_NAMES (build.rs-generated) must match the live ",
                    $router_name,
                    " surface"
                )
            );
        }
    };
}

/// Define the shared dual-axis annotate machinery from each server's
/// mapping table — the P5.4 scaffold every `ontology.rs` repeats
/// (the `tool_surface_pin!` precedent).
///
/// Nine servers carry the same invariant code: the `tool_to_dual_axis`
/// combinator, the `annotate` stamp, and the both-directions totality
/// test. Repeating it per server formed ~19 duplication clusters
/// (measured by the ratchet after the A-3 rollout) — this macro collapses
/// them to one definition. Each server keeps only what is genuinely
/// its own: the two mapping tables (`tool_to_pko`, `tool_to_dc`) and
/// the tool-name predicate for the reverse scan.
///
/// Emits, at the invocation site (the server's `ontology` module):
/// - `tool_to_dual_axis` — the both-axes combinator (a caller never
///   gets one axis without the other);
/// - `annotate` — the production stamp wired through `mcp_server!`'s
///   annotate override;
/// - a `dual_axis_covers_all_registered_tools` test asserting BOTH
///   directions: every registered tool has a mapping, and every
///   mapped name (found by `$predicate` over this file's quoted
///   strings) is a registered tool — the phantom-name catch
///   (live 2026-10-18: a typo'd phantom tool name passed coverage);
/// - an `unknown_tool_has_no_axes` test (no default concept for a
///   non-registered name).
///
/// The invoking module must bring `TOOL_NAMES` into scope (a
/// module-local `include!` of the generated list, or a re-export via
/// `use super::*`).
///
/// `$predicate` is a **function path** taking a `&str` (a quoted
/// chunk from this file) and returning `bool` — e.g. a closure-like
/// helper or a plain `fn(&str) -> bool` defined in the invoking
/// module. A path, never an expression — the same macro-hygiene rule
/// as `mcp_server!`'s annotate override.
///
/// `$pko_ty` and `$dc_ty` are the concept types (`PkoConcept`,
/// `DcConcept` — both `&'static str` aliases from
/// `hkask-bridge-ontology`, in scope in the invoking module). Passed
/// as paths so this crate stays dependency-free of the vocabulary
/// crate — the servers import it, the macro only relays their types.
///
/// # Example
/// ```ignore
/// // src/ontology.rs — the server's own mapping tables, then:
/// fn is_tool_name(s: &str) -> bool {
///     (s.starts_with("corpus_") || s.starts_with("media_"))
///         && s.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
/// }
/// hkask_mcp_server::dual_axis_annotate!(
///     tool_to_pko, tool_to_dc, PkoConcept, DcConcept, is_tool_name,
/// );
/// ```
#[macro_export]
macro_rules! dual_axis_annotate {
    (
        $pko:path,
        $dc:path,
        $pko_ty:ty,
        $dc_ty:ty,
        $prefixes:expr,
        $extra_rejects:expr $(,)?
    ) => {
        /// Both axes for a registered tool, or `None` when the name is
        /// not one.
        ///
        /// P5.4's dual-axis core: a caller never gets one axis without
        /// the other, so a partial mapping cannot silently satisfy the
        /// contract.
        pub(crate) fn tool_to_dual_axis(tool_name: &str) -> Option<($pko_ty, $dc_ty)> {
            match ($pko(tool_name), $dc(tool_name)) {
                (Some(process), Some(state)) => Some((process, state)),
                _ => None,
            }
        }

        /// Stamp the dual-axis identity onto a tool response (P5.4).
        ///
        /// The production consumer of the mapping: wired as the central
        /// `ToolContext::annotate_response` hook via the `mcp_server!`
        /// macro's annotate override — one point stamps every tool
        /// response at the `execute_tool` boundary. The annotation is
        /// additive — the existing body is untouched.
        ///
        /// A name with no mapping returns the body unchanged rather
        /// than inventing an axis: the totality test is what guarantees
        /// the registered surface is total, and a silent default would
        /// defeat it.
        pub(crate) fn annotate(tool_name: &str, mut body: serde_json::Value) -> serde_json::Value {
            let Some((process, state)) = tool_to_dual_axis(tool_name) else {
                return body;
            };
            if let Some(object) = body.as_object_mut() {
                object.insert(
                    "ontology".to_string(),
                    serde_json::json!({
                        "pko_procedure": process,
                        "dc_type": state,
                    }),
                );
            }
            body
        }

        /// Is this quoted string a possible tool-name literal in this
        /// file? The reverse-scan predicate, generated here so the
        /// prefix-fragment rule is defined once: a name is strictly
        /// longer than its prefix (the predicate's own prefix literals
        /// are fragments, excluded), and the remainder is a valid tool
        /// name tail. `$extra_rejects` names any non-tool literals in
        /// this file the prefix set would otherwise catch (e.g. a JSON
        /// fixture field); pass `&[]` when there are none.
        #[cfg(test)]
        fn is_tool_name(s: &str) -> bool {
            const PREFIXES: &[&str] = $prefixes;
            const EXTRA_REJECTS: &[&str] = $extra_rejects;
            if EXTRA_REJECTS.contains(&s) {
                return false;
            }
            for p in PREFIXES {
                if let Some(rest) = s.strip_prefix(p) {
                    return !rest.is_empty()
                        && rest
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit());
                }
            }
            false
        }

        #[cfg(test)]
        mod dual_axis_tests {
            use super::*;

            // The generated canonical tool-name list — included from
            // OUT_DIR, so the totality test derives its expectations from
            // the live tool surface (build.rs scan), never a hardcoded
            // list. The generator emits exactly one const (`TOOL_NAMES`),
            // read in full by the tests below.
            include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

            /// P5.4: every registered tool must carry a process AND a
            /// state identity — a tool added without an explicit arm
            /// fails here rather than silently losing its axes.
            #[test]
            fn dual_axis_covers_all_registered_tools() {
                for name in TOOL_NAMES {
                    let mapped = tool_to_dual_axis(name);
                    assert!(
                        mapped.is_some(),
                        "no dual-axis mapping for registered tool '{name}'; add an \
                         explicit arm to tool_to_pko and tool_to_dc in src/ontology.rs"
                    );
                }
                // The reverse direction: every name the mapping claims
                // must be a registered tool. Coverage alone let a
                // phantom name sit in a mapping undetected
                // (live 2026-10-18: a typo'd tool name,
                // matched no call site and still passed coverage).
                let ontology_src = std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("src/ontology.rs"),
                )
                .expect("src/ontology.rs must be readable");
                let mut mapped: Vec<&str> = Vec::new();
                // The stamp's own JSON keys are not tool names —
                // excluded by name, not by pattern.
                const STAMP_KEYS: [&str; 3] = ["ontology", "pko_procedure", "dc_type"];
                for chunk in ontology_src.split('"').skip(1).step_by(2) {
                    if !chunk.is_empty()
                        && is_tool_name(chunk)
                        && !STAMP_KEYS.contains(&chunk)
                    {
                        mapped.push(chunk);
                    }
                }
                mapped.sort_unstable();
                mapped.dedup();
                assert!(
                    !mapped.is_empty(),
                    "the reverse scan found no mapping names — the instrument is broken"
                );
                for name in &mapped {
                    assert!(
                        TOOL_NAMES.contains(name),
                        "mapping names '{name}', which is not a registered tool — a stale \
                         or misspelled arm in src/ontology.rs"
                    );
                }
            }

            /// The stamp is additive: the body's own fields survive, and
            /// both axis concepts appear under `ontology`. The probe
            /// takes one registered tool and asserts both axes plus the
            /// unmapped-name pass-through — every server's stamp contract
            /// is the same shape, so the test is defined once.
            #[test]
            fn annotate_stamps_both_axes_without_disturbing_the_body() {
                let body = serde_json::json!({"probe": 1});
                let probe = TOOL_NAMES
                    .first()
                    .copied()
                    .expect("a registered tool to probe with");
                let stamped = annotate(probe, body.clone());
                assert_eq!(stamped["probe"], serde_json::json!(1), "{stamped:?}");
                assert!(
                    stamped["ontology"]["pko_procedure"].is_string(),
                    "the stamp carries the process axis: {stamped}"
                );
                assert!(
                    stamped["ontology"]["dc_type"].is_string(),
                    "the stamp carries the state axis: {stamped}"
                );
                // An unmapped name returns the body unchanged — never an
                // invented axis.
                let untouched = annotate("not_a_registered_tool", body.clone());
                assert_eq!(untouched, body);
            }

            /// The combined mapping is defined only over the registered
            /// surface: an unknown name yields no dual-axis identity
            /// rather than a default pair. (Either axis may individually
            /// be total by design — a server's dc-table catch-all is
            /// legitimate; the COMBINED identity still requires the
            /// registered name to have both.)
            #[test]
            fn unknown_tool_has_no_dual_axis() {
                assert!(tool_to_dual_axis("not_a_tool").is_none());
            }
        }
    };
}

/// Define an MCP server struct with standard fields + constructor.
///
/// Generates the struct with a mandatory `webid` field plus any
/// domain-specific fields, a `new()` constructor, and a `ToolContext` impl.
///
/// # Example
/// ```ignore
/// mcp_server!(struct SkillServer {
///     inference_port: Arc<dyn InferencePort>,
///     skills: HashMap<String, SkillDef>,
/// });
/// ```
///
/// Expands to a struct with `webid, inference_port, skills`.
#[macro_export]
macro_rules! mcp_server {
    // Variant with custom fields
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            /// Process identity for ownership and server-domain operations.
            pub webid: hkask_types::WebID,
            $(
                $(#[$field_meta])*
                $field_vis $field : $ty
            ),*
        }

        impl $name {
            pub fn new(
                webid: hkask_types::WebID,
                $($field : $ty),*
            ) -> Self {
                Self { webid, $($field),* }
            }
        }

        impl $crate::server::ToolContext for $name {}
    };

    // Variant with custom fields AND a dual-axis annotate override
    // (P5.4): the server-local mapping stamps every response centrally.
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        } = $annotate:path $(;)?
    ) => {
        $(#[$meta])*
        $vis struct $name {
            /// Process identity for ownership and server-domain operations.
            pub webid: hkask_types::WebID,
            $(
                $(#[$field_meta])*
                $field_vis $field : $ty
            ),*
        }

        impl $name {
            pub fn new(
                webid: hkask_types::WebID,
                $($field : $ty),*
            ) -> Self {
                Self { webid, $($field,)* }
            }
        }

        impl $crate::server::ToolContext for $name {
            fn annotate_response(
                &self,
                tool_name: &str,
                body: serde_json::Value,
            ) -> serde_json::Value {
                $annotate(tool_name, body)
            }
        }
    };

    // Variant with no custom fields AND an annotate override (P5.4).
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident = $annotate:path $(;)?
    ) => {
        $(#[$meta])*
        $vis struct $name {
            /// Process identity for ownership and server-domain operations.
            pub webid: hkask_types::WebID,
        }

        impl $name {
            pub fn new(webid: hkask_types::WebID) -> Self {
                Self { webid }
            }
        }

        impl $crate::server::ToolContext for $name {
            fn annotate_response(
                &self,
                tool_name: &str,
                body: serde_json::Value,
            ) -> serde_json::Value {
                $annotate(tool_name, body)
            }
        }
    };
}
