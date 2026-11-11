#![forbid(unsafe_code)]
//! Shared harness surface for MCP tool-behavior tests.
//!
//! Every server's `tests/tool_behavior.rs` drives its tools through the
//! `Parameters<T>` seam and parses the MCP tool-response envelope. This crate
//! carries that one shape so the per-server test files stop repeating it:
//! envelope parsing, the `invoke_tool!` macro that drives a tool and returns
//! its parsed envelope, and the shared test doubles that several servers'
//! end-to-end tests hand-rolled.
//!
//! The crate is test-only by usage: it is consumed through dev-dependencies,
//! so it never enters a production build. The per-server request types stay
//! in the server crates (the seam is the server's public tool method, and
//! the request construction is the test's specification).

pub use rmcp::handler::server::wrapper::Parameters;

/// Parse a tool response envelope into its JSON body.
pub fn parse_envelope(output: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(output)
        .unwrap_or_else(|| panic!("tool output must be valid JSON, got: {output}"))
}

/// Drive a tool through the `Parameters<T>` seam and parse its envelope.
///
/// Expands to the tool call, an `.expect` naming the tool, and envelope
/// parsing — the shape every tool-behavior test repeats. Error-path tests
/// keep their own `expect_err` calls.
#[macro_export]
macro_rules! invoke_tool {
    ($server:expr, $method:ident, $request:expr) => {{
        let response = $server
            .$method($crate::Parameters($request))
            .await
            .expect(concat!("tool ", stringify!($method), " succeeds"));
        $crate::parse_envelope(&response)
    }};
}

pub mod fixtures {
    pub mod noop_inference_port;
}
