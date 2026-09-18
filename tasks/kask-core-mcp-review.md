# zed-kask findings review — kask core, MCP servers, and seams

**Date:** 2026-09-18 · **Mode:** findings-only, read-only (no edits, no commits) ·
**Compatibility:** none required — pre-release; breaking-change proposals allowed.
**Judge question:** does this code, as built, produce elegant, efficient, precise,
functional behavior — with *elegant* anchored to the project's own principles,
not private taste?

---

## 1. Evaluation frame

### 1.1 Architecture (the fork)

- Minimal-divergence fork of Zed (`DIVERGENCE.md`, release 0.40.0 on upstream 1.21.0).
  Everything under `kask/` is kask-owned and additive; the only other divergences are
  the named D-seams (D1–D66) plus the workspace-member arrays.
- **Governing invariant (§13.1):** hKask crates never depend on zed-kask crates;
  zed-kask depends on hKask. The sole bidirectional seam is `kask_bridge` (D8).
  Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.
- 12 MCP servers run as child processes over stdio under a governed `McpRuntime`
  (single spawn authority, D3); the agent's tool surface routes through the
  process-global `KaskToolSource`.
- Keychain is the single source of truth for API keys (D5/D9): data-service keys at
  `kask://credentials/<key>`, inference-provider keys at their provider `api_url`
  slots; one passphrase (`HKASK_DB_PASSPHRASE`) for every SQLCipher DB.
- Standardized artifact storage (D28): one rooted data tree with class subdirs;
  visible artifacts under `~/Documents/zk-data/`.

### 1.2 Human-in-the-loop philosophy (what the system is for)

- zed-kask is a human-in-the-loop system for working with AI agents and tools: the
  **user is the product manager** (keeper of functional requirements, judge of what
  the work is for); the agent is the technical program manager
  (`kask/docs/architecture/functional-interaction-spec.md`, D40).
- The Curator is the in-process cybernetic regulator: algedonic alerts, escalations,
  calibration, operator advice review. Regulation (`hkask-regulation`) is the
  nervous system that keeps the loop honest (P9).
- Magna Carta sovereignty (P1–P4): user data ownership, affirmative consent,
  generative space, capability separation; SQLCipher file as the private-sphere
  boundary; P12 authenticated host mandate with *surfaced* fallbacks.

### 1.3 Quality principles (the judge's yardstick)

Anchored to `kask/docs/architecture/core/PRINCIPLES.md` (P1–P12), the Magna Carta,
and the project `.rules`:

- **P5 Essentialism & Minimalism** — "remove before adding; every module must earn
  existence by reducing total system action" (the deletion test); 5W1H gate for new
  surface; bridges earn their keep.
- **P7 Evolutionary Architecture** — types and seams emerge from real usage; no
  speculative abstraction (trait-with-one-impl is a smell).
- **P8 Semantic Grounding** — P8.3 fallback ladder, "nothing is ever untagged";
  agent output grounding; published vocabularies, never private definitions.
- **P9 Homeostatic Self-Regulation** — honest feedback loops: errors surfaced, never
  swallowed; degradations labeled, never reported as success; `reg.*` namespace
  discipline.
- **`.rules` operational discipline** — no `unwrap()` on fallible paths; never
  silently discard errors (`let _ =`); advertised invariants must point to their
  enforcement line or say "not yet enforced"; stale comments are active
  misinformation; deletions must clean up what they orphan (no dead deps, no
  orphan piles); validation gates must return `Undetermined`/`Skipped`, not
  `Ready` with empty findings; MCP servers are leaf crates (visibility tightening
  is churn; focus on truly unused items).
- **D-seam discipline** — upstream findings route through kask-side seams only;
  every zed-side edit carries its DIVERGENCE.md update in the same pass.

### 1.4 Frame verification (`.rules` claims checked against the tree)

- `script/clippy` machete scope is `kask/`-only — **verified** (`script/clippy:56-59`).
- `hkask-ledger` listed in DIVERGENCE.md's workspace-member list — **stale**: the
  crate was deleted 2026-09-08 (per `zed-host-architecture-plan.md:88`); absent
  from the tree and the root manifest.
- Docs README (updated 2026-09-18) says "11 managed MCP servers" and "D1–D56" —
  **stale**: the tree has 12 servers (`hkask-mcp-spreadsheet` added) and
  DIVERGENCE.md runs D1–D66. Systemic doc drift is itself a finding (stale
  comments are active misinformation).

---

## 2. Crate inventory and lens assignments

Process lenses (run by the lead, not delegated): **kata-improvement** (outer loop),
**metacognition** (per-phase prediction/gap), **falsifiability** (gate on every
finding at synthesis), **grill-me** (adversarial pass on top findings),
**refactor-architecture** (cross-crate synthesis from cluster inputs + own reads).

Per-crate lenses — deep scope (all four: deep-module, code-review, bug-hunt,
lean-prover), delegated in clusters A1–A5 (core) and B1–B8 (servers):

