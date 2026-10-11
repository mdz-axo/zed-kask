//! Build script — generates `tool_names.gen.rs` from `#[tool]` fn signatures.
//!
//! The scan, generation, and change-detection mechanics live in the shared
//! support module (`hkask-mcp-server/build_support/tool_names.rs`) — one
//! implementation for every server, not a per-server copy (the former
//! per-server scans drifted: fixed windows missed long doc blocks, local
//! patterns captured non-tool fns). The generated set is pinned against the
//! live router by `tool_surface_pin!` in this server's test module, so a
//! tool added/renamed/unrouted fails the pin instead of degrading to "tool
//! not found" at dispatch.

#[path = "../../crates/hkask-mcp-server/build_support/tool_names.rs"]
mod tool_names;

fn main() {
    tool_names::generate_tool_names();
}
