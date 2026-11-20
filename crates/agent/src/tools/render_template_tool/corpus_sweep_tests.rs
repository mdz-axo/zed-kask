use super::*;

/// Corpus-wide pin: every shipped .j2 template, after stripping, must not
/// leak its `[inference]` header, contract block, or visibility line into
/// the renderable body — and must still contain its prose. This is the
/// C1 finding's enforcement point: the old stripper fired on 0 of 309
/// templates because it required a leading `---`.
#[test]
fn corpus_templates_strip_without_leaking_metadata() {
    let base =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../kask/registry/templates");
    let base = base.canonicalize().expect("templates dir exists in repo");

    let mut checked = 0usize;
    let mut leaked = Vec::new();
    let mut empty = Vec::new();
    for entry in walkdir(&base) {
        let content = std::fs::read_to_string(&entry).expect("read template");
        let stripped = strip_frontmatter(&content);
        checked += 1;

        // The header's load-bearing markers must never survive as
        // leading metadata. Checked only in the first few lines — the
        // word `contract:` legitimately appears mid-prose later (e.g.
        // prose that says "verify the contract: it enforces…").
        for (idx, line) in stripped.lines().take(5).enumerate() {
            let trimmed_line = line.trim();
            if trimmed_line.starts_with("visibility:")
                || trimmed_line.starts_with("contract:")
                // Bracketed header markers: the algedonic templates'
                // TOML-style [contract] block leaked wholesale when the
                // header lacked its `---` terminator (fixed 2026-09-09).
                || trimmed_line.starts_with("[contract]")
                || trimmed_line.starts_with("[/contract]")
                // Manifest residue: a KnowAct-style manifest block after
                // leading comments survives stripping (the
                // logo-formal-prompt leak, fixed 2026-09-09) and always
                // carries template_type: first.
                || trimmed_line.starts_with("template_type:")
            {
                leaked.push(format!(
                    "{}: line {} leaked header metadata `{}`",
                    entry.display(),
                    idx,
                    trimmed_line
                ));
            }
        }
        // A leading [inference] stanza must be gone (a mid-prose mention
        // is allowed).
        if stripped.trim_start().starts_with("[inference]") {
            leaked.push(format!(
                "{}: leading [inference] stanza survived",
                entry.display()
            ));
        }
        // A template must retain SOME body — a stripper that eats
        // everything is worse than one that eats nothing.
        if stripped.is_empty() {
            empty.push(entry.display().to_string());
        }
    }

    assert!(
        checked > 200,
        "corpus scan found only {checked} templates — scan path broken"
    );
    assert!(
        leaked.is_empty(),
        "{} templates leak metadata after stripping:\n{}",
        leaked.len(),
        leaked.join("\n")
    );
    assert!(
        empty.is_empty(),
        "{} templates strip to empty (over-stripping):\n{}",
        empty.len(),
        empty.join("\n")
    );
}

/// Corpus-wide pin: every shipped .j2 template must RENDER without a
/// context-independent defect under the same environment the tool uses
/// (lenient undefined behavior, registry-rooted loader). Stripping-clean
/// is not enough — triage.j2 stripped clean but failed at render time
/// with an unknown filter (`truncate`), and extract-hmems.j2's bare
/// `[inference]` marker made the stanza stripper eat its `{% if %}`
/// (orphaning the `{% else %}`) — both live-observed 2026-09-09; only a
/// full render catches the class.
///
/// Classification: only syntax errors, unknown filters/tests/functions/
/// methods, bad escapes, and broken includes are defects — they fail
/// regardless of context. Operation-on-undefined (InvalidOperation,
/// UndefinedError, …) with an EMPTY context is the template's context
/// contract failing loud — correct behavior; the caller retries with
/// the required variables. Those are allowed here.
#[test]
fn corpus_templates_render_without_error() {
    let base =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../kask/registry/templates");
    let base = base.canonicalize().expect("templates dir exists in repo");

    let mut checked = 0usize;
    let mut failed = Vec::new();
    for entry in walkdir(&base) {
        let content = std::fs::read_to_string(&entry).expect("read template");
        let stripped = strip_frontmatter(&content);
        checked += 1;
        let mut env = minijinja::Environment::new();
        bind_registry_loader(&mut env, &base);
        if let Err(err) = env.render_str(&stripped, serde_json::json!({})) {
            match err.kind() {
                minijinja::ErrorKind::SyntaxError
                | minijinja::ErrorKind::UnknownFilter
                | minijinja::ErrorKind::UnknownTest
                | minijinja::ErrorKind::UnknownFunction
                | minijinja::ErrorKind::UnknownMethod
                | minijinja::ErrorKind::BadEscape
                | minijinja::ErrorKind::BadInclude
                | minijinja::ErrorKind::TemplateNotFound => {
                    failed.push(format!("{}: {err}", entry.display()));
                }
                _ => {}
            }
        }
    }

    assert!(
        checked > 200,
        "corpus scan found only {checked} templates — scan path broken"
    );
    assert!(
        failed.is_empty(),
        "{} of {checked} templates fail to render:\n  {}",
        failed.len(),
        failed.join("\n  ")
    );
}

