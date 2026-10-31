---
title: "Private-Inference MCP Server — functional requirements (D-2)"
audience: [operators, architects, developers, agents]
last_updated: 2026-10-30
version: "1.0.0"
status: "Proposed — revised after PI review 2026-10-30; the three rulings (all three platforms in V1, single-user V1 with group access deferred, privacy boundary sounded out during the build) are folded in; every requirement is a falsifiable outcome claim with a named verification oracle and falsifier; not yet authorized"
domain: "Private inference / MCP servers"
mds_categories: [domain, trust, lifecycle]
---

# Private-Inference MCP Server — functional requirements (D-2)

- **Status:** revised after PI review — 2026-10-30; the three rulings (all three
  platforms in V1, single-user V1 with group access deferred, privacy boundary sounded
  out during the build) are folded in below. Companion deliverables: research
  syntheses (`kask/docs/research/private-inference-research-syntheses-2026-10-30.md`),
  architecture plan + kata record, and the point-of-view memo (both under
  `kask/docs/plans/`).
- **Form:** every requirement is a falsifiable outcome claim tied to a hKask principle
  (P1–P12, `kask/docs/architecture/core/PRINCIPLES.md`) or a charter value (V1–V8,
  `kask/docs/architecture/core/magnifica-humanitas.md`), with a **named verification
  oracle** and a **falsifier**. "Private by design" is used per its published anchor
  `dpv:PrivacyByDesign` (W3C DPV 2.3): "practices regarding incorporating data protection
  and privacy in the design of information and services."
- **Verification oracle for this document:** each FR names the test or operator drill
  that adjudicates it; none is claimed passed here — oracles are contracts for the
  implementation engagement.
- The standing test throughout: does this capability deepen the PI's seat at the bench
  (`kask/docs/architecture/core/co-research-environment.md`), or automate the PI out of
  it? Every FR answers it the same way: the MCP proposes and walks the PI through; the
  PI decides, holds the credentials, and owns the routing policy.

---

## FR-1 — Provisioning: a guided lifecycle, not a silent one

**Claim.** Through the `hkask-mcp-private-inference` guided intake, a PI holding provider
credentials can bring up and tear down a GPU instance on **any of the three supported
providers** (Runpod / Hetzner / ArubaCloud — all three in V1, PI ruling 2026-10-30)
running vLLM serving a PI-selected model set — and the intake **presents the three
platforms for comparison** (price, region, GPU catalog, connection profile) so that
selecting a platform is a designed step of the workflow, not a code path — where every
state-changing step (provision, deploy, teardown) executes **only after an explicit
approval input** that names provider, instance type, region, model set, and a priced
estimate, and each stage's outcome is verified before the workflow advances (the
build-corpus-pipeline intake model: bind identities, parameters and approvals at intake;
verify each stage before expansion; halt after three no-progress attempts).
Reference: `.agents/skills/build-corpus-pipeline/SKILL.md` Stage 0 + execution loop
(`kask/scripts/`-adjacent, cited as the internal reference model).

**Principle tie.** P2 — Affirmative Consent (default deny; state changes need explicit,
scoped, revocable consent); V4 — subsidiarity (the decision sits with the PI, the lowest
capable level); the charter test (guided, never silent).

**Oracle.** (a) Contract tests over the server's tool workflow with a mock provider-API
port, constructing the server **with the capability under test** (the mock provider
client; per the MCP test-protocol rule that capability-stripped constructors can only pin
degradation): a `provision`/`teardown` call without a bound approval fails closed with a
typed `failed_precondition` naming what is unbound; the approval record carries the
five-name binding (provider/instance/region/model-set/estimate); a stage gate refuses to
advance while its predecessor is unverified. (b) One recorded live drill per provider at
implementation acceptance — operator evidence attached to the FR, not CI.

**Falsifier.** Any provisioned instance with no approval record covering it; or any stage
transition with an unverified predecessor; or a cost-bearing action executed from an
agent-initiated call the PI never approved; or an intake that hardcodes one provider's
path while three are configured (the platform-selection pattern falsified).

