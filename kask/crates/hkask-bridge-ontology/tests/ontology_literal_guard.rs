//! Ontology literal guard — closes the string-literal side door.
//!
//! **The threat this guard exists for:** the bridge crate's fixture tests
//! (`all_terms_are_official` in every vocabulary module) verify that each
//! *constant* is a real term in a published ontology. But server code that
//! writes an ontology URI as a *string literal* bypasses those fixtures
//! entirely — five fabricated URIs (`dcterms:Assertion`,
//! `pko:StepExecution.output`, `pko:stepVerification`,
//! `pko:referencesResource`, `pko:Goal`) survived the entire module
//! remediation through that door, and the corpus pipeline shipped five more
//! fabricated schema.org predicates (`schema:causes`, `schema:resultOf`,
//! `schema:uses`, `schema:method`, `schema:subject`) plus a fabricated
//! `rdf:creator` the same way.
//!
//! **What this guard enforces — routing, not reality.** Every
//! ontology-shaped string literal under the scanned source trees must be an
//! explicit, reviewed entry in `ALLOWED_LITERALS` below. The honest way to
//! use an ontology term in code is a reference to a bridge constant (which
//! leaves no literal for this scan to find); a literal is the side door and
//! must be justified. This test cannot verify that a URI is *real* — that is
//! the fixtures' job. An allowlist entry asserts its term is
//! fixture-verified; the fixture tests remain the validity gate.
//!
//! **Scanned trees** (production + tests, both workspaces):
//! `kask/mcp-servers/*/src`, `kask/crates/*/src`, `crates/hkask-*/src` —
//! excluding the bridge crate itself, whose constants are the fixture-guarded
//! home for these literals. Comments are skipped: only string literals are
//! extracted (a hand-rolled scanner tracks line/block comments, string
//! escapes, raw strings, and char-vs-lifetime quotes).
//!
//! **Templates** (`.j2` under `kask/registry/templates/`) cannot reference
//! Rust constants, so they get a stronger rule: every ontology-shaped term
//! in a template must appear in one of the bridge crate's fixture term
//! lists — templates may only speak fixture-verified terms. This is
//! mechanical drift prevention between the registry templates and the
//! vocabulary modules.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Ontology-URI shape: one of the bridge namespaces, a colon, and a term.
/// Case-insensitive on the namespace (fabrications can be any case); the
/// term may contain letters, digits, underscores, and hyphens
/// (`dlp:participant-in`, `gc:GP1i_has_Character`).
const ONTOLOGY_TERM_PATTERN: &str = r"(?i)(pko|pplan|prov|fibo|golem|gc|crm|dlp|lrmoo|SEPIO|IAO|sumo|omc|mls|sdmx|dcterms|bibo|dcmitype|cito|schema|rdf):[A-Za-z][A-Za-z0-9_-]*";