| Cluster | Crate | LOC | Status |
|---|---|---|---|
| A1 foundation | `hkask-types` | 7,399 | pending |
| A1 | `hkask-tool-port` | 147 | pending |
| A1 | `hkask-event-store` | 921 | pending |
| A1 | `hkask-lisp` | 2,028 | pending |
| A1 | `hkask-steer-core` | 242 | pending |
| A1 | `hkask-email` | 328 | pending |
| A1 | `hkask-services-core` | 792 | pending |
| A2 storage/memory | `hkask-storage` | 9,565 | pending |
| A2 | `hkask-memory` | 5,288 | pending |
| A2 | `hkask-keystore` | 777 | pending |
| A3 regulation | `hkask-regulation` | 12,765 | pending |
| A3 | `hkask-forecast` | 1,566 | pending |
| A4 runtime/server fw | `hkask-mcp` | 3,649 | pending |
| A4 | `hkask-mcp-server` | 2,144 | pending |
| A4 | `hkask-inference` | 6,316 | pending |
| A4 | `hkask-bridge-ontology` | 3,526 | pending |
| A4 | `hkask-condenser` | 2,283 | pending |
| A4 | `hkask-spreadsheet` | 1,795 | pending |
| A5 bridge+seams | `kask_bridge` | 19,342 | pending |
| B1 | `hkask-mcp-companies` | 25,509 | pending |
| B2 | `hkask-mcp-corpus` | 25,032 | pending |
| B3 | `hkask-mcp-media` | 25,712 | pending |
| B4 | `hkask-mcp-swarm` | 23,359 | pending |
| B5 | `hkask-mcp-research` | 14,827 | pending |
| B6 | `hkask-mcp-prediction-markets` | 10,458 | pending |
| B6 | `hkask-mcp-scenarios` | 6,403 | pending |
| B7 | `hkask-mcp-training` | 9,303 | pending |
| B7 | `hkask-mcp-curator` | 8,144 | pending |
| B8 | `hkask-mcp-kata-kanban` | 8,758 | pending |
| B8 | `hkask-mcp-portfolio` | 6,440 | pending |
| B8 | `hkask-mcp-spreadsheet` | 625 | pending |

Bounded scope (zed-side kask-owned crates; lenses: deletion test + seam health +
obvious-defect scan; rationale: kask-owned but zed-side UI crates — the mission's
center of mass is `kask/` + `mcp-servers/` + seams; recorded as bounded, not
skipped):

| Cluster | Crate | LOC | Status |
|---|---|---|---|
| C1a widgets | `hkask-viz-core` | 660 | pending |
| C1a | `hkask-media-widget` | 6,243 | pending |
| C1a | `hkask-graph-widget` | 2,732 | pending |
| C1a | `hkask-kanban-widget` | 3,147 | pending |
| C1b widgets/leafs | `hkask-portfolio-widget` | 2,199 | pending |
| C1b | `hkask-scenarios-widget` | 1,698 | pending |
| C1b | `hkask-spreadsheet-widget` | 1,659 | pending |
| C1b | `hkask-swarm-widget` | 519 | pending |
| C1b | `hkask-media-benchmarks` | 317 | pending |
| C1b | `hkask-steer` | 579 | pending |
| C1b | `hkask-tool-invoker` | 352 | pending |
| C1b | `hkask-conversation-injector` | 258 | pending |
| C2 panels | `swarm_panel` | 10,145 | pending |
| C2 | `kanban_panel` | 3,922 | pending |
| C2 | `portfolio_panel` | 992 | pending |
| C2 | `marketplace_ui_common` | 220 | pending |

Seams under explicit audit (A5 brief + lead synthesis):

1. Tool-contract envelopes (`{"content": ...}` unwrap discipline across all servers).
2. Credential/passphrase resolution (canonical `resolve_db_passphrase` adoption;
   allowlist alignment).
3. Settings-to-server sync (`mcp_env` → per-server `config_env` allowlists → actual
   env reads; `nudge_mcp_servers`; load/unload lifecycle).
4. Shared DB patterns (SQLCipher open/passphrase, path layout, schema duplication
   across servers).
5. GPUI/background-runtime boundary (Send/Sync, tokio-vs-GPUI timer traps).
6. Tool-advertisement truth (server `TOOL_NAMES` → Steer overlays → prompts).
7. Inference IPC lifecycle (socket path, grants, circuit breaker).

---

## 3. Prioritized findings

*(filled at Phase 4 synthesis — each finding: file:line, falsifiable claim,
principle served/violated, severity, effort, discriminating check)*

## 4. Ranked opportunities

*(filled at Phase 4 synthesis)*

## 5. Process record

**Kata target condition:** every inventory crate examined by its assigned lenses;
every finding falsifiable with file:line; top findings grilled; report ranks by
severity × effort; no upstream-direct-edit proposals; no duplicated findings.

**Phase 0 (Ground) — outcome:** frame recorded from DIVERGENCE.md, docs README,
PRINCIPLES.md, Magna Carta, `.rules`; `.rules` spot-verified (machete scope ✓,
hkask-ledger stale ✗, server-count drift ✗). Exit criteria met.

**Phase 1 (Inventory) — outcome:** 31 deep + 16 bounded crates enumerated from the
tree + root manifest; clusters assigned; seams listed. Exit criteria met (every
crate has an owner).

**Phase 2 predictions (metacognition, recorded before the passes):**

- P2-1: big servers re-implement shared helpers (envelope parsing, DB open, env
  resolution) instead of using `hkask-mcp-server` primitives; expect ≥3
  duplication findings.
- P2-2: doc drift is systemic — "11 servers"/"D1–D56" stale across ≥4 documents.
- P2-3: kask_bridge env allowlists misalign with actual server env reads in ≥2
  places (transitive env reads are the known trap class).
- P2-4: ≥2 advertised invariants (doc-claimed gates) without enforcement lines in
  regulation/storage.
- P2-5: `hkask-types` carries residual dead types from deleted subsystems
  (pods/ledger/condenser teardowns).

*(per-cluster outcomes appended after each wave; gaps measured at Phase 4)*