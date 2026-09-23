//! Shared filesystem and output mechanics for MCP tool-name build scripts.
//! Each server retains its own tool-selection and generated-header policy.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn rust_sources(dir: &Path, recursive: bool) -> Vec<String> {
    fn collect(dir: &Path, recursive: bool, sources: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("src directory exists") {
            let entry = entry.expect("readable dir entry");
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    collect(&path, recursive, sources);
                }
                continue;
            }
            if path.extension().is_some_and(|ext| ext == "rs") {
                sources.push(
                    fs::read_to_string(&path).unwrap_or_else(|error| {
                        panic!("failed to read {}: {error}", path.display())
                    }),
                );
            }
        }
    }

    let mut sources = Vec::new();
    collect(dir, recursive, &mut sources);
    sources
}

pub fn entries(names: &BTreeSet<String>) -> String {
    let mut entries = String::new();
    for name in names {
        entries.push_str(&format!("    \"{name}\",\n"));
    }
    entries
}

pub fn write_if_changed(out_path: &Path, generated: &str, src_dir: &Path) {
    let existing = fs::read_to_string(out_path).unwrap_or_default();
    if existing != generated {
        fs::write(out_path, generated).expect("writable OUT_DIR");
    }
    println!("cargo:rerun-if-changed={}", src_dir.display());
    println!(
        "cargo:rerun-if-changed={}",
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/hkask-mcp-server/build_support/tool_names.rs")
            .display()
    );
}
