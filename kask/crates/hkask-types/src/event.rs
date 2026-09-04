//! regulation record types — Cross-cutting infrastructure
//!
//! regulation records are the cybernetic audit trail emitted by all loops.
//! They are not owned by any single loop — they are the shared
//! observability substrate that the Regulation (Loop 6) senses and the
//! Curator (Loop 5) audits.

use crate::id::{EventID, WebID};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

/// regulation record — Cybernetic observation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulationRecord {
    pub id: EventID,
    pub timestamp: DateTime<Utc>,
    pub observer_webid: WebID,
    pub span: Span,
    pub phase: CyclePhase,
    pub observation: Value,
    pub regulation: Option<Value>,
    pub outcome: Option<Value>,
    pub recursion_depth: u8,
    pub parent_event: Option<EventID>,
    pub visibility: String,
}

impl RegulationRecord {
    /// Create a new RegulationRecord.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  observer is valid, span is valid, phase is valid
    /// post: returns RegulationRecord
    pub fn new(
        observer_webid: WebID,
        span: Span,
        phase: CyclePhase,
        observation: Value,
        recursion_depth: u8,
    ) -> Self {
        Self {
            id: EventID::new(),
            timestamp: Utc::now(),
            observer_webid,
            span,
            phase,
            observation,
            regulation: None,
            outcome: None,
            recursion_depth,
            parent_event: None,
            visibility: "private".to_string(),
        }
    }
}

/// Validated Regulation span namespace.
///
/// Constructed via `SpanNamespace::new()` which validates against
/// the canonical set. The module path IS the loop assignment.
/// Cannot be forged — construction requires a valid namespace string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanNamespace(String);