/// Explicit, reviewed allowlist: (path from repo root, ontology term).
///
/// Every entry is a TEST FIXTURE pinning a wire contract — the literal is the
/// point: it independently asserts the exact URI the corresponding MCP
/// server emits on its payload, so a constant change on the server side
/// fails the widget test instead of moving silently with it. Each term is
/// fixture-verified in `kask/crates/hkask-bridge-ontology/fixtures/`:
/// - `dcterms:Dataset`, `fibo:Portfolio` — dublincore-bibo-cito-terms.txt,
///   fibo-verified-terms.txt
/// - `pko:Step`, `pko:Procedure` — pko-2.0.0-terms.txt
/// - `omc:CreativeWork`, `omc:Scene`, `omc:Asset`, `omc:Sequence` —
///   omc-v2.8-terms.txt
/// - `fibo:Corporation` — fibo-verified-terms.txt
///
/// Production code may NOT add entries here — reference a bridge constant
/// instead. New entries require a justification comment naming the verified
/// source, per the PR-review rule.
const ALLOWED_LITERALS: &[(&str, &str)] = &[
    // ── Widget/crate test fixtures pinning server-emitted ontology tags ──
    ("crates/hkask-graph-widget/src/block.rs", "dcterms:Dataset"),
    ("crates/hkask-kanban-widget/src/view.rs", "pko:Step"),
    (
        "crates/hkask-media-widget/src/media_ref.rs",
        "omc:CreativeWork",
    ),
    (
        "crates/hkask-media-widget/src/media_widget.rs",
        "omc:CreativeWork",
    ),
    ("crates/hkask-media-widget/src/media_widget.rs", "omc:Scene"),
    ("crates/hkask-media-widget/src/media_widget.rs", "omc:Asset"),
    (
        "crates/hkask-media-widget/src/media_widget.rs",
        "omc:Sequence",
    ),
    (
        "crates/hkask-media-widget/src/media_widget.rs",
        "fibo:Corporation",
    ),
    (
        "crates/hkask-portfolio-widget/src/block.rs",
        "fibo:Portfolio",
    ),
    ("crates/hkask-scenarios-widget/src/view.rs", "pko:Procedure"),
    ("crates/hkask-swarm-widget/src/block.rs", "pko:Procedure"),
    (
        "crates/hkask-viz-core/src/hkask_viz_core.rs",
        "dcterms:Dataset",
    ),
    (
        "crates/hkask-viz-core/src/hkask_viz_core.rs",
        "fibo:Portfolio",
    ),
    (
        "crates/hkask-viz-core/src/hkask_viz_core.rs",
        "pko:Procedure",
    ),
    ("crates/hkask-viz-core/src/hkask_viz_core.rs", "pko:Step"),
    // media_block.rs tests pin the ```media block wire format (the string
    // the widget parses) — the literal URI in the block payload is the
    // contract being pinned.
    (
        "kask/mcp-servers/hkask-mcp-media/src/media_block.rs",
        "omc:CreativeWork",
    ),
    (
        "kask/mcp-servers/hkask-mcp-media/src/media_block.rs",
        "omc:VersionInfo",
    ),
    (
        "kask/mcp-servers/hkask-mcp-media/src/media_block.rs",
        "omc:Capture",
    ),
];

/// Repo root (this crate lives at `<root>/kask/crates/hkask-bridge-ontology`).
fn repo_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."))
}

/// Recursively collect `.rs` files under `dir`.
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The source trees this guard scans: both workspaces' kask crates and MCP
/// servers, excluding the bridge crate itself (its constants ARE the
/// fixture-guarded literals this guard exists to route around).
fn scanned_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for scope in [
        "kask/mcp-servers", // every server's src/
        "kask/crates",      // every kask-workspace crate's src/
        "crates",           // root-workspace kask widgets (hkask-*)
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(scope)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Root-workspace scope: only the kask widgets (hkask-*), not
            // upstream zed crates.
            if scope == "crates" && !name.starts_with("hkask-") {
                continue;
            }
            let src = path.join("src");
            if src.is_dir() {
                collect_rust_files(&src, &mut files);
            }
        }
    }
    files.retain(|f| !f.starts_with(root.join("kask/crates/hkask-bridge-ontology")));
    files
}

/// A string literal extracted from Rust source: (line, contents).
struct StringLiteral {
    line: usize,
    text: String,
}

