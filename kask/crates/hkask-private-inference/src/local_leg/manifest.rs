//! L1 — versioned engine-surface manifests: the drift layer.
//!
//! The placement compiler speaks intents; a manifest maps intents to the
//! concrete flag spellings of ONE pinned engine surface and carries the
//! constraint set plus the fixtures that gate its admission in CI. A
//! llama.cpp release that renames flags or changes constraint semantics is
//! absorbed by editing this data and its fixtures — zero Rust changes
//! (plan hypothesis H2). The manifest is never trusted alone at launch:
//! the supervisor's startup probe repairs the model against the installed
//! binary (Good Regulator discipline, plan §6).

use hkask_lisp::eval_sandboxed;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSurfaceManifest {
    /// Stable identifier of the engine surface this manifest pins.
    pub surface_id: String,
    /// The engine build this flag-spelling set was verified against. The
    /// supervisor's startup probe refuses builds older than this pin with
    /// a typed error naming both versions.
    pub engine_version_pin: String,
    pub flags: FlagVocabulary,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Fixtures gate the manifest's admission in CI: every constraint is
    /// evaluated against every fixture env; a mismatch refuses the
    /// manifest (plan §4, L2).
    #[serde(default)]
    pub fixtures: Vec<ConstraintFixture>,
}

/// Intent → concrete flag spelling(s) for one pinned surface. One intent
/// may spell as MULTIPLE flags when the engine splits it: v0.9.0-dev
/// (commit 9e2a04a) split the KV cache type into `--cache-type-k` and
/// `--cache-type-v`, both taking the same value — recorded 2026-11-04 as
/// the first live drift specimen the manifest absorbs (H2: spelling drift
/// lands here as data, never in the compiler).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagVocabulary {
    pub binary: String,
    pub model: String,
    pub n_gpu_layers: String,
    pub cpu_moe: String,
    /// Every spelling is emitted with the policy value; a future re-merged
    /// engine surface is a one-element list — data-only.
    pub kv_cache_type: Vec<String>,
    pub ctx_size: String,
    pub flash_attn: String,
    pub host: String,
    pub port: String,
    pub threads: String,
}

/// The closed vocabulary of constraint kinds. New kinds upstream semantics
/// demand arrive as typed data here first; the `expr` escape hatch carries
/// predicates the vocabulary cannot yet express, evaluated by the in-tree
/// lisp sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Constraint {
    /// Launch bytes that must fit AVAILABLE RAM with a free fraction of
    /// `headroom_factor`. File-backed mmap'd weights are evictable and
    /// cannot OOM the machine, so the guard targets the anonymous class
    /// (KV cache + compute buffers) — but when weights are NOT mmap'd they
    /// count as resident too. The fraction must stay free (H4: measures
    /// AVAILABLE, not total — co-resident services are already inside it).
    AvailableRamHeadroom {
        /// Fraction of available RAM that must remain free (0.0..=0.9).
        headroom_factor: f64,
    },
    /// When dense layers offload to the GPU, dense bytes + KV estimate
    /// must fit the DECLARED VRAM budget; an undeclared budget fails
    /// closed — never an unverified allowance.
    VramDenseFit,
    /// Streaming-mode (P2, unroutable in v1): the per-layer expert cache
    /// must hold at least `floor_mult` × `n_expert_used` dynamic slots —
    /// PR #25294's documented abort class. Ships now so the P2 surface is
    /// gated the day it activates. A plan that declares no slots is not a
    /// streaming plan (allow); one that declares them partially fails
    /// closed.
    ExpertSlotFloor { floor_mult: u32 },
    /// Two planned flags that must not co-occur.
    MutuallyExclusive { a: String, b: String },
    /// Reserved escape hatch: an open-form predicate evaluated by the
    /// in-tree `hkask-lisp` sandbox against the constraint env as JSON.
    /// The form must evaluate to a boolean.
    Expr { form: String },
}

/// The environment every constraint evaluates against — the same shape at
/// fixture time (CI) and at placement time (launch).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConstraintEnv {
    pub available_ram_bytes: u64,
    pub total_ram_bytes: u64,
    /// The VRAM budget figure the guards consume, however sourced —
    /// sysfs-detected at the probe layer or caller-declared (see
    /// placement::VramSource).
    pub vram_bytes_declared: Option<u64>,
    pub gpu_offload: bool,
    /// Whether the plan's model weights are file-backed (mmap). When
    /// false (e.g. `--mlock` planned) weights count as resident RAM.
    pub model_mmap: bool,
    pub expert_bytes: u64,
    pub dense_bytes: u64,
    pub kv_bytes_estimate: u64,
    pub compute_bytes_estimate: u64,
    pub context_length: u32,
    pub slot_count: Option<u32>,
    pub n_expert_used: Option<u32>,
    pub planned_flags: Vec<String>,
}

