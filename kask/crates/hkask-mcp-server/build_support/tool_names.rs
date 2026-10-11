//! Shared filesystem and output mechanics for MCP tool-name build scripts.
//! Each server retains its own tool-selection and generated-header policy.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Select `#[tool]`-annotated `pub async fn` names from one Rust source.
///
/// State-machine scan: a `#[tool` line arms the collector; the collector
/// stays armed through the attribute's own continuation lines, multi-line
/// descriptions, and contract-doc blocks of ANY length (a former
/// fixed-window scan silently dropped `corpus_query`, whose 7-line doc
/// block exceeded the window — the name pin caught it, live 2026-10-05);
/// the first `pub async fn` while armed is captured and disarms. A block
/// close (`}`) disarms without capture — the armed attribute's fn is gone.
/// The `\b` boundary keeps `#[tool_router]` and `#[tool_handler]` from
/// arming the collector (live 2026-10-05: prediction-markets' scan
/// captured its `run` entry fn through `#[tool_handler]` — a false
/// positive the name pin caught). The generated set is verified against
/// the live router by `tool_surface_pin!`, so a mis-scan fails a test,
/// never ships.
pub fn scan_tool_fns(source: &str, names: &mut BTreeSet<String>) {
    let tool_attr = regex::Regex::new(r"#\[tool\b").expect("valid regex");
    let tool_fn = regex::Regex::new(r"(?:pub )?async fn (\w+)\s*\(").expect("valid regex");
    let mut armed = false;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if tool_attr.is_match(trimmed) {
            armed = true;
            continue;
        }
        if !armed {
            continue;
        }
        if let Some(name) = tool_fn.captures(trimmed).and_then(|c| c.get(1)) {
            names.insert(name.as_str().to_string());
            armed = false;
            continue;
        }
        if trimmed.starts_with('}') {
            armed = false;
        }
    }
}

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

/// The build-script path plumbing shared by every tool-names generator: the
/// crate's `src/` tree and the `tool_names.gen.rs` output path under cargo's
/// `OUT_DIR`. Servers with extra generated sets (swarm's cloud-router split)
/// build on this plus the part fns; every other server delegates to
/// [`generate_tool_names`].
pub fn src_and_out_paths() -> (PathBuf, PathBuf) {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let src_dir = Path::new(&manifest_dir).join("src");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let out_path = Path::new(&out_dir).join("tool_names.gen.rs");
    (src_dir, out_path)
}

/// The standard tool-names generation pass: scan `src/**/*.rs` for
/// `#[tool]`-annotated `pub async fn` signatures and emit
/// `pub const TOOL_NAMES` to `OUT_DIR/tool_names.gen.rs`.
/// One implementation for every server's build script — each driver is a
/// one-line delegation, not a per-server copy (the former per-server scans
/// drifted: fixed windows missed long doc blocks, local patterns captured
/// non-tool fns). The generated set is pinned against the live router by
/// `tool_surface_pin!` in each server's test module.
//
// `allow(dead_code)`: this module is `#[path]`-included into every server's
// build script, and not every including unit uses every fn — swarm's driver
// builds on `src_and_out_paths` + the part fns instead of delegating, so its
// compilation sees this as dead. The twelve delegating servers are the real
// consumers.
#[allow(dead_code)]
pub fn generate_tool_names() {
    let (src_dir, out_path) = src_and_out_paths();

    let mut names: BTreeSet<String> = BTreeSet::new();
    for source in rust_sources(&src_dir, true) {
        scan_tool_fns(&source, &mut names);
    }

    let entries = entries(&names);
    let generated = format!(
        "// Auto-generated by build.rs — do not edit.\n\
         // Scans `src/**/*.rs` for `#[tool]`-annotated `pub async fn` signatures.\n\
         pub const TOOL_NAMES: &[&str] = &[\n{entries}];\n"
    );

    write_if_changed(&out_path, &generated, &src_dir);
}
