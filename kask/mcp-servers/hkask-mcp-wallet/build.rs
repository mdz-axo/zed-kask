//! Build script — generates `tool_names.gen.rs` from `#[tool]` fn signatures.

#[path = "../../crates/hkask-mcp-server/build_support/tool_names.rs"]
mod tool_names;

fn main() {
    tool_names::generate_tool_names();
}