## FR-2 — Secure connection, fail-closed credentials

**Claim.** A zed-kask session reaches the private inference endpoint only over a channel
meeting the requirements derived from R-1: TLS to the endpoint with certificate
validation; the vLLM surface restricted to its OpenAI-compatible `/v1` prefixes behind an
allowlisting front proxy (vLLM's `--api-key` is not a perimeter — research syntheses
F1.1); provider-cloud credentials resolve fail-closed (a missing credential is
`permission_denied` naming the env var, never a silent fallback); the vLLM endpoint key is
set via environment on the instance, never a CLI argument.

**Principle tie.** P2 (scoped credentials), P12 (accountable identity on every action);
`dpv:PrivacyByDesign` (protection built into the design, not added after); D5's
one-key-one-location discipline (credentials at their provider slot, injected from the
keychain through `build_mcp_server_env`).

**Oracle.** `private_inference_connection_verify` (the server's own probe tool) plus
tests: (a) the probe's TLS handshake validates the cert and a plaintext `http://` request
is refused; (b) non-`/v1` endpoints are unreachable from the client position (allowlist
pinned by an integration test against a local vLLM fixture behind the configured proxy);
(c) the credential-absent path returns the typed `permission_denied` naming the env var
(unit test over the resolve chain); (d) `VLLM_API_KEY` never appears in any generated
instance command line (source-structure pin over the deploy-template generator).

**Falsifier.** Any successful unencrypted or unauthenticated inference call path; any
credential-absent path that yields an empty result or a fallback rather than the named
`permission_denied`; a vLLM endpoint reachable on its unauthenticated surfaces.

## FR-3 — Routing: surfaced, PI-controlled, never silent

**Claim.** A thread marked private resolves its inference to the private leg — the local
`ollama` provider or the private cloud endpoint, per a routing policy owned by the PI in
settings; unmarked threads keep the current providers unchanged; the routing decision is
surfaced in-thread (at first private use and on policy change), and the agent/thread
cannot rewrite the policy or widen its own routing (the boundary is written by someone
other than the caller). When the private leg is unavailable, a private thread fails with
a typed, named error — it never silently falls back to a public provider.

**Principle tie.** P4 — Clear Boundaries (capability separation; authority attenuates,
never amplifies; a caller-supplied value is not a boundary — hence the policy lives in
PI-owned settings, not in thread-reachable state); P1 — User Sovereignty; P10.

**Oracle.** Cross-crate pin tests at the enforcement point (pinned to the exact file by
the kata record in the architecture plan): (a) a thread marked private whose resolved
model is not a private-leg model fails the test (private-thread→public-provider is
structurally impossible); (b) the unavailability path returns the typed error and never
a public fallback (mock-provider test); (c) the surfacing path renders the decision text
(unit pin) and the policy setter lives in the settings surface, not in a tool the thread
calls.

**Falsifier (global, also feeds FR-4a).** Private-thread content (prompt or memory)
reaching any public provider — observed in any test or live session.

## FR-4 — Memory privacy invariants, per surface

**Claim.** For every memory surface, content produced by threads routed to private
inference is marked private at ingest, excluded from non-private recall, and blocked from
public export paths. **Scope (PI ruling 2026-10-30):** V1 covers a single zed-kask
user; group membership and credential sharing are deferred, and the per-user vs
group-shared boundary is sounded out during the build (P7) — the initial marking keys on
the existing `owner_webid` field and no group abstraction is built before the build shows
one is needed. The stated V1 use — the PI interacting with the private cloud through the
Curator — makes the Curator context-injection path the primary recycling risk this FR
closes. The marking predicate extends the existing `Visibility` /
`AccessControl` model (`kask/crates/hkask-types/src/visibility.rs` — Private/Shared/Public
already exists on `HMem`) with a private-inference provenance attribute; no parallel
predicate is invented (P5.5: a new instance of an existing pattern consolidates it).

| Surface | Marking predicate | Recall filter | Export block |
|---|---|---|---|
| Curator h_mems (`hkask-memory`, ingest via `kask_bridge` `RealMemoryPort::ingest_turn` + curator tools) | `Visibility::Private` + provenance attribute recording private-inference origin; `owner_webid` (the existing field — V1 single-user, PI ruling) | Curator recall paths (`curator_federated_search`, `curator_semantic_search`, `curator_memory_recall`, the bridge's curator-scoped injector) exclude private-inference-marked rows from non-private recall contexts | No curator export path emits private-inference rows into a public path |
| Corpus chunks + embeddings (`hkask-mcp-corpus`) | Intake-bound corpus privacy policy marks chunk/entity rows and their embeddings | `corpus_query` filters private-marked corpora out of non-private query contexts | `corpus_export_bundle` refuses private-marked corpora into public export paths |
| Transcripts (`hkask-mcp-media`, educt) | Transcript record attribute set when the recording's thread was private | `educt_search_transcripts` / `layers_search` exclude private transcripts from non-private search contexts | `educt_export` (`corpus_text`) refuses private transcripts |
| Goal records (`hkask-mcp-kata-kanban`) | Goal attribute carried on the service record | The `curator:goal:<id>` recall row and goal index exclude private goals from non-private recall | Goal history exports refuse private goals |
| QA and export paths (`hkask-mcp-corpus`) | Dataset/export mark inherited from source corpus policy | QA retrieval filters private corpora | `corpus_ingest_qa` output and grounding bundles refuse private-marked corpora |

**Principle tie.** P11 — Digital Public/Private Sphere ("users … can explicitly control
what is private versus shared; visibility is consent-governed"); P2.1 — Shared vs Public
Visibility (the `Visibility` enum is the live classification; its consent gates are
explicitly OUGHT-not-implemented today — these filters are the first enforcement leg of
that OUGHT); P1; P6.1 (per-user data directories as isolation); V7 — the limit as
positive.

**Oracle.** Per-surface unit tests at each filter/ingest point, plus three named global
falsifier tests (cross-crate): **(a)** private-thread content in a public provider's
request — a marked thread's model resolution can never be public (joins FR-3); **(b)** a
public thread **or public Curator thread** recalling private memory — the named
enforcement path is the Curator context-injection seam (`kask_bridge`'s
`BridgeCuratorContextInjector` → `RealMemoryPort::recall_context_curator`) plus the
curator recall tools (`curator_federated_search`, `curator_semantic_search`,
`curator_memory_recall`): a recall in a non-private context returning a
private-inference-marked row fails the test, while a private-marked Curator thread (the
PI's stated use) can recall its private rows; **(c)** private memory in a public export
path — an export tool emitting a private-marked row fails the test. Each surface's test
constructs the store with the capability under test.

**Honest limits, stated.** (1) Confidentiality against a host-level attacker on the
private cloud instance is out of scope — no TEE exists at these providers (research
syntheses F1.6); the boundary is tenancy + transport + keys, mirroring P11.1's corrected
claim style (name the actual boundary). (2) Falsifier (a) is adjudicable at the routing
seam; a hypothetical MCP-tool-mediated exfiltration path (an agent deliberately posting
private content through a public tool) is a taint-tracking problem — P4.2 records that
information flow is **not gated** by decision; the FR's claim is routing and memory
invariants, not general taint control, and the plan says so rather than implying
otherwise.

## FR-5 — Collaborative model selection: the MCP proposes, the PI decides

**Claim.** The `private_inference_model_brief` tool guides a research process in which
candidate models for the PI's task profile are presented across five dimensions —
benchmarks, licensing, VRAM fit, context length, maintenance status — each dimension
carrying its source identity (research-run discipline; leaderboards are 2026-10 snapshots
and must be re-sourced at run time, never hardcoded); the PI's selection is recorded as
the decision record, and FR-1's intake binds provisioning to that recorded selection.

**Principle tie.** P8 — Semantic Grounding (claims traceable to sources; an unsourced
dimension is labeled, never blank); the charter test (PI decides); V4; V2 — truth as a
common good (the brief presents evidence, including license restrictions the PI may not
like).

**Oracle.** (a) Unit test: for any candidate the brief emits all five dimensions, each
either carrying a source citation or explicitly `not_checked` — a blank or fabricated
dimension fails; (b) FR-1's intake contract test asserts the approval record references
the PI's recorded decision (the join is pinned); (c) VRAM-fit arithmetic (weights ×
quantization + KV cache vs card memory) is deterministic and computed by the server, not
narrated by the model — pinned by property tests over the fit function.

**Falsifier.** A provisioned model set with no decision record; a brief dimension
asserted without a source or a `not_checked` label; a license restriction material
(usage-limiting license chosen for a deployment that violates it) and not surfaced by
the brief.

---

## Cross-cutting constraints (from `.rules`, binding all FRs)

Kask-side Rust under `kask/`; no `mod.rs`; `[lib] path` in `Cargo.toml`; per-variant
error classification (`not_found` naming the missing thing; `permission_denied` naming
the env var; closed-set `invalid_argument` naming valid values); `{"content": <value>}`
envelope responses via `unwrap_tool_envelope`; `AnyJsonValue` for arbitrary-JSON inputs;
no `unwrap()` in production code; settings defaults in `Default` impls only; **no
`*_enabled` toggles — the credential's presence is the toggle**; the canonical
`HKASK_DB_PASSPHRASE` chain via `hkask_mcp_server::server::resolve_db_passphrase`;
`write_credential` changes followed by `nudge_mcp_servers`.

## PI rulings (2026-10-30) — recorded; no open questions block the plan

1. **Privacy boundary scope — sounded out during the build.** The per-user vs
   group-shared boundary is deliberately not fixed in advance (P7 — evolutionary
   architecture). The initial marking keys on the existing `owner_webid`; the build's
   named probe points: the recall-filter implementation (does owner-keying suffice for
   the single-user case?) and the credential-set shape (would a group credential need a
   boundary construct?). No group abstraction is built before the evidence shows one is
   needed.
2. **All three platforms in V1 — the platform-selection pattern is part of the
   design.** The provider abstraction, the intake comparison surface, and the per-cloud
   connection-profile differences (RunPod Secure-cloud pods; Hetzner private network +
   mandatory TLS/overlay; Aruba dedicated K8s GPUs) are load-bearing, not incidental
   (PI: removing a platform removes a critical layer of complexity that belongs in the
   MCP design). Functional consequence: a workload moves platforms by a new intake,
   not a code change.
3. **Group membership deferred; V1 is a single zed-kask user.** The initial capability:
   one user sets up a private AI cloud, marks threads private (native or Curator), and
   interacts with it through the Curator — with the invariant that private memories are
   never recycled into context injection in public model or public Curator threads
   (sharpened falsifier (b) above). Group-membership and credential-sharing constructs
   are deferred, not deleted; the revisit point is the architecture plan's §10.

## Traceability

| FR | Research stream | Architecture module (plan §) | Oracle type |
|---|---|---|---|
| FR-1 | R-1 (F1.1–F1.7), R-2 (F2.1 control-plane shape) | `hkask-mcp-private-inference` workflow | Contract tests + live drill |
| FR-2 | R-1 (F1.1, F1.3, F1.7) | Connection profile + provider clients | Probe tool + integration fixture + unit pins |
| FR-3 | R-2 (F2.2, F2.3), R-3 (F3.2) | Routing hook (Seam A) + settings policy | Cross-crate pin tests |
| FR-4 | repo analysis (P2.1/P11) | Visibility extension + per-surface filters | Per-surface tests + 3 global falsifier tests |
| FR-5 | R-3 (F3.1, F3.3) | `model_brief` tool + decision records | Unit + property + join pins |