//! Deterministic corpus-tag re-resolution — re-runs the shared
//! published-ontology resolver over the raw `candidate_terms` stored in a
//! tagged-chunks JSONL, with no inference and no embedding.
//!
//! The tagging pass is two separable halves: the LLM extracts free-text
//! descriptive terms (`tag-passages-batch.j2` forbids it from emitting
//! namespaces — "the server resolves published ontology anchors
//! deterministically"), and `canonicalize_terms` resolves them. When the
//! ontology registry grows (derived terms, rung-1 sources), the stored
//! `candidate_terms` are still valid LLM output — only the resolution is
//! stale. This example re-runs the pure resolver over the stored terms so a
//! registry update never forces a paid re-tag.
//!
//! Usage: reresolve_corpus_tags <input.jsonl> <output.jsonl> [--force]
//!
//! Identity fields are preserved verbatim: only the resolver-derived fields
//! (`ontology_tags`, `concepts`; `candidate_terms` only if normalization
//! itself changed it) are updated. Rows whose classification is not
//! `classified`, or that carry no `candidate_terms`, pass through untouched
//! — there is nothing to re-resolve. The output is refused if it exists
//! unless `--force` is passed, and the run fails closed if any classified
//! row fails `TaggedChunk::has_current_canonical_terms` after mutation.
//!
//! Verified live 2026-10-19 over the capabilities-researcher corpus
//! (67,177 rows): the run dropped the removed `fibo` namespace's 5,081
//! stale tags and re-anchored 6,284 additional rows onto non-core
//! namespaces (18,403 → 24,687 rich rows), gaining the newly pinned `cso`
//! (10,833 rows) and `dpv` (1,222 rows) rung-1 anchors.

use hkask_bridge_ontology::term_resolution::canonicalize_terms;
use hkask_types::corpus::TaggedChunk;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let paths: Vec<&String> = args.iter().filter(|a| *a != "--force").collect();
    if paths.len() != 2 {
        eprintln!("usage: reresolve_corpus_tags <input.jsonl> <output.jsonl> [--force]");
        return ExitCode::from(64);
    }
    let (input_path, output_path) = (paths[0], paths[1]);

    let input = match std::fs::File::open(input_path) {
        Ok(f) => std::io::BufReader::new(f),
        Err(e) => {
            eprintln!("cannot read input {input_path}: {e}");
            return ExitCode::from(2);
        }
    };
    if std::path::Path::new(output_path).exists() && !force {
        eprintln!("output {output_path} exists — pass --force to replace it");
        return ExitCode::from(2);
    }

    let mut out = match std::fs::File::create(output_path) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(e) => {
            eprintln!("cannot create output {output_path}: {e}");
            return ExitCode::from(2);
        }
    };

    // Metrics: before/after namespace histograms and reconciliation.
    let mut total: u64 = 0;
    let mut classified: u64 = 0;
    let mut passthrough: u64 = 0;
    let mut changed: u64 = 0;
    let mut rich_before: u64 = 0;
    let mut rich_after: u64 = 0;
    let mut ns_before: BTreeMap<String, u64> = BTreeMap::new();
    let mut ns_after: BTreeMap<String, u64> = BTreeMap::new();

    for (line_no, line) in input.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("input line {} read error: {e}", line_no + 1);
                return ExitCode::from(2);
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        total += 1;
        let mut row: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("input line {} is not valid JSON: {e}", line_no + 1);
                return ExitCode::from(2);
            }
        };

        // Only `classified` rows with candidate terms are re-resolvable;
        // everything else passes through byte-identical.
        let is_classified = row
            .get("classification")
            .and_then(|c| c.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|s| s == "classified");
        let has_terms = row
            .get("candidate_terms")
            .and_then(Value::as_array)
            .is_some_and(|t| !t.is_empty());

        if !(is_classified && has_terms) {
            passthrough += 1;
            writeln!(out, "{line}").expect("write output row");
            continue;
        }
        classified += 1;

        let terms: Vec<String> = row["candidate_terms"]
            .as_array()
            .expect("candidate_terms array checked above")
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
        let canonical = canonicalize_terms(&terms);

        let tags_before = row.get("ontology_tags").cloned().unwrap_or(Value::Null);
        if let Some(map) = tags_before.as_object() {
            if map.keys().any(|k| k != "core")
                || map.values().any(|v| {
                    v.as_array().is_some_and(|a| {
                        a.iter()
                            .any(|t| t.as_str().is_some_and(|s| s != "5w1h_core"))
                    })
                })
            {
                rich_before += 1;
            }
        }
        for ns in tags_before.as_object().into_iter().flat_map(|m| m.keys()) {
            *ns_before.entry(ns.clone()).or_default() += 1;
        }

        // Mutate only the resolver-derived fields. `candidate_terms` is the
        // resolver's input and is normally unchanged; copying it keeps the
        // row exactly what `canonicalize_terms` derived it from.
        row["candidate_terms"] =
            serde_json::to_value(&canonical.candidate_terms).expect("candidate terms serialize");
        row["ontology_tags"] =
            serde_json::to_value(&canonical.ontology_tags).expect("ontology tags serialize");
        row["concepts"] = serde_json::to_value(&canonical.concepts).expect("concepts serialize");

        let tags_after = row.get("ontology_tags").cloned().unwrap_or(Value::Null);
        if tags_after != tags_before {
            changed += 1;
        }
        let rich = tags_after.as_object().is_some_and(|map| {
            map.keys().any(|k| k != "core")
                || map.values().any(|v| {
                    v.as_array().is_some_and(|a| {
                        a.iter()
                            .any(|t| t.as_str().is_some_and(|s| s != "5w1h_core"))
                    })
                })
        });
        if rich {
            rich_after += 1;
        }
        for ns in tags_after.as_object().into_iter().flat_map(|m| m.keys()) {
            *ns_after.entry(ns.clone()).or_default() += 1;
        }

        // Fail closed: the mutated row must reconcile under the published
        // TaggedChunk's own gate before it is written.
        let tagged: TaggedChunk = match serde_json::from_value(row.clone()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "line {} does not deserialize as a TaggedChunk: {e}",
                    line_no + 1
                );
                return ExitCode::from(1);
            }
        };
        if !tagged.has_current_canonical_terms() {
            eprintln!(
                "line {} fails has_current_canonical_terms after re-resolution",
                line_no + 1
            );
            return ExitCode::from(1);
        }

        let serialized = serde_json::to_string(&row).expect("row serializes");
        writeln!(out, "{serialized}").expect("write output row");
    }
    out.flush().expect("flush output");

    println!("total rows:            {total}");
    println!("classified re-resolved: {classified}");
    println!("passthrough (untouched): {passthrough}");
    println!("rows changed:           {changed}");
    println!("rich before:            {rich_before}");
    println!("rich after:             {rich_after}");
    println!("namespaces before:      {ns_before:?}");
    println!("namespaces after:       {ns_after:?}");
    ExitCode::SUCCESS
}