/// What a constraint actually produced. `Deny` carries the refusal reason
/// so the typed error names the violated guard and the numbers that
/// tripped it.
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluation {
    Allow,
    Deny(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintFixture {
    pub name: String,
    pub env: ConstraintEnv,
    /// The exact set of constraints that must deny on this env — sorted
    /// set equality against what the FULL constraint set produces: a
    /// missing denial AND an unexpected denial both refuse the manifest.
    pub expected_denials: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest decode failed: {0}")]
    Decode(String),
    #[error("manifest validation failed: {0}")]
    Validate(String),
    #[error("constraint `{constraint}`: {reason}")]
    Constraint { constraint: String, reason: String },
    #[error(
        "expr constraint `{constraint}` returned `{value}`; a predicate must evaluate to a boolean"
    )]
    ExprNotPredicate { constraint: String, value: String },
    #[error("expr constraint `{constraint}` failed to evaluate: {error}")]
    ExprEval { constraint: String, error: String },
}

/// Decode a manifest TOML document. One decode home — every consumer
/// (tests, the future supervisor, the MCP tools) routes through here;
/// reader-side inline decoding is a fork of the format.
pub fn decode(toml_text: &str) -> Result<EngineSurfaceManifest, ManifestError> {
    let manifest: EngineSurfaceManifest =
        toml::from_str(toml_text).map_err(|e| ManifestError::Decode(e.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

/// Stable name of a constraint, for typed refusal messages.
pub fn constraint_name(constraint: &Constraint) -> &'static str {
    match constraint {
        Constraint::AvailableRamHeadroom { .. } => "available_ram_headroom",
        Constraint::VramDenseFit => "vram_dense_fit",
        Constraint::ExpertSlotFloor { .. } => "expert_slot_floor",
        Constraint::MutuallyExclusive { .. } => "mutually_exclusive",
        Constraint::Expr { .. } => "expr",
    }
}

impl EngineSurfaceManifest {
    /// Parameter-range validation at decode time — closed vocabulary and
    /// sane factors, so a bad data edit fails at the gate, not at launch.
    pub fn validate(&self) -> Result<(), ManifestError> {
        for constraint in &self.constraints {
            match constraint {
                Constraint::AvailableRamHeadroom { headroom_factor }
                    if !(0.0..=0.9).contains(headroom_factor) =>
                {
                    return Err(ManifestError::Validate(format!(
                        "available_ram_headroom.headroom_factor {headroom_factor} outside 0.0..=0.9"
                    )));
                }
                Constraint::ExpertSlotFloor { floor_mult } if *floor_mult < 1 => {
                    return Err(ManifestError::Validate(format!(
                        "expert_slot_floor.floor_mult {floor_mult} must be >= 1"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Evaluate one constraint against an env. Deterministic; the arithmetic
/// lives here, the parameters live in the manifest data (plan §4).
pub fn evaluate_constraint(
    constraint: &Constraint,
    env: &ConstraintEnv,
) -> Result<Evaluation, ManifestError> {
    match constraint {
        Constraint::AvailableRamHeadroom { headroom_factor } => {
            let name = "available_ram_headroom";
            let anonymous = env
                .kv_bytes_estimate
                .checked_add(env.compute_bytes_estimate)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: name.into(),
                    reason: "anonymous byte sum overflowed u64".into(),
                })?;
            let resident = if env.model_mmap {
                anonymous
            } else {
                anonymous
                    .checked_add(env.expert_bytes)
                    .and_then(|b| b.checked_add(env.dense_bytes))
                    .ok_or_else(|| ManifestError::Constraint {
                        constraint: name.into(),
                        reason: "resident byte sum overflowed u64".into(),
                    })?
            };
            let budget = (env.available_ram_bytes as f64 * (1.0 - headroom_factor)) as u64;
            if resident <= budget {
                Ok(Evaluation::Allow)
            } else {
                Ok(Evaluation::Deny(format!(
                    "resident {resident} bytes exceeds budget {budget} \
                     (available {} × (1 − {headroom_factor}))",
                    env.available_ram_bytes
                )))
            }
        }
        Constraint::VramDenseFit => {
            if !env.gpu_offload {
                return Ok(Evaluation::Allow);
            }
            let Some(vram) = env.vram_bytes_declared else {
                return Ok(Evaluation::Deny(
                    "GPU offload planned but no VRAM budget known".into(),
                ));
            };
            let resident = env
                .dense_bytes
                .checked_add(env.kv_bytes_estimate)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: "vram_dense_fit".into(),
                    reason: "dense+kv byte sum overflowed u64".into(),
                })?;
            if resident <= vram {
                Ok(Evaluation::Allow)
            } else {
                Ok(Evaluation::Deny(format!(
                    "dense {dense} + kv {kv} bytes exceed the VRAM budget {vram}",
                    dense = env.dense_bytes,
                    kv = env.kv_bytes_estimate
                )))
            }
        }
        Constraint::ExpertSlotFloor { floor_mult } => match (env.slot_count, env.n_expert_used) {
            (None, None) => Ok(Evaluation::Allow),
            (Some(slots), Some(used)) => {
                let floor = floor_mult.saturating_mul(used);
                if slots >= floor {
                    Ok(Evaluation::Allow)
                } else {
                    Ok(Evaluation::Deny(format!(
                        "expert cache slots {slots} below floor {floor_mult}×{used} = {floor}"
                    )))
                }
            }
            _ => Ok(Evaluation::Deny(
                "streaming plan is half-declared: slot_count and n_expert_used must both be set"
                    .into(),
            )),
        },
        Constraint::MutuallyExclusive { a, b } => {
            let has_a = env.planned_flags.iter().any(|f| f == a);
            let has_b = env.planned_flags.iter().any(|f| f == b);
            if has_a && has_b {
                Ok(Evaluation::Deny(format!(
                    "flags `{a}` and `{b}` must not co-occur"
                )))
            } else {
                Ok(Evaluation::Allow)
            }
        }
        Constraint::Expr { form } => {
            let env_json = serde_json::to_value(env).map_err(|e| ManifestError::Constraint {
                constraint: "expr".into(),
                reason: format!("env serialization failed: {e}"),
            })?;
            match eval_sandboxed(form, &env_json) {
                Ok(serde_json::Value::Bool(true)) => Ok(Evaluation::Allow),
                Ok(serde_json::Value::Bool(false)) => {
                    Ok(Evaluation::Deny(format!("predicate false: {form}")))
                }
                Ok(other) => Err(ManifestError::ExprNotPredicate {
                    constraint: "expr".into(),
                    value: other.to_string(),
                }),
                Err(e) => Err(ManifestError::ExprEval {
                    constraint: "expr".into(),
                    error: e.to_string(),
                }),
            }
        }
    }
}

/// The manifest admission gate: every constraint against every fixture.
/// A fixture declares the outcome it expects; a mismatch is a typed
/// error naming the fixture and the constraint.
pub fn run_fixtures(manifest: &EngineSurfaceManifest) -> Result<(), ManifestError> {
    for fixture in &manifest.fixtures {
        let mut denials: Vec<String> = Vec::new();
        for constraint in &manifest.constraints {
            if let Evaluation::Deny(_) = evaluate_constraint(constraint, &fixture.env)? {
                denials.push(constraint_name(constraint).to_string());
            }
        }
        let mut expected = fixture.expected_denials.clone();
        denials.sort();
        expected.sort();
        if denials != expected {
            return Err(ManifestError::Constraint {
                constraint: fixture.name.clone(),
                reason: format!(
                    "expected denials {expected:?} but the constraint set produced {denials:?}"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_TOML: &str =
        include_str!("../../../../registry/private-inference/llama-cpp-mainline.toml");

    #[test]
    fn decodes_and_validates_the_pinned_surface() {
        let m = decode(MANIFEST_TOML).expect("pinned manifest decodes and validates");
        assert_eq!(m.flags.binary, "llama-server");
        assert!(!m.constraints.is_empty());
        assert!(!m.fixtures.is_empty());
    }

    #[test]
    fn every_fixture_gates_every_constraint() {
        let m = decode(MANIFEST_TOML).expect("manifest");
        run_fixtures(&m).expect("all fixtures match their expected outcomes");
    }

    #[test]
    fn unknown_constraint_kind_is_rejected() {
        let bad = r#"
            surface_id = "x"
            engine_version_pin = "y"
            [flags]
            binary = "b"
            model = "--model"
            n_gpu_layers = "-ngl"
            cpu_moe = "--cpu-moe"
            kv_cache_type = ["--cache-type-k", "--cache-type-v"]
            ctx_size = "--ctx-size"
            flash_attn = "--flash-attn"
            host = "--host"
            port = "--port"
            threads = "--threads"
            [[constraints]]
            kind = "vibe_based"
        "#;
        assert!(
            decode(bad).is_err(),
            "closed vocabulary: unknown kinds reject"
        );
    }

    #[test]
    fn out_of_range_headroom_is_rejected() {
        let mut m = decode(MANIFEST_TOML).expect("manifest");
        m.constraints[0] = Constraint::AvailableRamHeadroom {
            headroom_factor: 1.5,
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn expr_constraint_must_return_a_boolean() {
        let c = Constraint::Expr {
            form: "(+ 1 2)".into(),
        };
        let env = ConstraintEnv::default();
        let err = evaluate_constraint(&c, &env).expect_err("non-boolean expr must fail");
        assert!(matches!(err, ManifestError::ExprNotPredicate { .. }));
    }

    #[test]
    fn expr_constraint_evaluates_against_the_env() {
        let c = Constraint::Expr {
            form: "(<= context_length 262144)".into(),
        };
        let mut env = ConstraintEnv::default();
        env.context_length = 131072;
        assert_eq!(
            evaluate_constraint(&c, &env).expect("eval"),
            Evaluation::Allow
        );
        env.context_length = 300000;
        assert!(matches!(
            evaluate_constraint(&c, &env).expect("eval"),
            Evaluation::Deny(_)
        ));
    }
}