/// Generic, not a name list: every `{% include %}` directive in the
/// corpus must resolve through the registry loader and render without
/// leaking the included template's header. Include names are extracted
/// from the shipped templates at test time — file names belong to the
/// skills that own them, not to this crate.
#[test]
fn corpus_includes_resolve_and_strip() {
    let base =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../kask/registry/templates");
    let base = base.canonicalize().expect("templates dir exists in repo");

    let mut checked = 0usize;
    let mut failed = Vec::new();
    for entry in walkdir(&base) {
        let content = std::fs::read_to_string(&entry).expect("read template");
        for (line_idx, line) in content.lines().enumerate() {
            let Some(include_name) = extract_include_name(line) else {
                continue;
            };
            checked += 1;
            let mut env = minijinja::Environment::new();
            bind_registry_loader(&mut env, &base);
            match env.render_str(
                &format!("{{% include \"{include_name}\" %}}"),
                serde_json::json!({}),
            ) {
                Ok(rendered) => {
                    // Leading header markers must not survive — the same
                    // leak convention as the strip sweep (first lines
                    // only; mid-prose mentions are legitimate).
                    for (idx, out_line) in rendered.lines().take(5).enumerate() {
                        let trimmed = out_line.trim();
                        if trimmed.starts_with("[inference]")
                            || trimmed.starts_with("[contract]")
                            || trimmed.starts_with("template_type:")
                        {
                            failed.push(format!(
                                "{} line {}: include `{include_name}` leaks `{trimmed}` at output line {}",
                                entry.display(),
                                line_idx + 1,
                                idx
                            ));
                        }
                    }
                }
                Err(err) => {
                    failed.push(format!(
                        "{} line {}: include `{include_name}` failed: {err}",
                        entry.display(),
                        line_idx + 1
                    ));
                }
            }
        }
    }
    assert!(
        checked > 0,
        "no {{% include %}} directives found in the corpus — the scan is \
         broken or includes were removed (delete this test with the last \
         include; do not leave it vacuous)"
    );
    assert!(
        failed.is_empty(),
        "{} include failures across the corpus:\n  {}",
        failed.len(),
        failed.join("\n  ")
    );
}

/// Extract the include target from a `{% include "name" %}` line, if any.
fn extract_include_name(line: &str) -> Option<String> {
    let start = line.find("{%")?;
    let rest = &line[start..];
    let end = rest.find("%}")?;
    let tag = &rest[..end];
    if !tag
        .trim_start_matches("{%")
        .trim_start()
        .starts_with("include")
    {
        return None;
    }
    let q1 = tag.find('"')?;
    let after = &tag[q1 + 1..];
    let q2 = after.find('"')?;
    Some(after[..q2].to_string())
}

/// Recursive .j2 walk (mirrors agent_skills/build.rs's collector).
fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            out.extend(walkdir(&path));
        } else if path.extension().is_some_and(|e| e == "j2") {
            out.push(path);
        }
    }
    out
}
