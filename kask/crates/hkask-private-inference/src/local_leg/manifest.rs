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
    /// The engine build this flag-spelling set was verified against —
    /// display text. The startup probe enforces the structured
    /// [`VersionPin`] (exact-commit: a different commit is an unverified
    /// surface, newer included) and refuses with a typed error naming
    /// both commits.
    pub engine_version_pin: String,
    /// The structured pin the startup probe enforces (D-S2, slice plan
    /// §6: commit hashes are not orderable, so "older than" is not
    /// computable as stated — the honest floor is exact-commit).
    pub version_pin: VersionPin,
    /// The endpoints the supervisor health-polls and verifies (drift
    /// data: an upstream endpoint rename is a manifest edit, zero Rust).
    pub probe: ProbeExpectations,
    pub flags: FlagVocabulary,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Fixtures gate the manifest's admission in CI: every constraint is
    /// evaluated against every fixture env; a mismatch refuses the
    /// manifest (plan §4, L2).
    #[serde(default)]
    pub fixtures: Vec<ConstraintFixture>,
}

/// The structured engine version pin the startup probe enforces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionPin {
    /// The engine commit the flag surface was verified against.
    pub commit: String,
    /// The engine version string (display text).
    pub version: String,
}

/// The endpoints the supervisor probes after spawn — data, not Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeExpectations {
    /// The health endpoint the launch gate polls (bounded by the
    /// measured load bound until first healthy).
    pub health_path: String,
    /// The OpenAI-compatible models listing the supervisor verifies
    /// after first healthy.
    pub models_path: String,
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
    /// Per-tensor placement override — the pin mechanism for weights that
    /// must stay on CPU while layers offload (the PLE n-gram table).
    pub override_tensor: String,
    /// Every spelling is emitted with the policy value; a future re-merged
    /// engine surface is a one-element list — data-only.
    pub kv_cache_type: Vec<String>,
    pub ctx_size: String,
    pub flash_attn: String,
    pub host: String,
    pub port: String,
    pub threads: String,
    /// The served-model alias pin — the server serves the loaded model
    /// under the brief's `model_id`, matching the provider entry's card
    /// names (the identity seam of the M7 surface).
    pub alias: String,
    /// The token file flag — the endpoint token reaches the server from a
    /// 0600 file, never a command-line arg (`/proc/<pid>/cmdline` leak) and
    /// never an env var (the pinned build exposes none for the key).
    pub api_key_file: String,
    /// The speculative-decoding draft flags. None on surfaces that spell
    /// no draft intent — the mainline pin's binary is not on disk to
    /// verify the spellings against, and an unverified spelling is never
    /// claimed (the unsloth prebuilt surface declares its verified set).
    /// A draft ask against a vocabulary-less surface is a typed refusal
    /// at compile time — never a silently omitted draft.
    pub model_draft: Option<String>,
    pub spec_type: Option<String>,
    pub spec_draft_n_max: Option<String>,
    pub spec_draft_ngl: Option<String>,
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
    /// Dense bytes pinned to CPU by a tensor override
    /// (`--override-tensor`) — excluded from the VRAM offload budget (the
    /// 28.8 GiB PLE table).
    #[serde(default)]
    pub pinned_cpu_bytes: u64,
    pub kv_bytes_estimate: u64,
    pub compute_bytes_estimate: u64,
    pub context_length: u32,
    pub slot_count: Option<u32>,
    pub n_expert_used: Option<u32>,
    /// The draft module's byte size — resident weights under !mmap
    /// (exactly like the main model's), and inside the VRAM channel
    /// only when the draft is GPU-placed.
    #[serde(default)]
    pub draft_bytes: u64,
    /// Whether the plan places the draft module on the GPU (the
    /// compiler's routing decision — CPU `-ngld 0` when the VRAM
    /// arithmetic leaves no room).
    #[serde(default)]
    pub draft_on_gpu: bool,
    /// zed-kask: MTP record §7 item 1 (2026-11-06) — the spec-decode
    /// VERIFY workspace in bytes: GPU-side whenever a draft is ASKED,
    /// INDEPENDENT of the draft module's placement (a CPU-routed draft
    /// still runs speculative verification in GPU memory — the D-B
    /// finding: the verify path OOM'd ctx 65536 on the 8 GB card with
    /// 0.9-1.2 GB free, under-reserved by exactly this term). Zero = the
    /// plan asks no draft (or a pre-§7 brief) — the old arithmetic.
    #[serde(default)]
    pub verify_workspace_bytes: u64,
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