/// Extract the contents of every string literal in `src`, skipping comments.
///
/// A hand-rolled scanner (no syn dependency — this must stay a cheap
/// guard, not a parsing pipeline) that tracks:
/// - line comments (`//`) and nested block comments (`/* */`)
/// - escaped string contents (`"..."`)
/// - raw strings (`r#"..."#`, any hash depth)
/// - char literals (`'x'`, `'\n'`) vs lifetimes (`'a`)
fn extract_string_literals(src: &str) -> Vec<StringLiteral> {
    let mut out = Vec::new();
    let mut state = State::Code;
    let mut block_depth = 0usize;
    let mut raw_hashes = 0usize;
    let mut literal_line = 0usize;
    let mut line = 1usize;
    let mut buf = String::new();
    let mut i = 0usize;
    while i < src.len() {
        let rest = &src[i..];
        match state {
            State::Code => {
                if rest.starts_with("//") {
                    state = State::LineComment;
                    i += 2;
                } else if rest.starts_with("/*") {
                    state = State::BlockComment;
                    block_depth = 1;
                    i += 2;
                } else if rest.starts_with('"') {
                    state = State::String;
                    literal_line = line;
                    buf.clear();
                    i += 1;
                } else if let Some(hashes) = raw_string_open(rest) {
                    state = State::RawString;
                    raw_hashes = hashes;
                    literal_line = line;
                    buf.clear();
                    i += 1 + hashes + 1; // r, hashes, quote
                } else if rest.starts_with('\'') {
                    // Char literal ('x' or '\x') vs lifetime ('a): a char
                    // literal closes within 2-4 bytes; a lifetime does not.
                    if rest.starts_with('\\') && rest.len() > 3 && rest.as_bytes()[3] == b'\'' {
                        i += 4;
                    } else if rest.len() > 2 && rest.as_bytes()[2] == b'\'' {
                        i += 3;
                    } else {
                        i += 1; // lifetime — not a literal
                    }
                } else {
                    // Advance by full UTF-8 chars — byte-wise stepping can
                    // land inside a multi-byte character and panic on the
                    // next slice.
                    if rest.starts_with('\n') {
                        line += 1;
                    }
                    i += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
            State::LineComment => {
                if rest.starts_with('\n') {
                    state = State::Code;
                    line += 1;
                }
                i += rest.chars().next().map_or(1, char::len_utf8);
            }
            State::BlockComment => {
                if rest.starts_with("/*") {
                    block_depth += 1;
                    i += 2;
                } else if rest.starts_with("*/") {
                    block_depth -= 1;
                    i += 2;
                    if block_depth == 0 {
                        state = State::Code;
                    }
                } else {
                    if rest.starts_with('\n') {
                        line += 1;
                    }
                    i += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
            State::String => {
                if rest.starts_with('\\') {
                    buf.push_str(&rest[..2]);
                    i += 2;
                } else if rest.starts_with('"') {
                    out.push(StringLiteral {
                        line: literal_line,
                        text: buf.clone(),
                    });
                    state = State::Code;
                    i += 1;
                } else {
                    if rest.starts_with('\n') {
                        line += 1;
                    }
                    buf.push(rest.chars().next().unwrap_or_default());
                    i += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
            State::RawString => {
                let close = format!("\"{}", "#".repeat(raw_hashes));
                if rest.starts_with(&close) {
                    out.push(StringLiteral {
                        line: literal_line,
                        text: buf.clone(),
                    });
                    state = State::Code;
                    i += close.len();
                } else {
                    if rest.starts_with('\n') {
                        line += 1;
                    }
                    buf.push(rest.chars().next().unwrap_or_default());
                    i += rest.chars().next().map_or(1, char::len_utf8);
                }
            }
        }
    }
    out
}

enum State {
    Code,
    LineComment,
    BlockComment,
    String,
    RawString,
}

/// If `rest` opens a raw string (`r"`, `r#"`, …), return its hash count.
fn raw_string_open(rest: &str) -> Option<usize> {
    let mut chars = rest.chars();
    if chars.next()? != 'r' {
        return None;
    }
    let mut hashes = 0usize;
    for c in chars.by_ref() {
        if c == '#' {
            hashes += 1;
        } else if c == '"' {
            return Some(hashes);
        } else {
            return None;
        }
    }
    None
}

/// Every ontology-shaped term in a source file's string literals:
/// (line, term).
fn ontology_terms_in_literals(src: &str) -> Vec<(usize, String)> {
    let re = regex::Regex::new(ONTOLOGY_TERM_PATTERN).expect("valid pattern");
    extract_string_literals(src)
        .into_iter()
        .flat_map(|lit| {
            let line_number = lit.line;
            re.find_iter(&lit.text)
                .map(move |m| (line_number, m.as_str().to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The set of verified terms across all bridge fixtures (first
/// whitespace-separated token of each non-comment line).
fn fixture_terms() -> HashSet<String> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut terms = HashSet::new();
    let Ok(entries) = std::fs::read_dir(&fixtures_dir) else {
        panic!("fixtures dir not found: {}", fixtures_dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "txt") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            panic!("failed to read fixture {}", path.display());
        };
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(term) = trimmed.split_whitespace().next() {
                terms.insert(term.to_string());
            }
        }
    }
    assert!(!terms.is_empty(), "no fixture terms loaded");
    terms
}

/// Every ontology-shaped term in any text (used for templates, where the
/// whole file — comments included — is instruction surface).
fn ontology_terms_in_text(text: &str) -> Vec<String> {
    let re = regex::Regex::new(ONTOLOGY_TERM_PATTERN).expect("valid pattern");
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

/// Normalize a matched term to the canonical casing the allowlist and
/// fixtures use: namespace prefixes are matched case-insensitively, so
/// restore the canonical bridge casing for comparison.
fn canonical_term(term: &str) -> String {
    let (namespace, rest) = term.split_once(':').unwrap_or((term, ""));
    let lower = namespace.to_lowercase();
    let canonical_namespace = match lower.as_str() {
        "sepio" => "SEPIO",
        "iao" => "IAO",
        other => other,
    };
    format!("{canonical_namespace}:{rest}")
}

#[test]
fn ontology_literals_route_through_constants_or_allowlist() {
    let root = repo_root();
    let allowed: HashSet<(&str, &str)> = ALLOWED_LITERALS.iter().copied().collect();
    let mut violations = Vec::new();

    for file in scanned_files() {
        let Ok(src) = std::fs::read_to_string(&file) else {
            panic!("failed to read {}", file.display());
        };
        let relative = file
            .strip_prefix(root)
            .expect("scanned file under repo root")
            .to_string_lossy()
            .replace('\\', "/");
        for (line, term) in ontology_terms_in_literals(&src) {
            let canonical = canonical_term(&term);
            if !allowed.contains(&(relative.as_str(), canonical.as_str())) {
                violations.push(format!("{relative}:{line}: {term}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "ontology URI string literals found outside the allowlist:\n  {}\n\
         \n\
         The fixture tests guard the bridge constants; string literals are the\n\
         unguarded side door (five fabrications survived the module remediation\n\
         through it). Fix: reference a bridge-crate constant\n\
         (hkask_bridge_ontology::<module>::<CONSTANT>) instead of a literal.\n\
         If the literal is a reviewed wire-contract pin, add it to\n\
         ALLOWED_LITERALS in this test with a justification naming the\n\
         verified source.",
        violations.join("\n  ")
    );
}

#[test]
fn template_terms_are_fixture_verified() {
    let root = repo_root();
    let fixtures = fixture_terms();
    let templates_dir = root.join("kask/registry/templates");
    let mut files = Vec::new();
    collect_j2_files(&templates_dir, &mut files);
    assert!(
        !files.is_empty(),
        "no .j2 templates found under {} — the registry moved?",
        templates_dir.display()
    );

    let mut violations = Vec::new();
    for file in files {
        let Ok(content) = std::fs::read_to_string(&file) else {
            panic!("failed to read {}", file.display());
        };
        let relative = file
            .strip_prefix(root)
            .expect("template under repo root")
            .to_string_lossy()
            .replace('\\', "/");
        for term in ontology_terms_in_text(&content) {
            let canonical = canonical_term(&term);
            if !fixtures.contains(&canonical) {
                violations.push(format!("{relative}: {term}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "ontology terms in registry templates that are not in any bridge fixture:\n  {}\n\
         \n\
         Templates cannot reference Rust constants, so every ontology term in\n\
         a template must appear in a bridge fixture term list\n\
         (kask/crates/hkask-bridge-ontology/fixtures/*.txt) — the verified\n\
         term sets. A term that is not in a published ontology must not be\n\
         offered to the LLM, even in a comment.",
        violations.join("\n  ")
    );
}

/// Recursively collect `.j2` files.
fn collect_j2_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_j2_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "j2") {
            out.push(path);
        }
    }
}

/// The scanner must actually catch violations — a guard that cannot fail is
/// worthless. Synthetic source exercising every scanner state: a fabricated
/// URI in a string literal MUST be caught; the same URI in comments (line,
/// block, doc) MUST NOT be; raw strings and escapes must not confuse it.
#[test]
fn scanner_catches_literals_not_comments() {
    let src = r##"// pko:FakeInLineComment must not match
/* pko:FakeInBlockComment must not match */
/// Doc comment: schema:notARealTerm must not match.
fn main() {
    let a = "pko:FakeInString";
    let b = r#"schema:FakeInRawString"#;
    let c = "escaped \" quote then gc:FakeAfterEscape";
    let d = 'x';
    let e: &'static str = "lifetime then dcterms:FakeAfterLifetime";
}
"##;
    let terms = ontology_terms_in_literals(src);
    let caught: Vec<&str> = terms.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        caught,
        [
            "pko:FakeInString",
            "schema:FakeInRawString",
            "gc:FakeAfterEscape",
            "dcterms:FakeAfterLifetime",
        ],
        "scanner must extract exactly the string-literal terms"
    );
}

/// The allowlist must not contain entries for files that no longer exist —
/// a stale entry silently widens the guard's blind spot.
#[test]
fn allowlist_entries_point_at_real_files() {
    let root = repo_root();
    for (path, _term) in ALLOWED_LITERALS {
        assert!(
            root.join(path).is_file(),
            "allowlist entry points at missing file: {path}"
        );
    }
}
