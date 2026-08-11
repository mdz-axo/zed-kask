//! Property tests for the pure functions of the `hkask-inference` crate.
//!
//! These tests replaced the deleted `mod tests` blocks that used `MockProvider`
//! and `MockExecutor` stubs. They target *pure* functions only — `MediaOp`
//! parsing, `ProviderScore::weighted`, the fal-workflow DAG utilities
//! (`workflow::topological_sort_graph`, `resolve_references`, `extract_urls`,
//! `validate_workflow_structure`, `parse_workflow_nodes`), and
//! `RouterModelEntry::infer_vision_support` — using proptest generators and the
//! `hkask-test-harness` Oracle taxonomy (invariant + reference oracles).
//!
//! No stub, fake, mock, or noop providers/executors are constructed.

#![forbid(unsafe_code)]

use hkask_inference::fal_workflow::{
    ExecutionMode, WorkflowNode, extract_urls, parse_workflow_nodes, resolve_references,
    validate_workflow_structure,
};
use hkask_inference::scoring::{ProviderScore, ScoreWeights};
use hkask_inference::workflow::topological_sort_graph;
use hkask_inference::{GraphNode, MediaOp, RouterModelEntry};
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use proptest::prelude::*;
use serde_json::Value;
use std::str::FromStr;

// ── MediaOp parse/serialize round-trip ────────────────────────────────────

/// The canonical string names of every `MediaOp` variant, in declaration order.
const KNOWN_OP_STRINGS: &[&str] = &[
    "generate_image",
    "image_to_image",
    "remove_background",
    "upscale",
    "generate_video",
    "image_to_video",
    "segment_object",
    "generate_speech",
    "transcribe",
    "execute_workflow",
];

/// Strategy yielding either a known op string or an arbitrary string, so a
/// single proptest covers both the accept and reject paths of `from_str`.
fn arb_op_string() -> BoxedStrategy<String> {
    let known = prop_oneof![
        Just(KNOWN_OP_STRINGS[0].to_string()),
        Just(KNOWN_OP_STRINGS[1].to_string()),
        Just(KNOWN_OP_STRINGS[2].to_string()),
        Just(KNOWN_OP_STRINGS[3].to_string()),
        Just(KNOWN_OP_STRINGS[4].to_string()),
        Just(KNOWN_OP_STRINGS[5].to_string()),
        Just(KNOWN_OP_STRINGS[6].to_string()),
        Just(KNOWN_OP_STRINGS[7].to_string()),
        Just(KNOWN_OP_STRINGS[8].to_string()),
        Just(KNOWN_OP_STRINGS[9].to_string()),
    ];
    prop_oneof![known, any::<String>()].boxed()
}