/// Canonical Regulation span namespaces — mirrors `RegulationSpan::as_str()` output plus namespaces
/// used by `SpanKind` (e.g. `reg.variety`).
/// Canonical Regulation span namespaces — all valid namespace strings for span construction.
///
/// This is the single source of truth for what Regulation spans exist. All domain span
/// strings must be registered here. `SpanNamespace::new()` and `::parse()` validate
/// against this set. Domain span enums construct `SpanNamespace` through `from_observable()`
/// which also validates against this set.
const CANONICAL_NAMESPACES: &[&str] = &[
    // ── ACP (Agent Communication Protocol) ──
    "reg.acp.ide.connection_state",
    "reg.acp.agent.memory_size",
    // ── Core infrastructure ──
    "reg.adapter",
    "reg.pod",
    "reg.alert",
    // ── Seam architecture ──
    // reg.architecture.seam.* removed 2026-08-30 — the "seam watcher"
    // that would have emitted them was never built; zero emitters.
    // ── Core infrastructure ──
    "reg.backup",
    "reg.backup.variety",
    // ── Authorization ──
    "reg.authorization",
    // ── API metering ──
    "reg.api.request",
    // ── Chat / Communication ──
    "reg.chat",
    "reg.chat.condense",
    // ── CI / QA ──
    "reg.ci.invariant.violation",
    // ── Classification ──
    "reg.classify.drift",
    "reg.classify.dual_fidelity",
    // ── Chat / Communication ──
    "reg.communication.agent",
    "reg.communication.agent.deregistered",
    "reg.communication.agent.invited",
    "reg.communication.agent.registered",
    "reg.communication.listener",
    "reg.communication.listener.started",
    "reg.communication.listener.stopped",
    "reg.communication.message",
    "reg.communication.message.ignored",
    "reg.communication.message.observed",
    "reg.communication.thread",
    "reg.communication.thread.created",
    "reg.communication.thread.monitored",
    "reg.consent",
    "reg.consolidation",
    // ── Contracts ──
    "reg.contract.accepted",
    "reg.contract.coverage",
    "reg.contract.proposed",
    "reg.contract.quality.violated",
    "reg.contract.rejected",
    "reg.contract.violated",
    // ── Curation / Curator ──
    "reg.curation",
    "reg.curation.escalation",
    "reg.curation.escalation.critical",
    "reg.curation.matrix",
    "reg.curator",
    "reg.curator.consolidation",
    "reg.curator.directive",
    "reg.curator.efficiency.exceeded",
    "reg.curator.metacognition",
    // ── Cybernetics ──
    "reg.cybernetics",
    "reg.cybernetics.backpressure",
    "reg.cybernetics.substitution",
    // Grounding alert signals from the cybernetics loop violation-delta sensor.
    "reg.grounding",
    // ── Email (curator interaction — outbound reg.email.sent + inbound reg.email.received) ──
    "reg.email",
    "reg.email.sent",
    // ── Deploy / Sessions ──
    "reg.deploy.backup_auto_export",
    "reg.deploy.backup_export",
    "reg.deploy.backup_upload",
    "reg.deploy.session_close",
    "reg.deploy.session_open",
    // ── Goal ──
    "reg.goal",
    // ── Healing ──
    "reg.heal",
    "reg.heal.attempt",
    "reg.heal.code_change_proposed",
    "reg.heal.dotenv",
    "reg.heal.escalated",
    "reg.heal.file_created",
    "reg.heal.llm_assisted",
    "reg.heal.retry_loop",
    "reg.heal.set_env",
    "reg.heal.strategy",
    "reg.heal.unmatched",
    // ── Inference ──
    "reg.inference",
    // ── Kata / Skill / Keystore ──
    "reg.kata",
    "reg.keystore",
    // ── Ledger (governance/rollback failure signals — runtime-posture-monitor visible) ──
    "reg.ledger",
    // ── MCP ──
    "reg.mcp",
    "reg.mcp.cap",
    "reg.mcp.health",
    // ── MCP Media ──
    "reg.mcp.media.face",
    // ── Media / Memory ──
    "reg.media",
    "reg.media.select",
    "reg.memory",
    "reg.memory.budget",
    "reg.memory.decay",
    "reg.memory.encode",
    // ── Regulation sensors (loop sense inputs; e.g. memory health) ──
    "reg.sensor",
    "reg.sensor.memory",
    "reg.memory.health",
    // ── Multi-agent ──
    "reg.multi.invite.accepted",
    "reg.multi.invite.sent",
    "reg.multi.role.assigned",
    // ── Outcome ──
    "reg.outcome",
    // ── Platform metrics ──
    "reg.platform.metric",
    "reg.platform.metric.dora.change_fail_rate",
    "reg.platform.metric.dora.deploy_freq",
    "reg.platform.metric.dora.lead_time",
    "reg.platform.metric.dora.mttr",
    "reg.platform.metric.loyalty",
    "reg.platform.metric.space.activity",
    "reg.platform.metric.space.communication",
    "reg.platform.metric.space.efficiency",
    "reg.platform.metric.space.performance",
    "reg.platform.metric.space.satisfaction",
    // ── QA ──
    "reg.qa.repair_attempted",
    "reg.qa.repair_exhausted",
    "reg.qa.repair_verified",
    // QA routine pass — emitted by scripts/qa-mcp-servers.sh per (tool, category)
    "reg.qa.run",
    "reg.qa.run.pass",
    "reg.qa.run.fail",
    "reg.qa.run.skipped",
    // ── Regulation (v0.31.0 Fermi impact-gate) ──
    "reg.outcome",
    "reg.outcome.calibration",
    "reg.outcome.coherence",
    "reg.outcome.predictive",
    // ── Agent ──
    "reg.agent.registered",
    // ── Semantic ──
    "reg.semantic.published",
    // ── Skill (organized by subdomain) ──
    // Lifecycle: skill discovery, loading, publishing
    "reg.skill.lifecycle",
    "reg.skill.lifecycle.skill_activated",
    "reg.skill.lifecycle.skills_loaded",
    "reg.skill.lifecycle.skills_discovered",
    "reg.skill.lifecycle.skill_published",
    // Registry: manifest validation
    "reg.skill.registry",
    "reg.skill.registry.registry_validated",
    // Cascade: step execution
    "reg.skill.cascade",
    "reg.skill.cascade.step_executed",
    "reg.skill.cascade.compute",
    "reg.skill.cascade.escalated",
    "reg.skill.cascade.branching_misconfigured",
    "reg.skill.cascade.choice_misconfigured",
    "reg.skill.cascade.timeout_retry",
    "reg.skill.cascade.parse_failure_retry",
    "reg.skill.cascade.skill_use_issue_report_failed",
    "reg.skill.cascade.step_failed",
    "reg.skill.cascade.gate_passed",
    "reg.skill.cascade.gate_failed",
    "reg.skill.cascade.parallel_joined",
    "reg.skill.cascade.tool_batch_joined",
    // Convergence: cascade outcomes
    "reg.skill.convergence",
    "reg.skill.convergence.converged",
    "reg.skill.convergence.escalated", // Provenance + profile enforcement
    "reg.skill.provenance",
    "reg.skill.profile_enforcement",
    // Frontmatter: SKILL.md parse errors (F-02 fix)
    "reg.skill.frontmatter",
    "reg.skill.frontmatter.missing",
    // Manifest: registry manifest errors (F-03 fix)
    "reg.skill.manifest",
    "reg.skill.manifest.unparsable",
    "reg.skill.manifest.absent",
    "reg.skill.manifest.unreadable",
    // Routing: skill-to-task matching (skill-router)
    "reg.skill.routing",
    "reg.skill.routing.matched",
    "reg.skill.routing.uncovered",
    // Discovery: capability gap detection and candidate evaluation (skill-discovery)
    "reg.skill.discovery",
    "reg.skill.discovery.gap_detected",
    "reg.skill.discovery.searched",
    "reg.skill.discovery.evaluated",
    // Bundle: composition and persistence (skill bundler / post-run UI)
    "reg.skill.bundle_compose",
    "reg.skill.bundle_save",
    // ── SLO ──
    "reg.slo.evaluated",
    // ── Sovereignty ──
    "reg.sovereignty",
    "reg.sovereignty.consent_anomaly",
    "reg.sovereignty.consent_audited",
    "reg.sovereignty.governance_report",
    "reg.sovereignty.portability_failure",
    "reg.sovereignty.portability_verified",
    // ── Spec ──
    "reg.spec",
    "reg.spec.executor",
    // ── Storage ──
    "reg.storage",
    "reg.storage.corruption",
    // ── Tool subsystems ──
    "reg.tool",
    "reg.tool.communication",
    "reg.tool.companies",
    "reg.tool.corpus",
    "reg.tool.curator",
    "reg.tool.filesystem",
    "reg.tool.kanban",
    "reg.tool.media",
    "reg.tool.memory",
    "reg.tool.registry",
    "reg.tool.research",
    "reg.tool.training",
    "reg.tool.web_search",
    // ── Web research (per-provider outcome spans — cybernetic feedback for
    // provider selection. Emitted by ProviderPool::search_compound and
    // search_single_provider. Read by the curator's MetacognitionLoop and
    // reg_query to compute rolling success-rate/latency per provider.) ──
    "reg.web.provider",
    // ── Variety ──
    "reg.variety",
    // ── Wallet span names removed 2026-08-30 — residuals of the wallet
    // module deleted in 219c74b180; no emitter constructs them.
    // ── Well ──
    // (removed with the wallet economy — no emitter)
    // ── Pipeline (corpus) ──
    "reg.pipeline",
    "reg.pipeline.calibration",
    "reg.pipeline.decimation",
    "reg.pipeline.triage",
    "reg.pipeline.decimation.binarize",
    "reg.pipeline.ocr",
    "reg.pipeline.ocr.circuit_breaker",
    "reg.pipeline.ocr.collusion",
    "reg.pipeline.ocr.low_confidence",
    "reg.pipeline.ocr.rate_limit",
    "reg.pipeline.ocr.silent_failure",
    "reg.pipeline.ocr.trust_invert",
    // OCR health snapshot persistence — the file the regulation loop senses
    // (a write failure blinds the loop to OCR degradation).
    "reg.pipeline.ocr.health",
    // Zero-chunk surfacing: a source that produces no chunks after processing
    // is pipeline degradation, surfaced not skipped.
    "reg.pipeline.chunk",
    "reg.pipeline.pdf_extract",
    // ── Batch (corpus AIMD concurrency ramp — ratified spec, PM decision
    // 2026-09-03) ──
    "reg.batch.concurrency",
    // ── Supply chain (security audit — supply-chain-sentinel skill) ──
    "reg.supply_chain",
    "reg.supply_chain.select",
    "reg.supply_chain.probe",
    "reg.supply_chain.report",
    "reg.supply_chain.convergence",
    // ── Runtime posture (security audit — runtime-posture-monitor skill) ──
    "reg.runtime",
    "reg.runtime.select",
    "reg.runtime.classify",
    "reg.runtime.regulate",
    "reg.runtime.convergence",
    "reg.runtime.policy",
    // ── Attack taxonomy (folded into kali-audit as taxonomy_map phase) ──
    "reg.taxonomy",
    "reg.taxonomy.select",
    "reg.taxonomy.map",
    "reg.taxonomy.report",
    "reg.taxonomy.convergence",
    // ── LoRA training (training-config audit — lora-training skill) ──
    "reg.lora",
    "reg.lora.select",
    "reg.lora.audit",
    "reg.lora.report",
    "reg.lora.convergence",
    "reg.lora.runtime",
    // ── Bug hunt (exploratory testing audit — bug-hunt skill) ──
    "reg.bughunt",
    "reg.bughunt.charter",
    "reg.bughunt.probe",
    "reg.bughunt.oracle",
    "reg.bughunt.taxonomize",
    "reg.bughunt.report",
    "reg.bughunt.learn",
    // ── Code review (convergent review audit — code-review skill) ──
    "reg.codereview",
    "reg.codereview.scope",
    "reg.codereview.perspectives",
    "reg.codereview.adjudicate",
    "reg.codereview.report",
    "reg.codereview.implement",
    // ── EQM (forecast-rationale quality measurement — eqm skill) ──
    // PDCA cascade: select → score → aggregate → validate. Emits the
    // overconfidence_bias signal that feeds superforecasting's calibration
    // step (a real cybernetic loop, not performative telemetry).
    "reg.eqm",
    "reg.eqm.select",
    "reg.eqm.score",
    "reg.eqm.aggregate",
    "reg.eqm.validate",
    // ── EQM improvement (rationale improvement Kata — eqm-improvement skill) ──
    // Improvement Kata PDCA: direction → current → target → predict → experiment.
    // Iterates to convergence; preserves the forecast probability (alignment
    // invariant). Sibling to metacognition's Kata loop.
    "reg.eqm_imp",
    "reg.eqm_imp.direction",
    "reg.eqm_imp.current",
    "reg.eqm_imp.target",
    "reg.eqm_imp.predict",
    "reg.eqm_imp.experiment",
    // ── Skill (unified cybernetic feedback — one namespace per skill) ──
    // Every skill emits reg.skill.<skill-id>.<phase> for its six PDCA phases.
    // The hierarchical is_canonical function makes reg.skill.<any-id>.* valid
    // without per-skill registration.
    "reg.skill",
    // ── Template ──
    "reg.template",
    // ── Training providers (provider HTTP call observability — post-mortem 2026-07-19) ──
    "reg.training.provider",
    "reg.training.provider.runpod.cancel",
    "reg.training.provider.runpod.drain",
    "reg.training.provider.runpod.graphql",
    "reg.training.provider.runpod.provision",
    "reg.training.provider.runpod.status",
    "reg.training.provider.runpod.submit",
    "reg.training.provider.runpod.teardown",
    "reg.training.provider.runpod.upload",
    // ── Training checkpoint (pod restart → Axolotl auto-resume) ──
    "reg.training.checkpoint.resume",
    // ── Widget (viz widget render + compose-back telemetry — D18/D21 seams) ──
    // Hierarchical: reg.widget.* children (render, disagree, graph_render,
    // evidence_set, whatif_discarded, reask) are valid via ancestor match.
    "reg.widget",
];

