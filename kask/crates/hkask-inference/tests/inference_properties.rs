//! Property tests for the pure functions of the `hkask-inference` crate.
//!
//! These tests replaced the deleted `mod tests` blocks that used `MockProvider`
//! and `MockExecutor` stubs. They target *pure* functions only — `MediaOp`
//! parsing, `ProviderScore::weighted`, and
//! `RouterModelEntry::infer_vision_support` — using proptest generators and the
//! `hkask-test-harness` Oracle taxonomy (invariant + reference oracles).
//!
//! No stub, fake, mock, or noop providers/executors are constructed.

#![forbid(unsafe_code)]

use hkask_inference::scoring::{ProviderScore, ScoreWeights};
use hkask_inference::{MediaOp, RouterModelEntry};
use proptest::prelude::*;
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
    "generate_speech",
    "transcribe",
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