/// The shipped engine-surface manifest — embedded at compile time from the
/// registry (the fleet layer's precedent: registry data ships with the
/// binary; a missing file is a compile error, not a runtime hole). Adding
/// a second pinned surface is a second entry + its registry TOML.
pub const SHIPPED_MANIFEST_TOML: &str =
    include_str!("../../../../registry/private-inference/llama-cpp-mainline.toml");

/// Decode the shipped manifest — the installed binary's manifest home;
/// the decode path is the same one decode home for every consumer.
pub fn shipped_manifest() -> Result<EngineSurfaceManifest, ManifestError> {
    decode(SHIPPED_MANIFEST_TOML)
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
                    .and_then(|b| b.checked_add(env.draft_bytes))
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
            // Dense bytes pinned to CPU by a tensor override are excluded
            // from the offload budget — the PLE n-gram table must not make
            // an otherwise-feasible partial placement read as over-budget.
            let offloadable = env
                .dense_bytes
                .checked_sub(env.pinned_cpu_bytes)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: "vram_dense_fit".into(),
                    reason: format!(
                        "pinned_cpu_bytes {} exceeds dense_bytes {} — inconsistent placement env",
                        env.pinned_cpu_bytes, env.dense_bytes
                    ),
                })?;
            let mut resident = offloadable
                .checked_add(env.kv_bytes_estimate)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: "vram_dense_fit".into(),
                    reason: "dense+kv byte sum overflowed u64".into(),
                })?;
            // MTP record §7 item 1 (2026-11-06): the spec-decode VERIFY
            // workspace is GPU-side whenever a draft is ASKED, independent
            // of the draft module's placement — a CPU-routed draft still
            // verifies in GPU memory (the D-B finding). Zero when the plan
            // asks no draft: the pre-§7 arithmetic.
            resident = resident
                .checked_add(env.verify_workspace_bytes)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: "vram_dense_fit".into(),
                    reason: "dense+kv+verify-workspace byte sum overflowed u64".into(),
                })?;
            // A GPU-placed draft rides the VRAM budget; a CPU-routed one
            // (the compiler's -ngld 0) stays outside the channel.
            if env.draft_on_gpu {
                resident = resident.checked_add(env.draft_bytes).ok_or_else(|| {
                    ManifestError::Constraint {
                        constraint: "vram_dense_fit".into(),
                        reason: "dense+kv+draft byte sum overflowed u64".into(),
                    }
                })?;
            }
            if resident <= vram {
                Ok(Evaluation::Allow)
            } else {
                let draft_part = if env.draft_on_gpu {
                    format!(" + draft {}", env.draft_bytes)
                } else {
                    String::new()
                };
                let workspace_part = if env.verify_workspace_bytes > 0 {
                    format!(" + verify workspace {}", env.verify_workspace_bytes)
                } else {
                    String::new()
                };
                Ok(Evaluation::Deny(format!(
                    "offloadable dense {offloadable} (dense {dense} − pinned {pinned}) + kv {kv}{workspace_part}{draft_part} bytes exceed the declared VRAM budget {vram}",
                    dense = env.dense_bytes,
                    pinned = env.pinned_cpu_bytes,
                    kv = env.kv_bytes_estimate
                )))
            }
        }
        Constraint::ExpertSlotFloor { floor_mult } => {
            // Streaming is declared by slot_count alone; n_expert_used is
            // model metadata present for every MoE — a non-streaming plan
            // must not read as half-declared merely because the model
            // reports its per-token expert count (the first live matrix
            // run was refused on exactly that misreading, 2026-11-04).
            match (env.slot_count, env.n_expert_used) {
                    (None, _) => Ok(Evaluation::Allow),
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
                    (Some(_), None) => Ok(Evaluation::Deny(
                        "streaming plan declared (slot_count set) without n_expert_used — the floor cannot be checked"
                            .into(),
                    )),
                }
        }
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

    const UNSLOTH_PREBUILT_TOML: &str =
        include_str!("../../../../registry/private-inference/llama-cpp-unsloth-prebuilt.toml");

    const SERVEURPERSO_STREAM_TOML: &str = include_str!(
        "../../../../registry/private-inference/llama-cpp-serveurperso-moe-stream.toml"
    );

    #[test]
    fn the_serveurperso_stream_surface_decodes_and_its_fixtures_gate_it() {
        // The P2 streaming experiment engine's surface (the pinned fork,
        // local-leg plan §12 item 2, the P2 streaming-parity plan §8):
        // decode + validate + every-fixture-gates, the exact-commit pin
        // (the binary's own --version spelling), and the streaming
        // constraint class this surface exists to guard. The streaming
        // spellings do NOT ride [flags] — FlagVocabulary has no streaming
        // intent (streaming is unroutable in v1, pinned in placement) —
        // the fork's --help-verified streaming vocabulary lives in the
        // TOML's provenance header until the band gates a routing intent.
        let manifest =
            decode(SERVEURPERSO_STREAM_TOML).expect("serveurperso stream manifest decodes");
        manifest
            .validate()
            .expect("serveurperso stream manifest validates");
        run_fixtures(&manifest).expect("every fixture gates every constraint");
        assert_eq!(manifest.version_pin.commit, "7174388");
        assert_eq!(manifest.version_pin.version, "0.3.0-dev");
        assert_eq!(manifest.surface_id, "llama-cpp-serveurperso-moe-stream-p2");
        assert!(
            manifest
                .constraints
                .iter()
                .any(|c| matches!(c, Constraint::ExpertSlotFloor { .. }))
        );
        assert!(
            manifest
                .constraints
                .iter()
                .any(|c| matches!(c, Constraint::MutuallyExclusive { .. }))
        );
        // The draft vocabulary is verified on this fork (the P2 legs run
        // without the draft — the vocabulary is recorded, not consumed).
        assert_eq!(
            manifest.flags.model_draft.as_deref(),
            Some("--spec-draft-model")
        );
    }

    #[test]
    fn the_unsloth_prebuilt_surface_decodes_and_its_fixtures_gate_it() {
        let manifest = decode(UNSLOTH_PREBUILT_TOML).expect("prebuilt manifest decodes");
        manifest.validate().expect("prebuilt manifest validates");
        run_fixtures(&manifest).expect("every fixture gates every constraint");
        assert_eq!(manifest.version_pin.commit, "a3c12db9");
        assert_eq!(
            manifest.flags.model_draft.as_deref(),
            Some("--spec-draft-model")
        );
        assert_eq!(manifest.flags.spec_type.as_deref(), Some("--spec-type"));
        assert_eq!(
            manifest.flags.spec_draft_n_max.as_deref(),
            Some("--spec-draft-n-max")
        );
        assert_eq!(
            manifest.flags.spec_draft_ngl.as_deref(),
            Some("--spec-draft-ngl")
        );
    }

    #[test]
    fn the_mainline_surface_spells_no_draft_vocabulary() {
        // The honest absence: the mainline pin's binary is not on disk to
        // verify draft spellings against, so the surface claims none — a
        // draft ask against it is a typed refusal at compile_placement.
        let manifest = decode(MANIFEST_TOML).expect("mainline manifest decodes");
        assert!(manifest.flags.model_draft.is_none());
        assert!(manifest.flags.spec_type.is_none());
        assert!(manifest.flags.spec_draft_n_max.is_none());
        assert!(manifest.flags.spec_draft_ngl.is_none());
    }

    #[test]
    fn the_verify_workspace_counts_even_with_a_cpu_routed_draft() {
        // MTP record §7 item 1 (the D-B finding): the spec-decode verify
        // workspace is GPU-side whenever a draft is ASKED, independent of
        // the draft module's placement — a CPU-routed draft still verifies
        // in GPU memory. The 8 GB card: offloadable 6.0 + kv 0.5 + the 2 GiB
        // workspace = 8.647 > 8 — DENY, the reason naming the workspace
        // term; the same plan without the workspace fits (the pre-§7
        // arithmetic).
        let env = ConstraintEnv {
            available_ram_bytes: 0,
            total_ram_bytes: 0,
            vram_bytes_declared: Some(8_000_000_000),
            gpu_offload: true,
            model_mmap: true,
            expert_bytes: 0,
            dense_bytes: 6_000_000_000,
            pinned_cpu_bytes: 0,
            kv_bytes_estimate: 500_000_000,
            compute_bytes_estimate: 0,
            context_length: 0,
            slot_count: None,
            n_expert_used: None,
            draft_bytes: 2_786_568_256,
            draft_on_gpu: false,
            verify_workspace_bytes: 2_147_483_648,
            planned_flags: Vec::new(),
        };
        let evaluation =
            evaluate_constraint(&Constraint::VramDenseFit, &env).expect("the guard evaluates");
        assert!(
            matches!(&evaluation, Evaluation::Deny(reason) if reason.contains("verify workspace")),
            "the workspace counts even with a CPU-routed draft, and the deny names it — got: {evaluation:?}"
        );

        // The same plan without the workspace reserve fits (the pre-§7
        // arithmetic) — the field's zero is the legacy behavior.
        let legacy = ConstraintEnv {
            verify_workspace_bytes: 0,
            ..env
        };
        assert_eq!(
            evaluate_constraint(&Constraint::VramDenseFit, &legacy).expect("the guard evaluates"),
            Evaluation::Allow,
            "a zero workspace is the pre-§7 arithmetic — the plan fits"
        );
    }

    #[test]
    fn byte_sum_overflow_is_a_typed_error_not_a_wrap() {
        // The overflow invariant (the T5 not-formalizable class): every
        // guard sum is checked — a u64 overflow refuses the constraint
        // evaluation with a typed error, never a wrapped sum that could
        // read as under-budget.
        let constraint = Constraint::AvailableRamHeadroom {
            headroom_factor: 0.15,
        };
        let env = ConstraintEnv {
            kv_bytes_estimate: u64::MAX,
            compute_bytes_estimate: 1,
            ..ConstraintEnv::default()
        };
        let err = evaluate_constraint(&constraint, &env)
            .expect_err("the anonymous byte sum must refuse on overflow");
        assert!(
            matches!(err, ManifestError::Constraint { ref constraint, .. } if constraint == "available_ram_headroom"),
            "overflow must surface as the typed constraint error, got: {err}"
        );
    }

    const MANIFEST_TOML: &str =
        include_str!("../../../../registry/private-inference/llama-cpp-mainline.toml");

    #[test]
    fn decodes_and_validates_the_pinned_surface() {
        let m = decode(MANIFEST_TOML).expect("pinned manifest decodes and validates");
        assert_eq!(m.flags.binary, "llama-server");
        assert!(!m.constraints.is_empty());
        assert!(!m.fixtures.is_empty());
        // The structured pin and probe expectations are required (the
        // fail-closed admission: no manifest ships without them).
        assert_eq!(m.version_pin.commit, "9e2a04a");
        assert_eq!(m.probe.health_path, "/health");
        assert_eq!(m.probe.models_path, "/v1/models");
        assert_eq!(m.flags.alias, "--alias");
        assert_eq!(m.flags.api_key_file, "--api-key-file");
    }

    #[test]
    fn shipped_manifest_home_decodes_the_same_bytes_as_the_registry() {
        // The installed-binary home embeds the registry file verbatim and
        // routes through the one decode home — including the fixture gate.
        let m = shipped_manifest().expect("shipped manifest decodes");
        run_fixtures(&m).expect("shipped manifest fixtures gate");
        assert_eq!(m.surface_id, "llama-cpp-mainline-cpu-p0");
    }

    #[test]
    fn every_fixture_gates_every_constraint() {
        let m = decode(MANIFEST_TOML).expect("manifest");
        run_fixtures(&m).expect("all fixtures match their expected outcomes");
    }

    #[test]
    fn unknown_constraint_kind_is_rejected() {
        // Every required field is present so the ONLY decode error is the
        // unknown constraint kind — the pin isolates its intent (a
        // missing-field failure would pass the assert without testing the
        // closed vocabulary).
        let bad = r#"
            surface_id = "x"
            engine_version_pin = "y"
            [version_pin]
            commit = "c0"
            version = "v0"
            [probe]
            health_path = "/health"
            models_path = "/v1/models"
            [flags]
            binary = "b"
            model = "--model"
            n_gpu_layers = "-ngl"
            cpu_moe = "--cpu-moe"
            override_tensor = "--override-tensor"
            kv_cache_type = ["--cache-type-k", "--cache-type-v"]
            ctx_size = "--ctx-size"
            flash_attn = "--flash-attn"
            host = "--host"
            port = "--port"
            threads = "--threads"
            alias = "--alias"
            api_key_file = "--api-key-file"
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