proptest! {
    /// `MediaOp::from_str` accepts exactly the canonical op strings (round-trips
    /// through `as_str`) and rejects everything else with an `Err`. The oracle
    /// re-derives the expected verdict independently of the implementation.
    #[test]
    fn prop_media_op_parse_roundtrip(s in arb_op_string()) {
        let parsed = MediaOp::from_str(&s);
        let output = match &parsed {
            Ok(op) => serde_json::json!({ "ok": true, "as_str": op.as_str() }),
            Err(_) => serde_json::json!({ "ok": false, "as_str": serde_json::Value::Null }),
        };
        let input = serde_json::json!({ "s": s });

        let oracle = oracle_invariant(|input, output| {
            let s = input["s"].as_str().ok_or("s not str")?;
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            let expected_ok = KNOWN_OP_STRINGS.contains(&s);
            if ok != expected_ok {
                return Err(format!("from_str({s:?}) ok={ok}, expected {expected_ok}"));
            }
            if ok {
                let as_str = output["as_str"].as_str().ok_or("as_str missing")?;
                if as_str != s {
                    return Err(format!("round-trip as_str {as_str:?} != input {s:?}"));
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── ProviderScore::weighted ───────────────────────────────────────────────

proptest! {
    /// `ProviderScore::weighted` is the dot product of the 7-dimension score
    /// and the 7-dimension weights. The reference oracle recomputes the dot
    /// product independently and compares — catching any mis-wired dimension.
    #[test]
    fn prop_provider_score_weighted_is_dot_product(
        task_fit in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        quality in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        control in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        reliability in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        cost in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        latency in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        continuity in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_task_fit in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_quality in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_control in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_reliability in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_cost in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_latency in any::<f64>().prop_filter("finite", |f| f.is_finite()),
        w_continuity in any::<f64>().prop_filter("finite", |f| f.is_finite()),
    ) {
        let score = ProviderScore { task_fit, quality, control, reliability, cost, latency, continuity };
        let weights = ScoreWeights { task_fit: w_task_fit, quality: w_quality, control: w_control, reliability: w_reliability, cost: w_cost, latency: w_latency, continuity: w_continuity };
        let produced = score.weighted(&weights);

        let input = serde_json::json!({
            "score": { "task_fit": task_fit, "quality": quality, "control": control,
                       "reliability": reliability, "cost": cost, "latency": latency, "continuity": continuity },
            "weights": { "task_fit": w_task_fit, "quality": w_quality, "control": w_control,
                         "reliability": w_reliability, "cost": w_cost, "latency": w_latency, "continuity": w_continuity },
        });
        let output = serde_json::json!({ "weighted": produced });

        let oracle = oracle_reference(|input| {
            let s = &input["score"];
            let w = &input["weights"];
            let dot = s["task_fit"].as_f64().unwrap_or(0.0) * w["task_fit"].as_f64().unwrap_or(0.0)
                + s["quality"].as_f64().unwrap_or(0.0) * w["quality"].as_f64().unwrap_or(0.0)
                + s["control"].as_f64().unwrap_or(0.0) * w["control"].as_f64().unwrap_or(0.0)
                + s["reliability"].as_f64().unwrap_or(0.0) * w["reliability"].as_f64().unwrap_or(0.0)
                + s["cost"].as_f64().unwrap_or(0.0) * w["cost"].as_f64().unwrap_or(0.0)
                + s["latency"].as_f64().unwrap_or(0.0) * w["latency"].as_f64().unwrap_or(0.0)
                + s["continuity"].as_f64().unwrap_or(0.0) * w["continuity"].as_f64().unwrap_or(0.0);
            serde_json::json!({ "weighted": dot })
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── resolve_references ────────────────────────────────────────────────────

/// True iff `value` contains any string leaf beginning with `$` (the marker
/// `resolve_references` treats as a reference to be resolved).
fn contains_dollar_string(value: &Value) -> bool {
    match value {
        Value::String(s) => s.starts_with('$'),
        Value::Object(obj) => obj.values().any(contains_dollar_string),
        Value::Array(arr) => arr.iter().any(contains_dollar_string),
        _ => false,
    }
}

proptest! {
    /// With empty results and empty `depends`, `resolve_references` returns
    /// `Ok(clone)` iff the value contains no `$`-prefixed string; any such
    /// string makes it return `Err` (unresolved reference). It never panics.
    #[test]
    fn prop_resolve_references_no_deps(value in arb_json_value()) {
        let results: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        let depends: Vec<String> = Vec::new();
        let outcome = resolve_references(&value, &results, &depends);

        let output = match &outcome {
            Ok(v) => serde_json::json!({ "ok": true, "value": v.clone() }),
            Err(_) => serde_json::json!({ "ok": false, "value": serde_json::Value::Null }),
        };
        let input = serde_json::json!({ "value": value });

        let oracle = oracle_invariant(|input, output| {
            let value = &input["value"];
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            let has_dollar = contains_dollar_string(value);
            if ok == has_dollar {
                return Err(format!(
                    "ok={ok} but has_dollar={has_dollar} (no-deps resolve must error iff a $-string is present)"
                ));
            }
            if ok {
                let returned = &output["value"];
                if returned != value {
                    return Err("Ok path did not return the value unchanged".into());
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── extract_urls ──────────────────────────────────────────────────────────

/// Every URL `extract_urls` returns must start with `https://` and match the
/// media-URL predicate the implementation uses (fal.media CDN or a known media
/// extension). This is the output-validity invariant; it also pins no-panic.
const MEDIA_EXTENSIONS: &[&str] = &[
    "fal.media",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
    ".gif",
    ".mp4",
    ".mp3",
    ".svg",
    ".wav",
];

fn is_media_url(url: &str) -> bool {
    url.starts_with("https://")
        && (url.contains("fal.media") || MEDIA_EXTENSIONS.iter().any(|e| url.contains(e)))
}

proptest! {
    /// `extract_urls` never panics on arbitrary JSON and every URL it returns
    /// satisfies the media-URL predicate. A value with no matching strings
    /// must yield an empty vec.
    #[test]
    fn prop_extract_urls_output_valid(value in arb_json_value()) {
        let urls = extract_urls(&value);
        let output = serde_json::json!({ "urls": urls });
        let input = serde_json::json!({ "value_len": value.to_string().len() });

        let oracle = oracle_invariant(|_input, output| {
            let urls = output["urls"].as_array().ok_or("urls not array")?;
            for u in urls {
                let s = u.as_str().ok_or("url not str")?;
                if !is_media_url(s) {
                    return Err(format!("extracted url {s:?} fails the media-URL predicate"));
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── topological_sort ──────────────────────────────────────────────────────

/// Encode a node graph as JSON for the oracle: each node is `{id, depends}`.
fn graph_to_json(nodes: &[GraphNode]) -> Value {
    let arr: Vec<Value> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id(),
                "depends": n.depends(),
            })
        })
        .collect();
    Value::Array(arr)
}

proptest! {
    /// For a graph built only with forward edges (node `j` may depend on any
    /// subset of `0..j`), `topological_sort` must return `Ok` with all node
    /// ids in an order where every dependency precedes its dependent. This is
    /// the topological-order invariant, checked independently of the impl.
    #[test]
    fn prop_topological_sort_dag(
        count in 1u8..=7u8,
        deps_bits in prop::collection::vec(any::<bool>(), 0..64),
    ) {
        let count = count as usize;
        let mut nodes: Vec<GraphNode> = Vec::with_capacity(count);
        for j in 0..count {
            let mut depends = Vec::new();
            for i in 0..j {
                let bit = deps_bits.get(i * count + j).copied().unwrap_or(false);
                if bit {
                    depends.push(format!("n{i}"));
                }
            }
            nodes.push(GraphNode::Source {
                id: format!("n{j}"),
                depends,
                value: Value::Null,
            });
        }

        let outcome = topological_sort_graph(&nodes);
        let output = match &outcome {
            Ok(order) => serde_json::json!({ "ok": true, "order": order }),
            Err(_) => serde_json::json!({ "ok": false, "order": serde_json::Value::Array(vec![]) }),
        };
        let input = serde_json::json!({ "graph": graph_to_json(&nodes) });

        let oracle = oracle_invariant(|input, output| {
            let graph = input["graph"].as_array().ok_or("graph not array")?;
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            if !ok {
                return Err("acyclic forward-edge graph was rejected".into());
            }
            let order = output["order"].as_array().ok_or("order not array")?;
            if order.len() != graph.len() {
                return Err(format!("order len {} != node count {}", order.len(), graph.len()));
            }
            let position: std::collections::HashMap<String, usize> = order
                .iter()
                .enumerate()
                .filter_map(|(idx, v)| v.as_str().map(|s| (s.to_string(), idx)))
                .collect();
            for node in graph {
                let id = node["id"].as_str().ok_or("id not str")?.to_string();
                let pos = position.get(&id).ok_or_else(|| format!("node {id} missing from order"))?;
                for dep in node["depends"].as_array().unwrap_or(&Vec::new()) {
                    let dep = dep.as_str().ok_or("dep not str")?.to_string();
                    let dep_pos = position.get(&dep).ok_or_else(|| format!("dep {dep} missing from order"))?;
                    if dep_pos >= pos {
                        return Err(format!(
                            "dependency {dep} at {dep_pos} does not precede dependent {id} at {pos}"
                        ));
                    }
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }

    /// A pure cycle (`n{i}` depends on `n{(i+1) % len}`) must be rejected with
    /// `Err`. The oracle pins that no ordering is ever produced for a cycle.
    #[test]
    fn prop_topological_sort_rejects_cycle(len in 2u8..=6u8) {
        let len = len as usize;
        let mut nodes: Vec<GraphNode> = Vec::with_capacity(len);
        for i in 0..len {
            let next = (i + 1) % len;
            nodes.push(GraphNode::Source {
                id: format!("n{i}"),
                depends: vec![format!("n{next}")],
                value: Value::Null,
            });
        }
        let outcome = topological_sort_graph(&nodes);
        let output = serde_json::json!({ "ok": outcome.is_ok() });
        let input = serde_json::json!({ "len": len });

        let oracle = oracle_invariant(|_input, output| {
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            if ok {
                return Err("a cyclic graph was accepted by topological_sort".into());
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }

    /// A node depending on an unknown id must be rejected (no silent skip).
    #[test]
    fn prop_topological_sort_rejects_unknown_dependency(ghost in "[a-z][a-z0-9_]{0,4}") {
        let nodes = vec![GraphNode::Source {
            id: "n0".to_string(),
            depends: vec![format!("n{ghost}")],
            value: Value::Null,
        }];
        let outcome = topological_sort_graph(&nodes);
        let output = serde_json::json!({ "ok": outcome.is_ok() });
        let input = serde_json::json!({ "ghost": ghost });

        let oracle = oracle_invariant(|_input, output| {
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            if ok {
                return Err("unknown dependency was accepted by topological_sort".into());
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── validate_workflow_structure ───────────────────────────────────────────

/// Build a node of the given kind (0 = Input, 1 = Run, 2 = Display) with an
/// index-derived id and the supplied payload.
fn node_of_kind(kind: u8, index: usize, payload: Value) -> WorkflowNode {
    let id = format!("n{index}");
    match kind {
        0 => WorkflowNode::Input {
            id,
            depends: Vec::new(),
            input: payload,
        },
        1 => WorkflowNode::Run {
            id,
            depends: Vec::new(),
            app: "app".to_string(),
            input: payload,
            mode: ExecutionMode::Sync,
        },
        _ => WorkflowNode::Display {
            id,
            depends: Vec::new(),
            fields: payload,
        },
    }
}

proptest! {
    /// `validate_workflow_structure` returns `Ok` iff there is at least one
    /// Input, one Run, and one Display node. The oracle re-derives the expected
    /// verdict from the kind histogram.
    #[test]
    fn prop_validate_workflow_structure(
        kinds in prop::collection::vec(prop::sample::select(&[0u8, 1u8, 2u8]), 0..6),
        payloads in prop::collection::vec(arb_json_value(), 0..6),
    ) {
        let count = kinds.len().min(payloads.len());
        let mut has_input = false;
        let mut has_run = false;
        let mut has_display = false;
        let mut nodes = Vec::with_capacity(count);
        for i in 0..count {
            match kinds[i] {
                0 => has_input = true,
                1 => has_run = true,
                _ => has_display = true,
            }
            nodes.push(node_of_kind(kinds[i], i, payloads[i].clone()));
        }
        let outcome = validate_workflow_structure(&nodes);
        let output = serde_json::json!({ "ok": outcome.is_ok() });
        let input = serde_json::json!({ "has_input": has_input, "has_run": has_run, "has_display": has_display });

        let oracle = oracle_invariant(|input, output| {
            let has_input = input["has_input"].as_bool().ok_or("has_input not bool")?;
            let has_run = input["has_run"].as_bool().ok_or("has_run not bool")?;
            let has_display = input["has_display"].as_bool().ok_or("has_display not bool")?;
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            let expected = has_input && has_run && has_display;
            if ok != expected {
                return Err(format!("validate ok={ok} but expected {expected}"));
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── parse_workflow_nodes round-trip ───────────────────────────────────────

proptest! {
    /// A workflow object built by serializing constructed `WorkflowNode`s must
    /// round-trip through `parse_workflow_nodes`: same node count, same ids,
    /// and each parsed node re-serializes to the same JSON as the original.
    /// This pins the serde `#[serde(tag = "type")]` contract.
    #[test]
    fn prop_parse_workflow_nodes_roundtrip(
        kinds in prop::collection::vec(prop::sample::select(&[0u8, 1u8, 2u8]), 0..5),
        payloads in prop::collection::vec(arb_json_value(), 0..5),
    ) {
        let count = kinds.len().min(payloads.len());
        let mut original: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        for i in 0..count {
            let node = node_of_kind(kinds[i], i, payloads[i].clone());
            let val = serde_json::to_value(&node).expect("serialize WorkflowNode");
            original.insert(format!("n{i}"), val);
        }
        let workflow = Value::Object(original.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

        let outcome = parse_workflow_nodes(&workflow);
        let output = match &outcome {
            Ok(nodes) => {
                let mut reparsed: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
                for n in nodes {
                    reparsed.insert(n.id().to_string(), serde_json::to_value(n).expect("re-serialize"));
                }
                serde_json::json!({ "ok": true, "reparsed": reparsed })
            }
            Err(_) => serde_json::json!({ "ok": false, "reparsed": serde_json::Value::Object(serde_json::Map::new()) }),
        };
        let input = serde_json::json!({ "original": original });

        let oracle = oracle_invariant(|input, output| {
            let original = input["original"].as_object().ok_or("original not object")?;
            let ok = output["ok"].as_bool().ok_or("ok not bool")?;
            if !ok {
                return Err("parse_workflow_nodes rejected a serialized workflow".into());
            }
            let reparsed = output["reparsed"].as_object().ok_or("reparsed not object")?;
            if reparsed.len() != original.len() {
                return Err(format!("reparsed count {} != original {}", reparsed.len(), original.len()));
            }
            for (id, val) in original {
                let got = reparsed.get(id).ok_or_else(|| format!("node {id} missing from reparsed"))?;
                if got != val {
                    return Err(format!("node {id} did not round-trip: {got:#} != {val:#}"));
                }
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ── RouterModelEntry::infer_vision_support ───────────────────────────────

/// Representative default vision families (a subset of the source's
/// `DEFAULT_VISION_FAMILIES` list). Used only to assert the positive
/// direction; the negative direction is pinned by the "never Some(false)"
/// invariant, which is robust to env-var additions.
const REPRESENTATIVE_VISION_FAMILIES: &[&str] =
    &["llava", "qwen2-vl", "gemma3", "pixtral", "minicpm-v"];

proptest! {
    /// For any model name, `infer_vision_support` returns `None` or `Some(true)`
    /// — never `Some(false)`. This is a structural invariant of the impl (it
    /// only ever returns `Some(true)` on a match) and is robust to the
    /// `HKASK_VISION_FAMILIES` env var adding more positive matches.
    #[test]
    fn prop_infer_vision_never_returns_some_false(model in "[A-Za-z0-9_.\\-/]{0,30}") {
        let result = RouterModelEntry::infer_vision_support(&model, None);
        let output = serde_json::json!({ "result": result });
        let input = serde_json::json!({ "model": model });

        let oracle = oracle_invariant(|_input, output| {
            match output["result"].as_bool() {
                None => Ok(()),
                Some(true) => Ok(()),
                Some(false) => Err("infer_vision_support returned Some(false)".into()),
            }
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }

    /// A model name containing a default vision family substring must return
    /// `Some(true)`, regardless of the surrounding context (prefix/suffix).
    #[test]
    fn prop_infer_vision_detects_known_family(
        family in prop::sample::select(REPRESENTATIVE_VISION_FAMILIES),
        prefix in "[A-Za-z0-9_.\\-/]{0,8}",
        suffix in "[A-Za-z0-9_.\\-/]{0,8}",
    ) {
        let model = format!("{prefix}{family}{suffix}");
        let result = RouterModelEntry::infer_vision_support(&model, None);
        let output = serde_json::json!({ "result": result });
        let input = serde_json::json!({ "family": family, "model": model });

        let oracle = oracle_invariant(|input, output| {
            let family = input["family"].as_str().ok_or("family not str")?;
            let result = output["result"].as_bool();
            if result != Some(true) {
                return Err(format!(
                    "model containing family {family:?} returned {result:?}, expected Some(true)"
                ));
            }
            Ok(())
        });

        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}
