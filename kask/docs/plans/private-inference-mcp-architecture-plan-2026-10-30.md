---
title: "Private-Inference MCP Server — architecture plan (D-3, with kata record D-5)"
audience: [architects, developers, operators, agents]
last_updated: 2026-10-30
version: "1.0.0"
status: "Proposed — revised after PI review 2026-10-30 (rulings folded in: all three platforms in V1, single-user V1 with group access deferred, privacy boundary sounded out during the build); implementation is a separate engagement, nothing here is built or claimed built"
domain: "Private inference / MCP servers"
mds_categories: [domain, composition, trust, lifecycle]
---

# Private-Inference MCP Server — architecture plan (D-3, with kata record D-5)

- **Status:** revised after PI review — 2026-10-30 (rulings folded in: all three
  platforms in V1 — §3.1; single-user V1 with group access deferred; the privacy
  boundary sounded out during the build). Implementation is a separate engagement;
  nothing here is built or claimed built.
- **Companion deliverables:** requirements
  (`kask/docs/plans/private-inference-mcp-requirements-2026-10-30.md`), research
  syntheses (`kask/docs/research/private-inference-research-syntheses-2026-10-30.md`),
  point-of-view memo (`kask/docs/plans/private-inference-point-of-view-memo-2026-10-30.md`).
- **Method:** the refactor-architecture discipline (friction discovery → deep-module
  design with the deletion test on every proposed module → seam placement), over the
  research findings and the repo's own constraint surface. The plan is greenfield-plus:
  it extends existing architecture rather than refactoring working code, so the
  discipline's weight falls on module justification (the deletion test) and seam
  honesty.
- **Verification oracle for this document:** every module carries a deletion-test
  verdict; every zed-side boundary is pinned to named files with pin-test sketches; the
  kata record (§9) states what its one experiment measured and settled.

## 1. Inputs and constraints

- Principles P1–P12 (`kask/docs/architecture/core/PRINCIPLES.md`); values V1–V8
  (`kask/docs/architecture/core/magnifica-humanitas.md`); the charter test
  (`kask/docs/architecture/core/co-research-environment.md`).
- `.rules` constraints, binding: kask-side Rust under `kask/`; no `mod.rs`; `[lib] path`
  declarations; per-variant error classification; fail-closed credentials naming the env
  var; envelope tool responses; `AnyJsonValue` inputs; no `unwrap()` in production;
  settings defaults in `Default` impls only; no `*_enabled` toggles; canonical
  `resolve_db_passphrase`; `nudge_mcp_servers` after credential writes.
- Reference models with provenance:
  - **Routing/gateway:** zk-ref-open openrouter routing model
    (`~/Documents/zk-data/zk-reference-open/sources/openrouter-routing-model.md`, CC BY
    4.0, zed-kask project): route by qualified name, trust the normalized envelope,
    attribute cost by the router's usage — a model-routing gateway sits between the
    caller and many providers.
  - **Protocol:** MCP specification (primary source; 2026-07-28 revision verified in
    R-2). The term "Model Context Protocol" resolves only to the coarse 5W1H core in the
    pinned ontologies — **operator ruling requested** (see §8).
  - **Infrastructure:** provider documentation (Runpod, Hetzner, ArubaCloud) as primary
    sources — verified claims in R-1; the not-checked gaps are named in §10.
  - **Security:** in-repo security-audit reference frameworks (OWASP LLM Top-10 2025,
    MITRE ATLAS v5.1, NIST SSDF SP 800-218A; `.agents/skills/security-audit/SKILL.md`)
    plus R-1's published sources.
  - **User interaction:** the build-corpus-pipeline guided intake model
    (`.agents/skills/build-corpus-pipeline/SKILL.md` — bind identities, parameters and
    approvals at intake; run the smallest discriminating probe; verify each stage before
    expansion; three no-progress halts).
- **PI rulings (2026-10-30), binding:** (1) all three provider platforms are built in
  V1 — the platform-selection pattern is a design requirement (§3.1), not a sequencing
  choice; (2) V1 is a single zed-kask user — group-membership/credential-sharing
  constructs are deferred, and the per-user vs group-shared privacy boundary is sounded
  out during the build (P7), the initial marking keyed on the existing `owner_webid`;
  (3) the stated V1 use is the PI interacting with the private cloud through the
  Curator — private threads mark and route identically for Curator sessions, and private
  memories are never recycled into context injection in public model or public Curator
  threads (FR-4(b)'s sharpened oracle).

## 2. Friction discovery (current condition, from the repo)

1. **No guided provisioning surface.** Bringing up a GPU instance is a web-console
   procedure with no approval record, no stage verification, and no connection profile —
   the PI's credentials and judgment live in browser tabs (friction: sovereignty without
   instruments).