/// Hierarchical namespace validation — a sub-namespace like
/// `reg.pipeline.decimation.binarize` is valid if any prefix
/// segment (including the full string) is registered.
fn is_canonical(namespace: &str) -> bool {
    // MIRRORED in scripts/check-reg-canonical.sh::is_canonical — update both together.
    if CANONICAL_NAMESPACES.contains(&namespace) {
        return true;
    }
    if let Some(last_dot) = namespace.rfind('.') {
        is_canonical(&namespace[..last_dot])
    } else {
        false
    }
}

impl SpanNamespace {
    /// Create a validated span namespace. Returns None if the namespace is
    /// not canonical (not registered in CANONICAL_NAMESPACES or a descendant).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  namespace is non-empty
    /// post: returns Some(SpanNamespace) if valid, None otherwise
    pub fn new(namespace: &str) -> Option<Self> {
        if is_canonical(namespace) {
            Some(Self(namespace.to_string()))
        } else {
            None
        }
    }

    /// Fallible construction — returns Err for invalid namespaces.
    /// Accepts both short ("tool") and full ("reg.tool") forms.
    ///
    /// Implements `FromStr` so that `"variety".parse::<SpanNamespace>()` works.
    /// Parse a SpanNamespace from string.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns Some(SpanNamespace) if valid, None otherwise
    pub fn parse(s: &str) -> Option<Self> {
        let full = if s.starts_with("reg.") {
            s.to_string()
        } else {
            format!("reg.{s}")
        };
        if is_canonical(&full) {
            Some(Self(full))
        } else {
            None
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid SpanNamespace (canonical)
    /// post: returns the full namespace string (e.g. "reg.tool")
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid SpanNamespace (starts with "reg.")
    /// post: returns the short name after the "reg." prefix (e.g. "tool"),
    ///       or the full namespace if it doesn't start with "reg."
    pub fn short_name(&self) -> &str {
        if let Some(rest) = self.0.strip_prefix("reg.") {
            rest
        } else {
            &self.0
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid SpanNamespace (canonical)
    /// post: returns the SpanCategory for this namespace; unknown prefixes return SpanCategory::Unknown
    ///
    /// F-SYN-009: classify this namespace into a `SpanCategory` for
    /// typed dispatch (e.g. by `DecayConfig::lambda_for`).
    ///
    /// Hierarchical matches by `short_name()` prefix are preserved
    /// (e.g. `reg.variety.sensor` → `Variety`). Unknown namespaces
    /// return `SpanCategory::Unknown` so the caller can decide the
    /// fallback policy explicitly (the historical behaviour was
    /// `cybernetics_lambda`).
    pub fn category(&self) -> SpanCategory {
        SpanCategory::from_short_name(self.short_name())
    }
}

/// F-SYN-009: typed dispatch key for span-category-dependent logic
/// (e.g. `DecayConfig::lambda_for`).
///
/// Replaces the previous `&str` dispatch with a closed enum, while
/// preserving the hierarchical `.starts_with` matches that the old
/// string-based dispatch used. An `Unknown` variant makes the
/// fallback policy explicit at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanCategory {
    /// `reg.variety*` — the cybernetics loop.
    Cybernetics,
    /// `reg.curation*`, `reg.spec*` — the curation loop.
    Curation,
    /// `reg.inference*` — the inference loop.
    Inference,
    /// `reg.pod*`, `reg.connector*` — agent pod / connector operations (Memory loop).
    Memory,
    // Wallet variant removed 2026-08-30 — residual of the wallet module
    // deleted in 219c74b180; no span emitter constructed `reg.wallet*`
    // namespaces anymore.
    /// `reg.skill*` — per-skill cybernetic feedback (variety, convergence, rJoule, outcome).
    Skill,
    /// Any other namespace. Callers decide the fallback policy.
    Unknown,
}

impl SpanCategory {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  s is a short_name() string (e.g. "variety", "variety.sensor")
    /// post: returns the matching SpanCategory; unrecognised prefixes return SpanCategory::Unknown
    pub fn from_short_name(s: &str) -> Self {
        let prefix = s.split('.').next().unwrap_or(s);
        match prefix {
            "variety" | "outcome" | "alert" => Self::Cybernetics,
            "curation" | "spec" => Self::Curation,
            "inference" => Self::Inference,
            "pod" | "connector" => Self::Memory,
            "skill" => Self::Skill,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for SpanCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SpanCategory::Cybernetics => "cybernetics",
            SpanCategory::Curation => "curation",
            SpanCategory::Inference => "inference",
            SpanCategory::Memory => "memory",
            SpanCategory::Skill => "skill",
            SpanCategory::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

impl FromStr for SpanNamespace {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}

impl std::fmt::Display for SpanNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── RegulationSpan ↔ SpanNamespace bridges ──────────────────────────────────────

impl SpanNamespace {
    /// Shared validation path — both `From<RegulationSpan>` and `from_observable()` route through here.
    fn from_str_validated(s: &str) -> Option<Self> {
        Self::new(s)
    }
}

impl TryFrom<crate::regulation::RegulationSpan> for SpanNamespace {
    type Error = &'static str;

    /// Convert a typed `RegulationSpan` to a `SpanNamespace`, validating against
    /// the canonical namespace registry.
    ///
    /// Returns `Err` if the span's namespace string is not in the canonical
    /// registry. This should not happen if `RegulationSpan::as_str()` is correct;
    /// if it does, the `RegulationSpan` variant needs to be added to
    /// `CANONICAL_NAMESPACES`.
    fn try_from(span: crate::regulation::RegulationSpan) -> Result<Self, Self::Error> {
        Self::from_str_validated(span.as_str())
            .ok_or("RegulationSpan namespace not registered in CANONICAL_NAMESPACES")
    }
}

/// Unified Regulation span — namespace + fully-qualified path
///
/// Constructed via `Span::new()` with a validated namespace.
/// The namespace is validated at construction time by `SpanNamespace`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// The validated namespace (e.g., SpanNamespace::new("reg.tool"))
    pub namespace: SpanNamespace,
    /// Fully-qualified span path (e.g., "reg.tool.invoked")
    pub path: String,
}

impl Span {
    /// Create a new span with validated namespace.
    ///
    /// Example: `Span::new(SpanNamespace::new("reg.tool"), "invoked")`
    /// Create a new Span.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  namespace is valid, path is non-empty
    /// post: returns Span
    pub fn new(namespace: SpanNamespace, path: &str) -> Self {
        let full_path = format!("{}.{}", namespace.as_str(), path);
        Self {
            namespace,
            path: full_path,
        }
    }

    /// Create a span from a typed `SpanKind` variant.
    ///
    /// Eliminates string typos at construction sites for the most common
    /// span paths. Each variant maps to a canonical (namespace, path) pair.
    /// Create a Span from a SpanKind.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  kind is valid
    /// post: returns Span with canonical namespace and path
    pub fn from_kind(kind: SpanKind) -> Self {
        let (ns, local_path) = kind.namespace_and_path();
        Span::new(
            SpanNamespace::new(ns).expect("canonical namespace"),
            local_path,
        )
    }
}

/// Typed span kind — canonical (namespace, path) pairs for common spans.
///
/// Use `Span::from_kind()` to construct spans without string literals,
/// reducing the risk of typos in span paths at construction sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpanKind {
    // ── Tool spans (reg.tool.*) ──
    /// Tool invocation completed: `reg.tool.completed`
    ToolCompleted,

    // ── Curation spans (reg.curation.*) ──
    /// Curation directive acknowledged: `reg.curation.directive_acknowledged`
    CurationDirectiveAcknowledged,

    // ── Variety spans (reg.variety.*) ──
    /// Algedonic alert emitted: `reg.variety.algedonic_alert`
    VarietyAlgedonicAlert,

    // ── Regulation spans (reg.regulation.*) — v0.31.0 Fermi impact-gate ──
    /// Impact verification completed: `reg.regulation.impact_verified`
    ImpactVerified,
    /// Action substituted due to repeated ineffectiveness: `reg.regulation.action_substituted`
    ActionSubstituted,
    /// Action blocked due to severe counterproductivity: `reg.regulation.action_blocked`
    ActionBlocked,
    /// Regulatory plateau detected — escalation triggered: `reg.regulation.plateau_detected`
    RegulatoryPlateauDetected,
    /// Loop-quality telemetry recorded: `reg.regulation.loop_quality`
    LoopMetricsTelemetry,
}

impl SpanKind {
    /// Return the (namespace, local_path) pair for this span kind.
    fn namespace_and_path(&self) -> (&'static str, &'static str) {
        match self {
            SpanKind::ToolCompleted => ("reg.tool", "completed"),
            SpanKind::CurationDirectiveAcknowledged => ("reg.curation", "directive_acknowledged"),
            SpanKind::VarietyAlgedonicAlert => ("reg.variety", "algedonic_alert"),
            SpanKind::ImpactVerified => ("reg.outcome", "impact_verified"),
            SpanKind::ActionSubstituted => ("reg.outcome", "action_substituted"),
            SpanKind::ActionBlocked => ("reg.outcome", "action_blocked"),
            SpanKind::RegulatoryPlateauDetected => ("reg.outcome", "plateau_detected"),
            SpanKind::LoopMetricsTelemetry => ("reg.outcome", "loop_quality"),
        }
    }
}

/// Phase of the cybernetic cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CyclePhase {
    Sense,
    Compute,
    Compare,
    Act,
}

impl CyclePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            CyclePhase::Sense => "sense",
            CyclePhase::Compute => "compute",
            CyclePhase::Compare => "compare",
            CyclePhase::Act => "act",
        }
    }

    /// Parse a phase string into a CyclePhase variant.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "sense" | "Sense" => CyclePhase::Sense,
            "compute" | "Compute" => CyclePhase::Compute,
            "compare" | "Compare" => CyclePhase::Compare,
            "act" | "Act" => CyclePhase::Act,
            _ => CyclePhase::Sense,
        }
    }
}

/// RegulationSink — Trait for persisting Regulation events
///
/// Implemented by storage backends (e.g., RegulationArchive in hkask-storage).
pub trait RegulationSink: Send + Sync {
    fn persist(&self, event: &RegulationRecord) -> Result<(), crate::InfrastructureError>;

    /// Persist an event only when its external source identity has not been observed.
    ///
    /// The default preserves compatibility for sinks without durable deduplication.
    fn persist_if_absent(
        &self,
        _source_event_id: &str,
        event: &RegulationRecord,
    ) -> Result<bool, crate::InfrastructureError> {
        self.persist(event)?;
        Ok(true)
    }
}