2. **No privacy routing.** Threads resolve their model from the registry with no
   notion of private content; any thread can dispatch anywhere its profile allows
   (`crates/agent/src/thread.rs:2245`, `Thread::model()` — the dispatch decision).
3. **Memory has a privacy vocabulary but no privacy enforcement.** `Visibility` /
   `AccessControl` exist on `HMem` (`kask/crates/hkask-types/src/visibility.rs`) and P2.1
   records the consent gates as OUGHT-not-implemented; recall paths are curator-scoped
   but not privacy-scoped (D6).
4. **Provider credentials already have a fail-closed chain and a RunPod precedent**
   (D5/D29: `RUNPOD_API_KEY` at its provider slot; serverless vLLM endpoint discovery) —
   the capability composes with existing seams rather than inventing a credential story.
5. **The local leg exists** (`ollama` wired in `hkask-inference`) but no policy surface
   decides when it is the private default.

## 3. The central decision: control plane, not data path

**Decision.** `hkask-mcp-private-inference` is a provisioning/policy **control plane**.
Inference routing stays in the provider layer — zed's `LanguageModelRegistry` and the
kask-side qualified-name resolution — exactly where the openrouter reference model puts
the gateway. Model tokens never traverse the new server.

**Evidence.** The zk-ref-open model (qualified names as routing keys; the registry is
the gateway); the opening expectation's falsifier probe returned **no data-path provider
MCP gateway** in-window (R-2 F2.2: RunPod's official servers manage
pods/endpoints/volumes; the one MCP-exposing gateway aggregates tools; the MCP
2026-07-28 spec **deprecates Sampling** with the migration "integrate directly with LLM
provider APIs"). A data-path MCP hop would add a trust boundary and latency to every
token and buy nothing the registry lacks.

**Functional consequence for the PI.** Private inference behaves like any provider in
the model picker once connected; the MCP's value is the guided lifecycle and the
privacy invariants, not a new chat path.

### 3.1 The platform-selection pattern is a design requirement (PI ruling 2026-10-30)

Building all three providers from day one is a design position, not scope inflation:
the capability's product is *choosing and operating a platform under privacy
constraints*, so the provider abstraction and the intake comparison surface are where
the complexity budget is spent. Concretely: one `ProviderCloudPort` (M2) with **three
production implementations** (RunPod REST/GraphQL; Hetzner Cloud API; ArubaCloud API)
plus the mock port; per-provider **connection-profile templates** carrying each cloud's
real security shape (RunPod: Secure-cloud pods, `Cloud Type: SECURE` [verified];
Hetzner: private networks that still require TLS/overlay [verified]; Aruba: dedicated
K8s-integrated GPUs [verified]); and the intake options menu comparing price / region /
GPU catalog / connection profile across all three (FR-1). What this buys functionally:
a workload moves platforms by a new intake, not a code change — and the design carries
the cross-platform complexity the PI wants in the MCP, rather than deferring it to
per-provider forks.

## 4. Modules, each with a deletion-test verdict

| # | Module (location) | Responsibility | Deletion test | Verdict |
|---|---|---|---|---|
| M1 | `kask/mcp-servers/hkask-mcp-private-inference/` (new server) | The guided lifecycle: intake → approval → provision → deploy → verify → connect → teardown; decision records; inventory; verify probe; model brief | Delete → the PI hand-rolls provisioning in web consoles with no approval record, no stage gates, no verify, no privacy marking; the complexity reappears as unaudited operator procedure | **EARNS** |
| M2 | Provider clients as a `providers/` submodule of M1 over a `ProviderCloudPort` trait | RunPod REST/GraphQL; Hetzner Cloud API; ArubaCloud API — **all three built in V1** (PI ruling) + the mock port (test seam) | Delete → three provider clients duplicate discovery/auth/deploy shapes and the platform-selection pattern (§3.1) has no seam; with three production impls the trait is a real interface, not the trait-with-one-impl trap. A standalone `hkask-provider-clouds` **crate** still fails: only M1 consumes it | **EARNS as submodule (3 impls + mock); standalone crate REJECTED** |
| M3 | Private endpoint as a **configured instance of the existing `openai_compatible` provider class** (D69 pattern) | The connect step writes a `language_models.openai_compatible` entry (name `Private-vLLM`, api_url = instance, served-model ids from `/v1/models` discovery) + the vLLM key at the provider `api_url` keychain slot (D5 one-key-one-location) | A **new `ProviderId::Private` in `hkask-inference`** fails the deletion test: delete it and nothing is lost — the existing provider class + registry already carry qualified-name routing (the P5.5 answer: a new instance of an existing pattern consolidates, it does not copy) | **CONFIGURED INSTANCE; new provider code REJECTED** |
| M4 | Privacy marking in `hkask-types` | A private-inference provenance attribute alongside `Visibility`/`AccessControl` (marking predicate of FR-4) | A standalone `hkask-privacy` crate fails: one attribute + docs; folding into `hkask-types` loses nothing | **EXTENDS `hkask-types`; standalone crate REJECTED** |
| M5 | Per-surface filters (in each owning store) | `hkask-memory` curator recall paths; `hkask-mcp-corpus` (`corpus_query`, `corpus_export_bundle`, `corpus_ingest_qa` outputs); `hkask-mcp-media` (`educt_search_transcripts`, `layers_search`, `educt_export`); `hkask-mcp-kata-kanban` goal recall/index | Delete any → FR-4's falsifiers (b)/(c) become unadjudicable at that surface; the complexity does not reappear anywhere else — it is simply gone, and the invariant with it | **EARNS** (each surface independently) |
| M6 | Routing hook (Seam A, zed-side) | Thread privacy mark on `KaskThreadState` (persisted via the thread metadata, D28 per-message storage) + enforcement at `Thread`'s model resolution: a marked thread's resolved model must be a private-leg model (policy allowlist from settings); unavailability fails typed, never falls back | Delete → FR-3(a) is unenforced: the thread's model IS the dispatch decision (`crates/agent/src/thread.rs:2245`), so no kask-only hook can substitute | **EARNS** |
| M7 | Settings + credentials chain (Seam B, extends D9) | `kask.private_inference` settings section (routing policy: private-leg provider order, fail-closed stance; defaults in `Default` impls; **no `*_enabled` toggles** — the credential's presence is the toggle); settings-UI page (PI-owned policy surface); `BuiltinMcpServer` registration with per-server allowlists | Delete → the routing policy has no PI-owned home; it would live either in agent-writable state (P4 violation) or nowhere | **EARNS** |
| M8 | State store (SQLCipher, D28 layout: `mcp/private-inference/private-inference.db`, `HKASK_PRIVATE_INFERENCE_DB` override, `resolve_db_passphrase` chain) | Approval records (approval tokens = server-minted handles per MCP 2026-07-28 statelessness), decision records, connection profiles (key **references**, never key material), instance inventory, verify results; idempotency keys on identity-minting calls (the D3 kata-kanban shape) | Delete → approvals/teardown are not replay-safe and an `Interrupted` retry can double a state change (the D3 replay-protection precedent) | **EARNS** |
| M9 | Model-brief service (in M1) | FR-5's five-dimension research process, re-sourced at run time; deterministic VRAM-fit arithmetic | Delete → FR-5 gone; the PI hand-picks models without license/VRAM discipline and the dimension work reappears ad hoc in chat threads | **EARNS** |
| M10 | Connection-verify probe (in M1) | FR-2's runtime adjudicator: TLS + cert validation, `/v1`-only reachability, credential-resolution check | Delete → FR-2 unadjudicable after connect; a broken channel surfaces as mystery failures | **EARNS** |

**Rejected-module register (deletion-test failures, recorded per P5.5):**
`hkask-provider-clouds` standalone crate (M2); `ProviderId::Private` (M3);
`hkask-privacy` standalone crate (M4); a `policy_set` MCP tool — the routing policy must
be PI-owned settings, and an agent-callable write would make the boundary
caller-supplied (P4: "a per-call token check whose value the caller supplies is not a
boundary"); a data-path gateway mode in the server (§3 evidence); an unmark agent tool
(the mark narrows exposure and is agent-callable at PI direction; unmarking **widens**
exposure, so it lives on the PI's surfaces — settings page / thread-header toggle, both
inside Seam A/B — never on the agent's tool path).

## 5. Tool surface sketch (M1)

All responses are `{"content": <value>}` envelopes (`unwrap_tool_envelope`); inputs
taking arbitrary JSON use `AnyJsonValue`; errors are per-variant: `not_found` naming
the missing thing (instance id, approval token), `permission_denied` naming the env
var (credentials), `failed_precondition` naming the unbound stage (approval/state),
`invalid_argument` naming valid values (closed sets).

| Tool | Purpose | Key inputs / notes |
|---|---|---|
| `private_inference_intake_status` | Current workflow state, unbound parameters, next PI decision | None |
| `private_inference_intake_options` | Provider menu: instance types/regions/prices (provider-API-backed), connection options per cloud (R-1 menu: TLS front, overlay, tunnel, private network) | `provider?` filter |
| `private_inference_model_brief` | FR-5: five-dimension model brief for a task profile, sources cited or `not_checked` | `task_profile` (AnyJsonValue), `candidates?` |
| `private_inference_intake_bind` | THE approval: binds provider/instance/region/model-set/priced estimate/connection profile + the PI's decision record; mints the approval token (server-minted handle) | `idempotency_key`, binding fields; a missing field is a typed refusal |
| `private_inference_provision` | Execute provisioning; requires the approval token | `approval_token` — typed `failed_precondition` without it |
| `private_inference_deploy_status` | Deploy progress + per-stage verification results | `instance_id?` |
| `private_inference_connection_verify` | FR-2 probe (TLS, `/v1`-only reachability, credential resolution) | `instance_id` |
| `private_inference_connection_profile` | Emit the connect steps: the `openai_compatible` settings entry, the keychain slot write (executed by the **agent session** via existing `write_credential` + settings built-ins, then `nudge_mcp_servers`) — the MCP child never writes zed settings or the keychain itself | `instance_id` |
| `private_inference_teardown` | Tear down; requires a fresh approval token; idempotent | `approval_token`, `idempotency_key` |
| `private_inference_status` | Active-state overview: instances, spend, model set | None |
| `private_inference_policy_get` | Read the routing policy (surfacing; the policy is **written** in Settings → Kask, never by a tool) | None |
| `private_inference_thread_mark` | Set the private mark on a thread (narrowing; at PI direction) | `thread_id` |

Workflow state machine (Stage-0 model): `IDLE → INTAKE → PROVISIONED → DEPLOYED →
CONNECTED → ACTIVE`, each transition verified before advance; three no-progress
attempts halt for the PI; teardown returns to `IDLE` with the inventory updated.
Deploy mechanism per provider is an implementation-engagement decision with named
options (RunPod: official vLLM Pod/Serverless templates [verified nav-level]; Hetzner/
Aruba: cloud-init or SSH — the plan names, does not choose, the mechanism).

## 6. Settings and credentials chain (M7)

```
zed keychain (kask://credentials/..., provider api_url slots per D5)
  → build_mcp_server_env (per-server allowlists: credentials + config_env)
  → hkask-mcp-private-inference child env
  → resolve_db_passphrase(&ctx.credentials)  (HKASK_DB_PASSPHRASE, 2-tier canonical chain)
```

- Credentials allowlist (server registration, `kask/crates/kask_bridge/src/mcp_servers.rs`):
  `RUNPOD_API_KEY` (already in the chain, D29) + the Hetzner and ArubaCloud provider keys
  (all three in V1, PI ruling; exact env var names verified against provider docs at
  implementation — **not_checked** in-window); `config_env`: `HKASK_DATA_DIR`,
  `HKASK_PRIVATE_INFERENCE_DB`, `HKASK_DB_PASSPHRASE`. Allowlists align with actual
  env reads (the `.rules` alignment rule).
- The instance's vLLM key: generated at deploy (env-set on the instance, never a CLI
  arg), stored by the PI at the provider `api_url` slot through the existing
  `write_credential` flow, followed by `nudge_mcp_servers` — no new credential-chain code.
- Missing credentials: `permission_denied` naming the env var, never empty results.
- Policy: `kask.private_inference.routing` — private-leg provider order
  (e.g. `["ollama", "Private-vLLM"]`), fail-closed stance; defaults in `Default` impls
  only; no `*_enabled` toggles.

## 7. D-seam placement

- **Seam A — NEW D-seam (next number, D97 per the D1–D96 register): "Private-thread
  routing + thread privacy mark."** Files: `crates/agent/src/kask_thread_state.rs`
  (extend — the fork-owned per-thread state home, the D2/D25 pattern), `crates/agent/src/thread.rs`
  (enforcement at the model-resolution point; `Thread::model()` at `:2245` is the
  dispatch decision), thread metadata persistence (`crates/agent/src/db.rs`, the D28
  metadata blob carries the mark), and a thread-header toggle in `crates/agent_ui`
  (the unmark surface). **Curator sessions mark and route identically to native agent
    threads** — the PI's stated V1 use is interacting with the private cloud through the
    Curator, so Seam A's enforcement covers Curator threads too, and FR-4(b)'s recall
    filter applies to the Curator context-injection path (`BridgeCuratorContextInjector`
    → `recall_context_curator`). Pinning tests, same change per `.rules` divergence discipline:
  (1) a private-marked thread never resolves a public provider (cross-crate: agent +
  settings); (2) private-leg unavailability fails typed with no public fallback
  (mock-provider); (3) the mark round-trips through thread persistence; (4) the
  routing-change surfacing renders. Every `// zed-kask:` comment carries its test.
- **Seam B — EXTEND D9 (Settings + credentials):** the `kask.private_inference` section
  in settings content, the Settings → Kask page, `mcp_env` allowlists, and the
  `kask/docs/reference/kask-settings.md` section — all inside D9's existing file list.
- **Kask-side (no seam):** the server crate and its store; the registration inventory
  per the D86 pattern (`kask/crates/kask_bridge/src/mcp_servers.rs` + root `Cargo.toml`
  workspace member/dep + `kask/scripts/build/mcp-servers.txt` +
  `kask/docs/reference/mcp-servers/README.md`); M4/M5 filters inside their owning crates.
- **Explicitly no seam needed:** the provider entry for the private endpoint (M3 — a
  settings entry written through existing settings behavior), the connect step (existing
  built-in tools), MCP-server registration (kask-side).

## 8. Ontology anchor record (rulings requested)

- `privacy by design` → `dpv:PrivacyByDesign` (W3C DPV 2.3, domain-supplement tier) —
  anchored; the FR language follows its definition.
- `multi-tenancy` → `cso:multi-tenancy` (CSO 3.5, domain-supplement tier) — anchored.
- `confidential computing`, `virtual private cloud`, `Model Context Protocol` → coarse
  5W1H core only — **operator rulings requested** (per the P8.3 ladder: record identity
  and authority in the derived registry; never assign private definitions meanwhile).
  The plan uses provider-primary sources for infrastructure semantics and the MCP
  specification as primary source for protocol semantics, which is the honest substitute
  until the rulings land.

## 9. Kata record (D-5) — one bounded Improvement Kata cycle

- **Step 1 (direction):** the work order's target condition — every requirement
  falsifiable with a named oracle; every module deletion-tested; every zed-side boundary
  pinned to named files.
- **Step 2 (current condition, measured over the drafted deliverables):** FRs with
  named oracles **5/5** (D-2); load-bearing research sources with annotated verification
  states **13/13** across three completed runs; modules with written deletion-test
  verdicts **0** (D-3's own content — pending this document); zed-side seams pinned to
  named files **1 of 2** — the settings/credentials surface pinned via D9's known file
  list; FR-3's routing enforcement point **unpinned**. Focus metric
  (`seams_pinned`): **before = 1**, target = 2. Method: file counts over the drafted
  documents and the DIVERGENCE.md register (D — taken from the tree).
- **Step 3 (target + focus obstacle):** target as above; the ONE focus obstacle:
  **FR-3's enforcement point** — the plan asserted routing stays in the provider layer
  without naming the component that resolves a thread's model, so "a private thread
  never dispatches to a public provider" had no enforcement location and Seam A could
  not be placed.
- **Step 4 (experiment — Plan):** next step: grep the thread→model resolution path
  (`model_override` handling in `kask_bridge`; model resolution in
  `crates/agent/src/thread.rs`). **Prediction (stated before the probe):** agent threads
  resolve their model zed-side — `Thread` holds the serving model resolved from
  `LanguageModelRegistry` per profile/settings, with `KaskThreadState` as the fork-owned
  per-thread home (D2/D25 pattern), while `kask_bridge`'s `LanguageModelInferencePort`
  honors `model_override` only for MCP-server inference over IPC; therefore Seam A is a
  genuine zed-side D-seam at the thread model-selection point — no kask-only hook can
  enforce FR-3(a).
- **Step 4 (Do + Check):** probe executed (two greps). Observed: `LanguageModelInferencePort`
  resolves overrides against the registry with a **typed, no-substitution error** on
  failed resolution (`kask/crates/kask_bridge/src/inference_chat.rs:664-670`, `:863-870` —
  the bridge path serves MCP-server inference; an existing fail-closed precedent FR-3
  cites); and `Thread::model()` returns the thread's held model
  (`crates/agent/src/thread.rs:2245`). **Prediction result: SUPPORTED.** Post-experiment
  metric: `seams_pinned` **after = 2**. Deterministic check (`lisp_eval`, D):
  gap-to-target `(- 2 2)` = **0**; improvement `(- 2 1)` = **+1**.
- **Act:** Seam A pinned (§7) with its four pin-test sketches; M6's deletion test
  firmed to the observed dispatch point; the rejected `policy_set` tool and unmark-tool
  decisions recorded (§4). One cycle, target attained on the focus metric — the cycle
  closes; remaining gaps are the implementation engagement's (§10).

## 10. Risks and open items for the implementation engagement

1. **Provider API clients (all three, from the start — PI ruling):** RunPod
   REST/GraphQL verified to exist (primary docs); Hetzner Cloud API and ArubaCloud API
   auth/shapes **not_checked** in-window — verifying both is a first-order
   implementation task, and all three land in V1 together.
2. **Protocol posture:** `hkask-mcp-server`/rmcp's support level against MCP
   2026-07-28 (stateless, MRTR) is unverified — the stateless approval-token design
   matches the spec's direction, but the stack's actual protocol version must be
   confirmed, not assumed.
3. **vLLM hardening specifics:** the `/v1`-prefix-only authentication claim rests on
   secondary sources citing vLLM docs (R-1 F1.1, partial) and CVE-2026-22778 on one
   source (inferred) — the implementation engagement verifies both against vLLM's
   primary docs and pins a version floor check in `connection_verify`.
4. **Overlay mechanics:** deploying a WireGuard/Tailscale-class overlay from a guided
   workflow needs a controller decision (who holds overlay keys — the PI's keychain);
   Hetzner requires TLS/overlay even on private networks (F1.3, verified).
5. **PI rulings folded in (2026-10-30):** no requirement blocks on an open question.
   The privacy boundary is sounded out during the build — named probe points: the
   recall-filter implementation (does `owner_webid` keying suffice for the single-user
   case?) and the credential-set shape (would a group credential need a boundary
   construct?); group membership is deferred with a V1 scope of one zed-kask user; the
   Curator interaction path is a first-class V1 use, which is why FR-4(b)'s oracle names
   the context-injection seam.
6. **Privacy-filter blast radius:** the M5 filters touch four stores' recall paths;
   the implementation engagement sequences them surface-by-surface with each surface's
   falsifier test landing in the same change (the cross-seam test rule).

## 11. Replaces / extends (P5.5 honesty)

**Extends:** `hkask-types` (`Visibility`/`AccessControl` + provenance attribute);
the D28 storage layout (`mcp/private-inference/`); the D9 settings surface
(`kask.private_inference` + UI page); the D69 `openai_compatible` provider class (a
configured instance — zero new provider code); the D5 credential chain (new provider
keys join the existing allowlist discipline); the D86 server-registration inventory.
**Replaces:** nothing — this is the first capability of its class; no existing
provisioning or privacy-routing surface exists to remove, and the rejected-module
register (§4) records what was deliberately **not** built (the deletion-test burden the
plan carries in place of removals). **Net planned surface:** +1 MCP server crate
(+ state store), +1 settings section + UI page, +1 new D-seam (D97) + D9 extension,
per-surface filter code in four stores, +3 planning documents (this set).

**V1 scope (PI ruling 2026-10-30):** all three platforms; a single zed-kask user; the
group-membership and group-credential constructs deferred, not designed speculatively
(P7) — their revisit point is §10 item 5.