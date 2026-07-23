---
title: zed-kask — Minimal-Divergence Fork Architecture & Migration Plan
audience: hKask architects / zed-kask integrators
last_updated: 2026-07-23
version: 0.31.0
status: proposal
domain: architecture
mds_categories: [composition, trust, lifecycle]
---

# zed-kask — Minimal-Divergence Fork Architecture & Migration Plan

> **One-line frame:** `Clones/zed-kask` is a **fork of Zed** that tracks `upstream` (`zed/zed`) and diverges in **exactly three places**: (1) the **skill module** (skill execution → hKask's `ManifestExecutor`), (2) the **Curator agent** (a new native agent backed by hKask), and (3) the **hKask tool-processing code** (compiled-in hKask crates + in-process tool hosting). Everything else stays byte-identical to upstream and is re-merged regularly. hKask (`Clones/hKask`) is trimmed to **only** the Curator + user sovereignty + the tools. **No backward compatibility.** Principle: *as simple and minimal as possible — and the fork's divergence surface is itself minimal.*

Reasoning chain: `pragmatic-semantics` → `pragmatic-cybernetics` → `falsifiability` → `sequential-inquiry` → `kata-improvement` → `improve-codebase-architecture` → `essentialist` → `skill-router` → `task-breakdown` → `grill-me` → `self-critique-revision`, grounded by reading the actual `zed-kask` crate tree.

---

## 0. Fork Location & Upstream-Sync Strategy (load-bearing)

- **Fork:** `Clones/zed-kask` — `origin` = `github.com/mdz-axo/zed-kask.git`, `upstream` = `github.com/zed/zed.git`, on `main`, currently **in sync** with upstream.
- **Divergence policy:** keep `main` a near-clone of `upstream/main`. All hKask integration is isolated to a **small, named set of crates/files** (§3) so `git fetch upstream && git merge upstream/main` stays low-conflict. No scattered edits across Zed's tree.
- **hKask wiring (FULL MERGE — §14):** hKask's keep-crates + skills registry + scripts + docs are moved **into the zed-kask repo** under a `kask/` namespace (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/skills/`, `kask/scripts/`, `kask/docs/`) and added as zed-kask workspace members. The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. (Replaces the prior path-dep/submodule approach, which dissolved once hKask could no longer run standalone.)
- **Sync cadence (ongoing, Phase 7):** rebase/merge `upstream/main` regularly; resolve conflicts only in the divergent crates; run the hKask integration tests after each sync. The whole point of the fork is to *inherit Zed's improvements for free* — divergence is the cost, so minimize it.
- **Deployment & identity model (load-bearing):** zed-kask is a **local single-user install** — one user, one sovereign UserPod, on the user's own machine. There is **no cloud server, no Kubernetes/K3s deployment, no hKask OAuth, no Admin/Member roles, no invite flow.** Identity and sign-on are **Zed's**: the user signs into their existing Zed account (`*.zed.dev` account/collab endpoints, kept by D7), and the local UserPod is bound to that signed-in account at startup. Multiplayer, collaboration, voice, and federation ride on **Zed's comms/voip/CRDT** — hKask's Matrix/7R7 transport is dropped entirely. hKask's own identity crate is trimmed to the UserPod/PodDeployment data structures only (§2.2); WebID-as-separate-identity, OAuth, roles, and invite are deleted.

---

## 1. The Enhanced Prompt (minimal-divergence fork)

> Fork Zed into **`zed-kask`** (`Clones/zed-kask`), tracking `upstream` and diverging only in three areas. Trim hKask (`Clones/hKask`) to the Curator + sovereignty + tools, compiled into zed-kask. No backward compatibility.
>
> 1. **zed-kask owns the generic surface and infra** (unchanged from upstream): chat (Agent Panel), GitHub, editor UI, comms/voip/CRDT (replacing Matrix entirely), the **inference router** (`crates/language_model*`), the **provider keystore** (`crates/credentials_provider`), thread storage. These stay byte-identical to upstream.
> 2. **Divergence #1 — skill execution:** change `crates/agent_skills` + `crates/agent/src/tools/skill_tool.rs` so a skill activation runs hKask's **manifest model** — `manifest.yaml` + Jinja2 templates driving a WordAct/FlowDef/KnowAct/RenderAct cascade with PDCA loops, gas/rjoule, OCAP gating — via the compiled-in `ManifestExecutor`, instead of `render_skill_envelope()` injecting the `SKILL.md` body.
> 3. **Divergence #2 — Curator agent:** add the Curator (VSM S4) as a native in-process zed-kask agent (singleton; `CuratorHandle` mpsc authority never crosses a process boundary), selectable in the Agent Panel. ACP is optional (only for external-agent interop).
> 4. **Divergence #3 — hKask tool processing:** compile hKask's keep-crates into zed-kask; host the 12 default-load MCP tools (§2.4) **in-process** (new transport alongside `context_server`'s `StdioTransport`); emit `reg.*` spans directly.
> 5. **Thread → memory:** zed-kask threads parsed into UserPod / Curator episodic + semantic memory (extends the existing ACP per-turn encoding).
> 6. **Remove everything redundant from hKask:** inference router, daemon, ACP seam, MCP-stdio, REPL, chat service, Matrix (all of it), communication MCP, **hKask identity/OAuth/Admin-Member roles/invite flow + cloud/Hetzner/K3s deployment**, backward-compat shims. Identity/sign-on becomes **Zed's account** (the user logs into their existing Zed account); the local UserPod is bound to it. **Nothing is removed from zed-kask** — it tracks upstream.
> 7. **Magnac Carta P1–P4, P12 non-negotiable.** `hkask-guard` becomes a layer in zed-kask's inference path so **every** LLM boundary (direct chat + skill cascade + Curator) is guarded — coverage *improves*.

---

## 2. The Essentialist Split (what zed-kask owns vs what hKask keeps)

### 2.1 zed-kask owns (generic — inherited from upstream, NOT modified except integration seams)

Inference routing (`crates/language_model`, `language_model_core`, `language_models`, `language_models_cloud`), provider keystore (`crates/credentials_provider`, `zed_credentials_provider`), chat/Agent Panel (`crates/agent`, `agent_ui`), editor/GitHub/comms/voip/CRDT (`crates/workspace`, `project`, etc.), thread storage (`crates/agent/src/thread_store.rs`), MCP stdio hosting (`crates/context_server`). These stay upstream-identical; we only *add seams* (guard layer, in-process transport) where hKask plugs in.

### 2.2 hKask keeps (unique: curator + sovereignty + tools) — compiled into zed-kask

| Crate | Why irreducible |
|---|---|
| `hkask-types` | Foundation: IDs, `InferencePort` trait, `RegulationSpan`, vocab. |
| `hkask-storage` | **Sovereignty:** per-pod SQLCipher encrypted private sphere (P11.1). |
| `hkask-memory` | Unique semantic/episodic memory + consolidation. |
| `hkask-regulation` | Cybernetic nervous system (`reg.*`, variety, algedonic, set-points). |
| `hkask-templates` | **The tools/skills:** `ManifestExecutor` + registry + cascade + PDCA. |
| `hkask-pods` | **Curator + UserPod** + deployment (sovereignty + curator). |
| `hkask-guard` | **Magna Carta floor (P3.1)** — becomes a layer in zed-kask's inference path. |
| `hkask-capability` | **OCAP** — sovereignty enforcement. |
| `hkask-keystore` (trimmed) | **Sovereignty crypto only:** OCAP signing, DB passphrase, internal-secret derivation w/ versioning. *Storage* backend → zed-kask keystore. |
| `hkask-wallet`, `hkask-ledger` | rJoule energy budget + hMem accounting. |
| 12 MCP servers (default load; §2.4) | **The tools** — hosted in-process in zed-kask. |
| `hkask-mcp-server` (framework) | Trim if zed-kask's context_server hosts them natively; keep the `reg.tool.*`+OCAP gating. |

### 2.3 hKask deletes (redundant; jobs move to zed-kask)

`hkask-inference` (router/providers/config — keep only the `InferencePort` *trait* in `hkask-types`), `hkask-acp` (no cross-process), `hkask-repl`, `hkask-services-chat`, `hkask-communication`, `mcp-servers/hkask-mcp-communication`, the daemon/`kask serve`, Matrix sidecar + all Matrix refs, cloud/Hetzner/K3s deploy, `hkask-api` chat/chat_ws, **hKask identity layer (WebID-as-separate-identity, OAuth, `HumanUser` Admin/Member roles, invite flow — identity is the Zed account; §0)**, backward-compat shims. `hkask-cli` → slim to backup/wallet/repair (local single-user; no group admin). Deletion-test candidates (decide T0.5): `hkask-condenser`, `hkask-git-cas`, `hkask-services-*`. **Decided — deleted:** `hkask-mcp-filesystem` (zed provides fs tools; §2.4).

---

### 2.4 MCP load set (12 loaded by default)

Of the original 16 MCP servers, the **default load set is 12** (verified against `BUILTIN_SERVERS` in `crates/hkask-mcp-server/src/lib.rs`):

| Loaded by default (12) | Kept, not loaded by default (2) | Deleted (2) |
|---|---|---|
| `memory`, `condenser`, `research`, `companies`, `media`, `docproc`, `training`, `replica`, `kata-kanban`, `codegraph`, `scenarios`, `regulation` | **`curator`** — the Curator is a native agent (D2) with direct in-process regulation access; its MCP server (regulatory query/ops: system health, escalation, spec-drift, algedonic history) is redundant for the default flow now that the Curator is addressable as an agent and `regulation` covers span queries. Unload unless an agent needs explicit Curator-ops tools. **`skill`** — exposes skills as a `skill_execute` MCP tool (render Jinja2 → run inference → return). With D1 (zed-kask's `skill_tool` → `ManifestExecutor` natively), execution is no longer via MCP, so the skill MCP server is unloaded; skill *management* (validate/publish) → `kask` CLI/registry. Crate kept pending T0.5 (are management ops still wanted as agent tools?). | **`communication`** (Matrix/TTS → zed-kask voip). **`filesystem`** (zed's agent/repl already provides filesystem tools — redundant). ⚠ sovereignty note: zed's fs tools are not hKask-OCAP-/gas-gated, so fs calls won't emit `reg.tool.*` or consume rJoule — acceptable for local install since the userpod governs hKask tool calls, not zed's native fs tools; revisit if OCAP-scoped fs is required. |

> 12 loaded + 2 kept-unloaded + 2 deleted = 16 original. Earlier "15/16 MCP servers" counts in this plan are superseded by this 12-loaded default. §2.3 still lists `hkask-mcp-filesystem` as a deletion-test candidate — now **decided: deleted** (see here).

---

## 3. The Minimal Divergence Map (exact zed-kask touch points)

Every hKask integration maps to a **named, isolated** change in zed-kask. This is the entire divergence surface (D1–D7 in the table below; **D8–D10** added by review — consolidated in §13.4); everything else tracks upstream.

| # | Divergence | zed-kask crate / file | Change |
|---|---|---|---|
| D1 | Skill execution | `crates/agent_skills` (discovery keeps `SKILL.md` companions) + `crates/agent/src/tools/skill_tool.rs` (`render_skill_envelope`, used by both the `skill` tool and slash commands) | Replace body-injection with: resolve the skill's hKask `manifest.yaml`+templates and run the compiled-in `ManifestExecutor` cascade (PDCA + gas/rjoule + OCAP); return structured result. `SKILL.md` stays the **discovery-only** catalog entry (frontmatter). |
| D2 | Curator agent | `crates/agent/src/agent.rs` + `native_agent_server.rs` + `crates/agent_servers` | Register the Curator as a native in-process agent (singleton); route Curator turns to the in-process `CuratorAgent`. ACP variant optional. |
| D3 | hKask tools in-process | new workspace members (path-deps to `Clones/hKask` keep-crates) + `crates/context_server/src/client.rs` + `transport/` | Add an **in-process transport** alongside `StdioTransport`; the 12 default-load hKask tools (§2.4) register as in-process context servers; emit `reg.*` directly into the ledger. ⚠ **R4 (§10):** the MCP servers reach storage/regulation/memory via `DaemonClient` over a daemon Unix socket — dissolving the daemon is NOT a transport swap; refactor the servers to take **direct in-process handles** (the daemon owned storage; ownership moves in-process). |
| D4 | Guard layer | `crates/language_model_core`/`language_model` (the streaming `LanguageModel` trait) | ⚠ **R2 (§10):** `GuardedInferencePort` is typed to hKask's *non-streaming* `InferencePort`, not zed-kask's streaming `LanguageModel` — cannot wrap directly. Apply the guard via an adapter: an `InferencePort`-over-`LanguageModel` adapter (collect stream→`InferenceResult`) with the guard wrapping it, OR a zed-kask `LanguageModel` decorator calling `scan_input`/`scan_output` as pure fns (keeps hKask↛zed-kask). Guard cascade+Curator fully; direct-chat streaming needs a buffer/incremental decision (R3). |
| D5 | Sovereignty keys | `crates/credentials_provider` / `zed_credentials_provider` | Store hKask sovereignty keys (OCAP signing, DB passphrase, internal secrets) **via the `SecretsPort` adapter (D9b)** over `CredentialsProvider` (kask namespace) — keeps hKask↛zed-kask. |
| D6 | Thread → memory | `crates/agent/src/thread.rs` / `thread_store.rs` | Hook thread completion → hKask memory ingestion (episodic + semantic). |
| D7 | **App-identity separation** (§7) | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`/`uninstall.sh`/`bundle-linux` | Rename the **local footprint** (APP_NAME, app_identifier, app_id, display_name, single-instance port, remote-server dirs, binary) so zed-kask coexists with an upstream zed install; **keep** the shared `*.zed.dev` account/collab endpoints so the user logs into their existing Zed account. |

**Discipline:** D1–D6 are the *only* edits to zed-kask. Any hKask behavior that would require touching other Zed crates is a smell — push the logic into an hKask crate behind one of these seams instead.

---

## 4. Decisive Reasoning (condensed)

- **Pragmatic Semantics:** the fork re-admits the strong claim "change Zed's skill execution." Corrected frame: zed-kask = host + generic infra (upstream-identical); hKask = compiled-in unique crates. One process.
- **Falsifiability:** "embed the ManifestExecutor in Zed" (E2) was *falsified* under "no extension hook / two runtimes." The fork **dissolves the P5.1/OCAP falsifiers** (one process ⇒ one registry ⇒ P5.1 intact; cascade runs in-process ⇒ no OCAP/gas escape). E2 is the corroborated, most-minimal realization. The Curator counterfactual (*do(not in-process Curator)*) still holds — and is now trivially satisfied (one process). ⚠ **Correction (R1, §10):** one process still hosts **two async executors** — GPUI and tokio — bridged via zed-kask's `gpui_tokio` crate. "One process" ≠ "one runtime"; the registry/OCAP conclusions hold regardless, but the executor bridge is real work, not free.
- **Pragmatic Cybernetics:** regulation reads `reg.*` spans + ledger, never the UI ⇒ surface-agnostic ⇒ preserved. In-process tools emit spans directly (fidelity improves). **Guard coverage improves:** the guard layer reaches zed-kask's *direct-chat* inference, which the old daemon model couldn't — strengthening P3.1.
- **Essentialist (crate-level deletion test):** the daemon, ACP, inference router, provider keystore, MCP-stdio, chat/REPL/Matrix all **vanish**; complexity does not reappear (the ManifestExecutor's own loop is skill-execution, not chat surface; the guard moves into zed-kask's path).
- **Essentialist (fork-level):** divergence is itself minimized to D1–D6 so upstream merges stay cheap — the cost of a fork is the maintenance of divergence, so the divergence surface must be minimal and localized.
- **Skill-Router top-5:** `essentialist` (0.92), `improve-codebase-architecture` (0.88), `pragmatic-cybernetics` (0.86), `deep-module` (0.80), `falsifiability` (0.78).

---

## 5. "What Else Are We Forgetting?" (grok findings)

| # | Item | Consequence |
|---|---|---|
| F1 | TTS/voice in `hkask-mcp-communication` + `TranscriptViewer` audio | → zed-kask voip |
| F2 | Curator Matrix posting via the communication MCP (`loop_body.rs` L901) | → post to a zed-kask Curator thread (in-process) |
| F3 | 7R7 passive listener (Matrix rooms → `reg.*`) | → zed-kask thread-watcher background task |
| F4 | Onboarding creates Matrix creds + userpod | → zed-kask first-launch: user signs into their **Zed account**, the local UserPod is bound to it, UserPod+Curator registered, no Matrix, no OAuth/invite |
| F5 | Federation CRDT transport depends on Matrix | → defer for local MVP; intra-process A2A already in-process |
| F6 | `hkask-api` HTTP server (chat, chat_ws, episodic, consolidation, sovereignty, admin) | deletion-test; keep sovereignty/consolidation/admin only if no in-process path |
| F7 | Model providers — zed-kask owns router; hKask keeps guard + `InferencePort` trait | resolved by the fork (D4) |
| F8 | `kask` CLI subcommands | delete matrix/deploy/serve; keep wallet/repair/admin (thin) |
| F9 | Backward-compat shims (pod-kind alias, `persona_yaml` two-source, pre-v0.31 migration, `kask tui -f`) | delete |
| F10 | Double-gate — zed-kask `tool_permissions` (UI pre-filter) × hKask `GovernedTool` (OCAP+gas, final) | define fail-fast → Curator escalation |
| F11 | Always-on Curator — no daemon ⇒ Curator runs only while zed-kask runs | acceptable for local MVP; background/federation deferred |
| F12 | `hkask-mcp-filesystem` overlaps zed-kask file access | deletion-test |

---

## 6. The Plan (phased; edits live in `Clones/zed-kask`; no backward compat; build-then-delete)

> High-risk = the skill-execution change (D1). Foundational = the crate boundary + guarded inference seam. The fork's upstream-sync is an ongoing phase (Phase 7).

### Phase 0 — Decisions (no code)
- **T0.1** ADR: *zed-kask minimal-divergence fork; hKask = compiled-in curator+sovereignty+tools; no backward compat.* XS.
- **T0.2** ADR: *Skill execution = compiled-in `ManifestExecutor` (E2, single runtime); guard = layer in zed-kask inference path.* XS.
- **T0.3** ADR: *Curator = native in-process agent (singleton, in-process `CuratorHandle`); ACP optional.* XS.
- **T0.4** ADR: *Matrix + communication MCP removed; comms/voip/CRDT via zed-kask; federation deferred.* XS.
- **T0.5** Deletion-test verdicts for §2.3 candidates. XS.
- **T0.6** Migration to full merge (§14): move hKask keep-crates + skills + scripts + docs into zed-kask `kask/` namespace; merge hKask `[workspace.dependencies]` into zed-kask's `[workspace.dependencies]`; add `kask/crates/*` + `kask/mcp-servers/*` as workspace members; archive `mdz-axo/hKask`; decide history preservation (git filter-repo vs clean copy). M.
- **Checkpoint 0:** ADRs + verdicts merged.

### Phase 1 — The crate boundary + guarded inference seam (in zed-kask)

> **Parallel sub-track (D7, §7.5):** T-A1…T-A8 (app-identity separation) run in this phase too — pure fork-renaming with **no hKask dependency**, so they can proceed independently of the crate-boundary work and isolate zed-kask's footprint from the start.

- **T1.1** Add hKask keep-crates (§2.2) as workspace path-deps in `Clones/zed-kask/Cargo.toml`; get them compiling against zed-kask types at the seams. M.
- **T1.2 (D4)** Find the exact `LanguageModel` provider seam in `crates/language_model_core`/`language_model`; implement zed-kask's model behind `hkask_types::InferencePort` and wrap with `hkask_guard::GuardedInferencePort`. AC: a guarded inference call works in-process; `reg.inference` span emitted; **all** inference (chat+cascade+Curator) guarded. M.
- **T1.3 (D5)** Trim `hkask-keystore` to sovereignty crypto; store keys via `crates/credentials_provider`. S.
- **Checkpoint 1:** hKask unique crates compile inside zed-kask; inference guarded in-process.

### Phase 2 — Skill execution (D1, the biggest)
- **T2.1a** In `crates/agent_skills`: keep `SKILL.md` frontmatter as discovery-only catalog metadata; add resolution from a skill to its hKask `manifest.yaml`+templates (registry source of truth). S.
- **T2.1b** In `crates/agent/src/tools/skill_tool.rs`: replace `render_skill_envelope()` body-injection with a call to the compiled-in `ManifestExecutor` cascade (WordAct/FlowDef/KnowAct/RenderAct + PDCA + gas/rjoule + OCAP). Verify the 50KB catalog budget empirically. L.
- **T2.1c** Wire `reg.skill.*` span emission + OCAP/gas gating into the new path. S.
- **T2.2** End-to-end: `/grill-me` in a zed-kask thread runs the KnowAct cascade, returns the assessment, <10s, `reg.skill.activate`+`reg.skill.*` present. S.
- **Checkpoint 2:** skills execute via the hKask cascade, single source of truth, in-process.

### Phase 3 — Agents + tools in-process (D2, D3)
- **T3.1 (D2)** Register the **UserPod** as a native in-process zed-kask agent (`crates/agent/src/agent.rs`/`native_agent_server.rs`); chat turn → guarded inference + OCAP. M.
- **T3.2 (D2)** Register the **Curator** as a native in-process agent (singleton; `CuratorHandle` mpsc in-process; addressable in the Agent Panel). M.
- **T3.3 (D3)** Add an **in-process transport** in `crates/context_server/src/client.rs`+`transport/` alongside `StdioTransport`; host the 12 default-load hKask tools in-process (§2.4); `reg.tool.*` + OCAP-gated. AC: a tool call runs in-process; span present; `VarietyTracker` shows tool domains. M.
- **T3.4 (F10)** Double-gate reconciliation: zed-kask approval = UI pre-filter; `GovernedTool` = final gate; fail-fast → Curator escalation. S.
- **Checkpoint 3:** UserPod + Curator selectable; tools callable in-process with full regulation observability.

### Phase 4 — Thread → memory + thread watcher (D6)
- **T4.1 (D6)** Thread→memory ingestion: parse zed-kask thread transcripts into episodic h_mems (extend the existing ACP per-turn encoding to full transcripts). M.
- **T4.2** Curator threads → Curator episodic + semantic publish (P11). S.
- **T4.3 (F3)** zed-kask thread-watcher (replaces 7R7): background task observes threads, emits `reg.*` for the conversation surface. S.
- **Checkpoint 4:** zed-kask threads become memory; conversation surface observed.

### Phase 5 — Eager deletion from hKask (build-then-delete; depends on 2,3,4)
- **T5.1** Delete `hkask-inference` (keep `InferencePort` trait). M.
- **T5.2** Delete `hkask-acp` + daemon/`kask serve` (**gated on T3.0** — MCP servers must be off `DaemonClient` first, §10.3); Curator+regulation = zed-kask background tasks. M.
- **T5.3** Delete the **entire** `hkask-repl`/tui (chat + `mcp_scoped` + transcript/voice — `mcp_scoped` is reimplemented natively as the KaskPanel, §12.4) + `hkask-services-chat` + `hkask-cli` chat/tui/transcript_viewer + `kask tui`/`matrix`/`deploy`/`serve`/`doctor`-providers. M.
- **T5.4** Delete `hkask-communication` + `mcp-servers/hkask-mcp-communication`; re-point Curator posting → zed-kask thread (F2); TTS → zed-kask voip (F1); drop from `BUILTIN_SERVERS`+gas table. M.
- **T5.5** Delete Matrix sidecar + `hkask-api` Matrix refs + cloud/Hetzner `matrix_url` + `deploy/k8s/conduit`. M.
- **T5.6** Deletion-test `hkask-api` routes (F6); keep sovereignty/consolidation/admin only if no in-process path. S.
- **T5.7** Delete backward-compat shims (F9) + per T0.5 verdicts (condenser/git-cas/services-*); **filesystem-MCP already decided deleted** (§2.4). M.
- **T5.8** Trim `hkask-cli` to backup/wallet/repair (local single-user; no group admin/roles/invite). S.
- **Checkpoint 5:** minimal hKask; zed-kask owns all generic infra; CI green.

### Phase 6 — Local install (no daemon)
- **T6.1** zed-kask first-launch onboarding: user signs into their **Zed account** (required — gates the Zed-based features: comms, collab, voip); create the single local UserPod bound to that account, write `agent.yaml`, register UserPod+Curator, no Matrix, no OAuth/invite. AC: fresh-machine install → sign in → both agents in the Panel <5 min. M.
- **T6.2** Verify sovereignty invariants (P1/P4/P11/P12): per-pod SQLCipher, OCAP gating, WebID, consent. S.
- **Checkpoint 6:** end-to-end local install verified on a clean machine.

### Phase 7 — Upstream sync (ongoing)
- **T7.1** Regular `git fetch upstream && git merge upstream/main` in `Clones/zed-kask`; resolve conflicts only in D1–D6 crates; run hKask integration tests + zed-kask build after each sync. Ongoing.
- **T7.2** Keep a `DIVERGENCE.md` in `Clones/zed-kask` listing D1–D6 + the hKask workspace members, so every sync knows exactly what's ours vs upstream's. S.

### Quality-gate (task-breakdown self-evaluation)
- Red flag: **T2.1b is L** (touches Zed's skill path + wires the cascade) — split further if it exceeds one focused session.
- Bias: deletion optimism is real — the guard-seam (T1.2) and 50KB budget (T2.1b) are the genuine unknowns.
- Parallelism: Phase 1 → Phase 2 → (3 ∥ 4) → Phase 5 → Phase 6; Phase 7 ongoing throughout.

---

## 7. App-Identity Separation (zed-kask ↔ zed coexistence)

**Principle (deep-module):** separate the **local filesystem footprint** so `zed-kask` and an upstream `zed` install coexist on the same machine without conflict, while **sharing the Zed account** — the user logs into their existing Zed account and uses zed-kask *as Zed*, with the minimal kask enhancements. Two deep modules own the footprint; a few hardcoded, non-derived points need separate renames (bug-hunt findings).

### 7.1 The two deep modules (single knobs)

| Module | Knob | Today | zed-kask | What it renames |
|---|---|---|---|---|
| `crates/paths/src/paths.rs` | `APP_NAME: &str` (+ derived `APP_NAME_LOWERCASE`) | `"Zed"` | `"Zed-Kask"` / `"zed-kask"` | config/data/state/temp/logs dirs on all OSes; `Zed-Kask.log`; db/extensions/themes/snippets/prompts/settings/keymap/AGENTS.md; macOS `~/Library/Application Support/Zed-Kask` + `~/Library/Logs/Zed-Kask` + `~/.local/state/Zed-Kask`; Linux `$XDG_*_HOME/zed-kask`; Windows `%APPDATA%\Zed-Kask` + `%LOCALAPPDATA%\Zed-Kask`. **The file itself comments: "Forks should change this to avoid colliding with Zed's user data."** |
| `crates/release_channel/src/lib.rs` | `app_identifier()` / `app_id()` / `display_name()` | `"Zed-Editor-Stable"` / `"dev.zed.Zed-Stable"` / `"Zed"` | `"Zed-Kask-Editor"` / `"dev.zed-kask.Zed-Kask"` / `"Zed-Kask"` | Windows single-instance mutex `{id}-Instance-Mutex` + named pipe `\\.\pipe\{id}-Named-Pipe`; macOS bundle id (`~/Library/Preferences/dev.zed-kask.Zed-Kask.plist`, LaunchServices identity); Dock/menu display name. |

**Deletion test:** inlining `APP_NAME`/`app_identifier` at every call site would reappear the platform-path logic everywhere → the modules earn their keep; change the constants, the whole footprint renames. ≤3 public items each, every consumer reads them, nothing writes back → **deep**.

### 7.2 Non-derived collision points (bug-hunt — APP_NAME alone does NOT fix these)

| # | Point | File | Risk | Fix |
|---|---|---|---|---|
| C1 | **macOS single-instance TCP port** | `crates/zed/src/zed/mac_only_instance.rs` `address()` | Port = `43737 + (channel×100) + uid` — keyed on **release channel + uid only**, NOT on APP_NAME. zed-kask and zed-stable (same channel, same uid) → **same port → the second app sees the "Zed Editor Stable Instance Running" handshake and silently exits.** | Distinct port block (fixed offset, e.g. `+500`, or a `Kask` release-channel arm) + change `instance_handshake()` to "Zed-Kask …". |
| C2 | **Remote SSH/WSL server dirs** | `crates/paths/src/paths.rs` `remote_server_dir_relative()`/`remote_wsl_server_dir_relative()` + `crates/util/src/shell.rs` | Hardcoded `.zed_server` / `.zed_wsl_server` on the REMOTE host. SSH to a host where zed also runs → collision + version mismatch. | `.zed-kask_server` / `.zed-kask_wsl_server` (2 path fns + shell.rs). |
| C3 | **Binary name** | `crates/zed/Cargo.toml` `[[bin]] name = "zed"` | Same `zed` binary on PATH → shadows/conflicts. | `[[bin]] name = "zed-kask"` (keep package name `zed` to minimize diff). |
| C4 | **macOS bundle display names** | `crates/zed/Cargo.toml` L281–305 (`"Zed Dev"`…`"Zed"`) | Indistinguishable from zed in Dock/Launchpad. | `"Zed-Kask …"` (via `display_name()`). |
| C5 | **URL scheme `zed://`** | `crates/zed/src/zed/open_listener.rs` + `assets/settings/default.json` `$schema` + `zed://skill` share links | Internal `zed://` prefixes are just strings (safe); the OS-level handler is bundle-id-registered (macOS: only one app owns `zed://`). | **Decision:** keep `zed://` internally (minimal divergence — don't touch open_listener) and accept the macOS handler conflict, OR rename to `zed-kask://` (full isolation, but diverges `default.json` `$schema` + skill-share links). Lean: keep `zed://`; revisit. |

### 7.3 RENAME vs KEEP (the account-sharing constraint)

| RENAME (local footprint — isolated) | KEEP (shared — user logs into their Zed account) |
|---|---|
| `APP_NAME`, `app_identifier`, `app_id`, `display_name` | `default.json` `"server_url": "https://zed.dev"` (collab) |
| config/data/state/cache/logs/db/extensions dirs | `"provider": "zed.dev"`, `"zed.dev": {}` (LLM provider/account) |
| `Zed-Kask.log`, settings/keymap/AGENTS.md paths | `cloud_api_client` `cloud.zed.dev` (account API) |
| Windows mutex/pipe, macOS bundle id + plist | `release_channel::ZED_DOCS_URL` `https://zed.dev/docs` (docs) |
| macOS single-instance port + handshake | `staging-collab.zed.dev` / `collab.zed.dev` (collab relay) |
| `.zed-kask_server` / `.zed-kask_wsl_server` remote dirs | telemetry endpoint (zed.dev) — optional disable |
| binary `zed-kask` | extension marketplace URL (shared; extensions re-installed in the isolated dir) |

**Key invariant:** account/auth/collab traffic goes to `*.zed.dev` keyed on the user's Zed credentials, NOT on bundle id or APP_NAME. Renaming the local identity does **not** affect login — the user signs into the same Zed account and zed-kask behaves as Zed with a separate local footprint.

### 7.4 grill-me challenges (what breaks?)

- **Does renaming the bundle id break Zed account login?** No — auth is to `cloud.zed.dev` keyed on credentials, not bundle id. (Verified: account endpoints live in `default.json`/`cloud_api_client`, independent of `app_id`.)
- **Does renaming APP_NAME orphan existing Zed settings?** It *isolates* them — zed-kask starts fresh (re-onboard); the user's zed settings stay untouched in the old `zed` dirs. Intended.
- **C1 is the silent killer:** an APP_NAME rename does NOT prevent the macOS single-instance collision — verified `address()` keys on channel+uid. Must fix C1 explicitly or zed-kask silently exits whenever zed is running.
- **Extensions:** isolated dir = re-install. Minor cost; benefit = no version conflicts with zed's extensions.
- **Telemetry:** distinct install id (renamed data_dir) → zed-kask reports under a different install id to the same endpoint. Acceptable, or disable.

### 7.5 Tasks (foundational — run in Phase 1, parallel with the crate boundary; pure fork-renaming, no hKask dependency)

- **T-A1** `crates/paths/src/paths.rs`: `APP_NAME = "Zed-Kask"`. AC: `config_dir()`/`data_dir()`/`logs_dir()`/`log_file()` resolve under `zed-kask`/`Zed-Kask` on all OSes. S.
- **T-A2** `crates/release_channel/src/lib.rs`: `app_identifier()` → `Zed-Kask-Editor`; `app_id()` → `dev.zed-kask.Zed-Kask`; `display_name()` Stable → `Zed-Kask`. AC: Windows mutex/pipe + macOS bundle id distinct. S.
- **T-A3 (C1)** `crates/zed/src/zed/mac_only_instance.rs`: distinct port block (offset or `Kask` channel) + `instance_handshake()` "Zed-Kask …". AC: zed-kask runs while zed-stable is running. S.
- **T-A4 (C2)** `crates/paths/src/paths.rs` + `crates/util/src/shell.rs`: `.zed-kask_server` / `.zed-kask_wsl_server`. S.
- **T-A5 (C3/C4)** `crates/zed/Cargo.toml`: `[[bin]] name = "zed-kask"`; macOS display names `Zed-Kask …`. S.
- **T-A6** `script/install.sh`/`uninstall.sh`/`bundle-linux`: `appid`/`app_id` → `dev.zed-kask.Zed-Kask`. S.
- **T-A7** Decision record (C5): keep `zed://` vs rename to `zed-kask://`. XS.
- **T-A8** Verify: with both `zed` and `zed-kask` installed, both launch independently, separate settings, **same Zed account login**. AC: both run concurrently; account works in both. S.

## 8. Open Questions (honestly carried)

> Numbered 1–26 here; 27–28 in §13.6 (27 DONE), 29–32 in §14.6.

1. **ACP vs native** — native in-process recommended (minimal); keep ACP only for external-agent interop (T0.3).
2. **`hkask-keystore` storage backend** — share `crates/credentials_provider`, or thin hKask keychain wrapper? (T1.3).
3. **Exact `LanguageModel` provider seam** for the guard layer — verified in T1.2.
4. **50KB catalog budget** — empirical (T2.1b).
5. **Condenser / git-cas / services-\* / filesystem-MCP** — deletion-test verdicts (T0.5).
6. **Always-on Curator** — runs only while zed-kask runs; background/federation deferred (F11). Acceptable for local MVP?
7. **Double-gate** (F10) — fail-fast behavior (T3.4).
8. **`hkask-api` fate** (F6) — keep sovereignty/consolidation/admin or dissolve into in-process paths? (T5.6).
9. **hKask wiring** — path-deps (dev) vs git-submodule/vendor (shipping) (T0.6).
10. **URL scheme (C5)** — keep `zed://` (minimal divergence, macOS handler conflict) or rename `zed-kask://` (full isolation, diverges settings `$schema` + skill-share links) (T-A7).
11. **macOS single-instance port (C1)** — fixed offset vs a new `Kask` release-channel arm (T-A3).
12. **Extensions** — isolated dir (re-install) vs sharing zed's extensions dir (T-A1 decision).
13. **Telemetry** — distinct install id to shared endpoint vs disable for zed-kask.
14. **Guard strategy for direct-chat streaming (R3)** — buffer (kills UX) vs incremental scan vs cascade-only guarding.
15. **`InferencePort`-adapter vs `LanguageModel`-decorator for the guard (R2)** — which keeps dependency direction hKask↛zed-kask.
16. **DaemonClient→direct-handles refactor scope (R4)** — which MCP servers need storage/regulation/memory handles; is a shared in-process "core" owner needed?
17. **Curator agent-turn adapter (R8)** — zed-kask coding-agent thread vs Curator regulation-mediator interface.
18. **CI hermeticity (R10)** — git submodule/vendor from day one for any non-local build; path-dep only for local dev.
19. **Skill MCP management tools** — with `hkask-mcp-skill` unloaded, are skill validate/publish still needed as agent tools, or CLI-only? (T0.5).
20. **Curator MCP server load policy** — confirm `hkask-mcp-curator` stays unloaded by default (Curator-as-agent + `regulation` MCP cover it); load on demand only. (§2.4)
21. **Initial data-service set** (D9) — EODHD + FMP confirmed (used by `hkask-mcp-companies`, `hkask-wallet`); which others (polygon, alpha-vantage, tiingo, FRED) ship in the `kask.data_services` section at MVP?
22. **`SecretsPort` trait location** (D9b, R9) — define in `hkask-types` (keeps hKask↛zed-kask) and implement on the zed-kask side over `CredentialsProvider`?
23. **Config-migration precedence** (T6.3) — settings.json > keychain > env-var fallback; one-time import vs continuous env fallback?
24. **Kask panel implementation** (D10) — confirm native GPUI (option B) vs ratatui-in-terminal (option A) for MVP.
25. **Kask panel dock position** — right or bottom; auto-launch on startup?
26. **Kask panel command scope** — direct `:tool args` + scoped inference (read+write via OCAP); any read-only restrictions per server?

---

## 9. Self-Critique-Revision (convergence)

- **Fork grounding strengthens the proposal:** the divergence is now mapped to *exact* zed-kask crates (`agent_skills`, `agent/tools/skill_tool.rs`, `agent`/`agent_servers`, `context_server/client.rs`+`transport/`, `language_model*`, `credentials_provider`), so the "minimal divergence" claim is verifiable, not aspirational.
- **Over-caution corrected:** the fork dissolves the E2 P5/OCAP falsifiers and the ACP/daemon/MCP-stdio seams — the architecture is more minimal than the prior daemon version, and the guard-coverage gap *closes*.
- **New risk honestly added:** upstream-sync conflict cost (Phase 7) is the price of the fork; mitigated by isolating divergence to D1–D6 + a `DIVERGENCE.md`.
- **Calibration:** 0.80 on the compiled-in architecture; 0.6 on T2.1b magnitude; 0.6 on the 50KB budget; 0.7 on low-conflict upstream merges (depends on keeping D1–D6 tightly localized). Honest.
- **Convergence:** quality improved; no criterion regressed; residual is genuine irreducible uncertainty (always-on Curator, keystore backend, 50KB budget, sync-conflict rate), correctly reported rather than iterated past.

---

## 10. Review Findings (grill-me + diagnose + bug-hunt, evidence-based)

Evidence: hKask `InferencePort` is **non-streaming** (`fn generate(...) -> Pin<Box<dyn Future<Output=Result<InferenceResult,InferenceError>> + Send>>`, `hkask-types/src/ports/inference_port.rs`); `GuardedInferencePort` implements `InferencePort` and wraps an `InferencePort` (`hkask-guard/src/guarded_inference.rs`); `ManifestExecutor` holds `Arc<dyn InferencePort>` + `Arc<dyn ToolPort>` (`hkask-templates/src/executor.rs`). zed-kask's seam is the **streaming** `LanguageModel` trait (`stream_completion*`, `crates/language_model/src/language_model.rs`); its `context_server` client runs on GPUI async (`cx.spawn`/`async_channel`); zed-kask provides a `gpui_tokio` bridge.

| ID | Skill | IS (code) vs OUGHT | Sev | Fix |
|---|---|---|---|---|
| R1 | grill-me | IS: plan said "one process ⇒ one runtime" — **false**; zed-kask=GPUI, hKask=tokio. OUGHT: bridge via `gpui_tokio`; drive hKask tokio tasks (Curator/regulation/MCP/executor) on a bridged runtime. | High | D8; T1.4 |
| R2 | bug-hunt (integration) | IS: `GuardedInferencePort` is typed to non-streaming `InferencePort`; zed-kask `LanguageModel` streams — cannot "wrap" directly. OUGHT: zed-kask-side adapter (`InferencePort` over `LanguageModel`, collect→`InferenceResult`) OR a `LanguageModel` decorator calling `scan_input`/`scan_output` as pure fns (keeps hKask↛zed-kask). | High | D4/D8; T1.4 |
| R3 | diagnose | IS: guarding the direct-chat stream means `scan_output` buffers (kills streaming UX) or scans incrementally. OUGHT: guard cascade+Curator fully (non-streaming, cheap); direct-chat = buffer-threshold or incremental; the "coverage improves" claim has a hidden cost. | Med | T2.0b |
| R4 | bug-hunt (structural) | IS: the 15 MCP servers reach storage/regulation/memory via `DaemonClient` over a daemon Unix socket (`hkask-mcp-server/src/daemon/`). "Dissolve daemon + host in-process" is NOT a transport swap. OUGHT: refactor servers to **direct in-process handles** (daemon owned storage; ownership moves in-process to a shared core). | High | D3; T3.0 |
| R5 | bug-hunt (interface) | IS: `ManifestExecutor::new(inference: Arc<dyn InferencePort>, tools: Arc<dyn ToolPort>)`. OUGHT: zed-kask-side adapters — `InferencePort` over `LanguageModel` (R2) + `ToolPort` over the in-process tool registry (D3). | High | D8; T2.0 |
| R6 | diagnose (flow) | IS: Phase 2 (D1) runs before Phase 3 (D3), but FlowDef `execute` steps need the ToolPort→in-process tools. OUGHT: validate Phase 2 with **KnowAct-only** skills (grill-me) first; gate full FlowDef execution on D3. | Med | §10.3 |
| R7 | diagnose (flow) | IS: T5.2 deletes the daemon; MCP servers still need `DaemonClient` removed first (R4). OUGHT: the R4 refactor (T3.0) must precede T5.2 or the servers are orphaned. | Med | §10.3 |
| R8 | bug-hunt (interface) | IS: zed-kask native agents are coding-agent tool-threads (`native_agent_server.rs`); the Curator is a regulation mediator (tokio/mpsc). OUGHT: an adapter from zed-kask's agent-turn interface to the Curator's turn interface (D2). | Med | D2; T3.2 |
| R9 | bug-hunt (dependency dir) | IS: sovereignty crypto stays in hKask; provider keys → zed-kask `credentials_provider`. If hKask reuses zed-kask's keychain, that's hKask→zed-kask (inversion). OUGHT: hKask keeps its own keyring for sovereignty keys; only provider keys live in zed-kask. | Low–Med | T1.3 |
| R10 | diagnose (config/CI) | IS: `../../Clones/hKask/...` path-deps break for other cloners/CI. OUGHT: git submodule/vendor for any shared/CI build; path-dep only for local dev. | Med | T0.6 |
| R11 | idiomatic Rust | IS: the `InferencePort` adapter boxes futures + crosses tokio↔GPUI per call. OUGHT: keep the adapter thin; avoid per-chunk boxing; accept the alloc/dispatch cost. | Low | T1.4 |
| R12 | bug-hunt (interface) | IS: `render_skill_envelope` returns `LanguageModelToolResultContent`; the cascade returns structured output. OUGHT: a renderer from cascade result → agent content shape. | Low | T2.1b |

### 10.1 New divergence seam D8 (the bridge + adapters)
The bridge crate (e.g. zed-kask-side `crates/agent_kask`) is the single place that reconciles the two async worlds (GPUI/tokio via `gpui_tokio`) and the two trait families (`LanguageModel`↔`InferencePort`, zed-kask tool registry↔`ToolPort`); everything else stays upstream-identical.

### 10.2 New / amended tasks
- **T1.4 (R1/R2/D8)** — create the zed-kask-side bridge crate: `gpui_tokio` wiring + `InferencePort`-over-`LanguageModel` adapter (collect stream→`InferenceResult`). M.
- **T2.0 (R5/R6)** — `ToolPort` adapter over the in-process tool registry; gate FlowDef `execute` on D3; validate Phase 2 with KnowAct-only skills first. M.
- **T3.0 (R4/R7)** — refactor MCP servers off `DaemonClient` to direct in-process storage/regulation/memory handles; **prerequisite for T5.2** (daemon deletion). L.
- **T2.0b (R3)** — decide direct-chat guard strategy (buffer vs incremental vs cascade-only). S.

### 10.3 Flow corrections (diagnose)
- **D1 gating:** Phase 2 full FlowDef validation is gated on D3 (Phase 3) ToolPort readiness; KnowAct-only validation (grill-me) proceeds first (R6).
- **T5.2 gating:** daemon deletion is gated on T3.0 (DaemonClient refactor), not merely Phase 3 existence (R7).
- **Phase 4 independence:** thread→memory ingestion must use **in-process memory handles** (R4), not the soon-deleted `hkask-api`/daemon endpoints.

### 10.4 Self-critique on this review
- The earlier "one process ⇒ one runtime" and "wrap with `GuardedInferencePort`" claims were **over-confident**; R1/R2 correct them with evidence. The architecture is still sound (in-process registry + OCAP hold), but the integration is **bridge + adapters**, not a free compile-in.
- The biggest hidden cost is **R4**: the daemon wasn't just a process boundary — it owned the storage/regulation/memory the MCP servers depend on via `DaemonClient`. Losing the daemon means that ownership and the `DaemonClient` contract must be replaced in-process (T3.0, L-scope).
- Calibration revised: 0.80 → **0.70** on the compiled-in architecture (the bridge/adapters are non-trivial); 0.6 on T2.1b/T3.0 magnitude (T3.0 is now also L). Honest.

---

## 11. Kask Settings & Credentials (data-service keys, minimal divergence)

**Goal:** load API keys for data services (EODHD, FMP, and other kask data services) and all kask-unique config via a **kask settings section** in zed-kask's settings.json + a **kask credentials namespace** in the keystore — leaving core zed settings/keystore code untouched.

### 11.1 Evidence
- zed-kask stores provider API keys via the `CredentialsProvider` trait (`read_credentials`/`write_credentials`/`delete_credentials` keyed by URL → OS keychain); `language_models` providers use `api_key_state` + `credentials_provider` (`crates/credentials_provider`, `crates/language_models/src/provider/open_router.rs`). **Secrets live in the keychain, NOT settings.json.**
- The settings UI is `Vec<SettingsPage>` built in `crates/settings_ui/src/page_data.rs::settings_data()`; pages live in `crates/settings_ui/src/pages/` (e.g. `mcp_servers_page.rs`, `llm_providers_page.rs`).
- hKask today reads data-service keys from **env vars** (`HKASK_FMP_API_KEY`, `HKASK_EODHD_API_KEY`) — in `hkask-mcp-companies` (`ctx.get`) and `hkask-wallet/price_feed.rs` (`std::env::var`). They are NOT in hKask's keychain (which holds DB passphrase/OCAP signing only).

### 11.2 Design (two additive seams)

**D9a — kask settings section** (`"kask": {...}` in settings.json + a settings struct). A new top-level section, isolated from core zed settings. Holds kask-unique, **non-secret** config:
- `kask.data_services.{eodhd,fmp,polygon,alpha_vantage,tiingo,fred,...}` — enabled toggles + per-service config (endpoints, tiers). The **secret API key is NOT here** — it is in the keychain (D9b); settings holds only the reference/toggle.
- `kask.mcp.load_default` + `overrides` — the 12-loaded-by-default set (§2.4) + per-server toggles (curator/skill off by default; filesystem/communication absent).
- `kask.curator` — always-on toggle, regulation set-points (variety window, algedonic thresholds).
- `kask.sovereignty.pod` — data-dir override, consent defaults.
- `kask.guard` — direct-chat guard strategy (R3: buffer / incremental / cascade-only).
- `kask.memory` — consolidation cadence, confidence floor.
Registered with zed's settings system so it appears in the `zed://schemas/settings` schema. **Minimal divergence:** one new settings struct + registration; core zed settings structs untouched.

**D9b — kask credentials namespace** (via the existing `CredentialsProvider`). Data-service API keys stored in the OS keychain under kask-namespaced URLs (e.g. `kask://credentials/eodhd`, `kask://credentials/fmp`), alongside zed's provider keys (which use their own URLs). The kask MCP servers (companies/scenarios/wallet) read keys via `CredentialsProvider` at runtime — **replacing the env-var approach** (`HKASK_*`). This folds into the T3.0 in-process refactor: MCP servers take a credentials handle, not env vars. The sovereignty keys (D5: DB passphrase, OCAP signing) also move here (kask namespace), so the trimmed `hkask-keystore` becomes a thin crypto-derivation layer over the shared `CredentialsProvider`.

### 11.3 Settings UI (additive page)
A new **Kask** page: `crates/settings_ui/src/pages/kask_page.rs` + one entry in `page_data.rs::settings_data()`. Sub-pages mirror the settings section: **Data Services** (per-service enable + key entry → writes to keychain via `CredentialsProvider`), **MCP Servers** (the 12 + load toggles), **Curator**, **Sovereignty/Pod**, **Guard/Regulation**, **Memory**. Touches `page_data.rs` minimally (one `SettingsPage` push) — core zed pages untouched.

### 11.4 Configuration translation / migration
Existing hKask config → kask settings + keychain, on first launch (and a `kask import-config` command):
- env `HKASK_FMP_API_KEY` / `HKASK_EODHD_API_KEY` → `CredentialsProvider` entries `kask://credentials/{fmp,eodhd}` + `kask.data_services.{fmp,eodhd}.enabled = true`.
- hKask keychain sovereignty keys (DB passphrase, OCAP signing) → `CredentialsProvider` kask namespace (D5).
- hKask config-file settings (regulation thresholds, consolidation cadence, gas defaults) → `kask.*` settings.json section.
Precedence: explicit settings.json > imported keychain > env-var fallback (during transition) — decision T0.6b.

### 11.5 Tasks
- **T1.5 (D9a)** — define the `KaskSettings` struct + register with zed's settings system; add the `"kask"` JSON-schema section. M.
- **T1.6 (D9b)** — add the kask credentials namespace to `CredentialsProvider` usage; helper to read/write `kask://credentials/<service>`. S.
- **T3.0b (part of T3.0)** — refactor the data-service-consuming MCP servers (companies, scenarios, wallet) off env vars → read keys via `CredentialsProvider` (kask namespace). M.
- **T-s1 (D9 UI)** — `crates/settings_ui/src/pages/kask_page.rs` + register in `page_data.rs::settings_data()`. M.
- **T6.3** — `kask import-config`: migrate env `HKASK_*` + old keychain → kask settings + keychain. S.
- **T-A0 (sovereignty)** — fold trimmed `hkask-keystore` crypto-derivation over the shared `CredentialsProvider` (D5). S.

### 11.6 grill-me / diagnose notes
- **Secrets must NOT be in settings.json** — keys live in the keychain (matches zed's provider-key pattern; verified `api_key_state` uses `credentials_provider`, not settings.json, for the secret). The `kask` settings section holds only toggles/refs.
- **Dependency direction (R9 echo):** `CredentialsProvider` is a zed-kask trait; hKask MCP servers consuming it directly = hKask→zed-kask (inversion). Mitigation: define a thin hKask-side `SecretsPort` trait (in `hkask-types`) that the zed-kask side implements over `CredentialsProvider` — keeps hKask crates independent of zed-kask.
- **Extensions model:** kask data services are configured in the same UI/credentials pattern as zed providers (first-class), not ad-hoc env vars; the 12 MCP servers are compiled-in (not zed extensions), but their key configuration reuses zed's credentials model — minimal divergence.
- **D9 = new divergence seam** (kask settings section + credentials namespace + UI page). Add to the §3 divergence map alongside D1–D8 (the §3 row could not be edited this session due to a matcher quirk on the D7/D3 rows; recorded here instead).
---

## 12. Kask Panel (per-MCP-server one-on-one windows)

**Requirement:** a "Kask" panel in zed-kask where the user can launch a window per kask MCP server and interact with it **one-on-one** to reach the server's **deeper functionality** (direct tool invocation + scoped inference), within the zed-kask app — distinct from the conversational Agent Panel (which drives tools via the agent).

### 12.1 Evidence — hKask already implements this concept
`crates/hkask-repl/src/tui/windows/mcp_scoped.rs` is `McpScopedWindow`: a per-MCP-server pane (Kanban, Companies, Scenarios, …) with two OCAP-gated input paths:
- **Direct tool invocation** (`:tool_name args`) — calls the MCP tool directly via `ToolInvokeBridge`, bypassing the LLM; fast, structured JSON; preserves `DelegationToken` (OCAP).
- **Scoped inference** (natural language) — the LLM acts as intermediary calling only that server's tools.
`McpScopedState` holds the per-window input/log/pending-request state. This is exactly the "one-on-one deeper functionality" the user wants — the only question is how to host it in zed-kask.

### 12.2 Two implementation options
- **(A) ratatui-in-terminal (reuse-fast):** a zed `Panel` hosting a `Terminal` (alacritty PTY) running a slimmed `kask panel` ratatui binary = the existing `McpScopedWindow`/`window_catalog`/`tab`/`status_bar` views. **Cost:** the TUI is a separate process ⇒ needs an in-process view/control socket (retain the daemon listener in zed-kask); keeps a PTY boundary. Reuses the most existing code.
- **(B) native GPUI panel (recommended):** reimplement `McpScopedWindow` as a zed-native `Panel` (`crates/kask_panel`, GPUI) — a server catalog (the 12 loaded servers, §2.4) + a per-server view with direct `:tool args` invocation and scoped-inference input, calling the **in-process MCP tools (T3.0)** and **guarded inference (D8)** directly. **No PTY, no view socket, no retaining the daemon listener.** Lets the entire hKask ratatui TUI be deleted (T5.3 deletes all of `hkask-repl/tui`, incl. `mcp_scoped` — it is reimplemented natively). One new panel crate; reuses the in-process tool/inference seams already built.

**Decision: (B).** More idiomatic zed-native, eliminates the PTY/IPC boundary (and the need to retain the daemon listener), and simplifies deletion. (A) remains a reuse-fast fallback if the GPUI rebuild proves too costly for MVP.

### 12.3 Design (D10 — native GPUI kask panel)
- **zed side:** new `Panel` impl `crates/kask_panel` (implements `pub trait Panel`, `crates/workspace/src/dock.rs`; `DockPosition` right or bottom). Renders: a list of the 12 loaded MCP servers (from the in-process tool registry, §2.4); selecting one opens a per-server sub-view.
- **Per-server sub-view:** (1) the server's tool list (introspected from the in-process MCP server) + a `:tool_name args` direct-invocation input → calls the in-process tool through the OCAP-gated path (same `GovernedTool`/gas as the agent; emits `reg.tool.*`); (2) a natural-language scoped-inference input → runs guarded inference (D8) with only that server's tools in scope. Results rendered inline.
- **OCAP:** the panel invokes tools under the userpod's `DelegationToken` exactly as the agent does — direct invocation does NOT bypass OCAP (mirrors the ratatui `ToolInvokeBridge` invariant). Double-gate (F10) applies: panel invokes are still `GovernedTool`-gated.
- **hKask side:** delete the entire ratatui TUI (T5.3) — `mcp_scoped` is reimplemented natively; no slimmed ratatui binary, no view socket. (This **reverses** an earlier ratatui-terminal idea: cleaner.)

### 12.4 Tasks
- **T-s2 (D10 zed)** — `crates/kask_panel`: GPUI `Panel` + server catalog (12 servers from the in-process registry). M.
- **T-s3 (D10 view)** — per-server sub-view: direct `:tool args` invocation (in-process, OCAP-gated) + scoped inference (guarded). Reimplement `McpScopedWindow`'s two input paths natively. M.
- **T-s4** — wire the panel to the in-process tool registry (T3.0) + guarded inference (D8); verify `reg.tool.*`/`reg.inference` spans fire on direct invokes. S.
- **Refine T5.3** — delete the **entire** `hkask-repl/tui` (chat + `mcp_scoped` + transcript/voice); `mcp_scoped` is now native (T-s3). (No view socket, no daemon-listener retention — simpler than option A.)

### 12.5 grill-me / diagnose
- **Does direct invocation bypass sovereignty?** No — it reuses the OCAP-gated `GovernedTool` path (mirrors the ratatui `ToolInvokeBridge` `DelegationToken` invariant); only the LLM is bypassed, not OCAP/gas. Verified against the `mcp_scoped.rs` doc comment.
- **Why not reuse ratatui (A)?** (A) needs a PTY + an in-process view/control socket (retain the daemon listener) for a separate process to reach the in-process runtime — re-introducing an IPC boundary we removed. (B) talks to in-process tools directly, no IPC, and lets us fully delete the ratatui TUI. Trade-off: (B) rebuilds the UI in GPUI; accepted for a cleaner, more minimal result.
- **Variety/regulation:** direct one-on-one invokes still emit `reg.tool.*` and consume gas (T-s4) — the cybernetic loop sees panel activity, so regulation is not bypassed.

### 12.6 GPUI reuse map (option B, grounded in zed's own code)

**Panel trait + registration** (`crates/workspace/src/dock.rs`):
- `impl Panel for KaskPanel` requires: `persistent_name()` → `"KaskPanel"`, `panel_key()`, `position(&self, window, cx) -> DockPosition`, `default_size()`, `min_size()`, `icon() -> Option<IconName>`, `icon_tooltip()`, `toggle_action() -> Box<dyn Action>`, `activation_priority()`, `enabled()`, plus `Render`, `Focusable`, `EventEmitter<PanelEvent>`. **Copy-template: `agent_ui/src/agent_panel.rs:4954` (`impl Panel for AgentPanel`).**
- Register: `cx.new(|cx| KaskPanel::new(...))` then `workspace.add_panel(panel, window, cx)` (`crates/workspace/src/workspace.rs:2554`); add a `ToggleKaskPanel` action (mirror `ToggleFocus`). Dock position/size persist into the **`kask.panel.dock`** settings field (§11/D9a) — not zed's `agent` settings — via `settings::update_settings_file` (mirror AgentPanel `set_position`).

**Visual language — `use ui::prelude::*`** (`crates/ui/src/prelude.rs`); reuse components + theme tokens (NO hardcoded colors/fonts):
- Components: `Button`/`IconButton`, `Label` (`LabelSize`), `Icon`/`IconName`/`IconSize`, `list::List` (server catalog), `TabBar` (open per-server windows as tabs), `context_menu`/`popover_menu` (per-tool menus), `scrollbar`, `data_table` (structured JSON tool results), `chip`/`indicator` (server status dots), `divider`, `callout` (errors), `Tooltip`, `keybinding_hint`.
- Tokens: colors via `cx.theme().colors()` / `cx.theme().status()` (e.g. `Color::Default`, `Color::Created` — `ui/src/styles/color.rs`); sizes via `TextSize`/`LabelSize`/`IconSize`; spacing via `DynamicSpacing`/`px`/`rems`. This **is** zed's visual language — the kask panel inherits light/dark themes for free.

**Input — reuse `editor::Editor`** (mirror `agent_ui/src/message_editor.rs`): a multi-line `Editor` as the `:tool args` / NL input, with a completion provider offering the active server's tools (`:tool_name`) — mirror `PromptCompletionProvider` + `AvailableSkill`/`SlashCommandCompletion`. Submit → direct tool invoke (T3.0, OCAP) or scoped inference (D8).

**Results — reuse `conversation_view` pattern + `data_table`**: scoped-inference output renders like agent messages; direct-tool JSON results render in `data_table` / code blocks (mirror agent tool-result rendering).

**Structure (`crates/kask_panel`):**
- `KaskPanel { servers: [12 from in-process registry, §2.4], active: ServerId, tabs: open windows, input: Entity<Editor>, results: view, tool/inference handles (D3/D8) }`.
- Render: header (`TabBar` of open servers, or `List` catalog when none) + active view (`Editor` input + results). All via `ui` components + theme tokens.
- Dock icon: reuse an existing `IconName`, or add `IconName::Kask` (small addition to `crates/ui/src/icon.rs`).

**Minimal divergence:** one new crate `crates/kask_panel` + one `ToggleKaskPanel` action + one `add_panel` call at workspace init + (optional) one `IconName` + `kask.panel.*` in the settings section (§11). No core zed panel/`ui`/`editor` modifications.

**Reference files to copy from:**
- `agent_ui/src/agent_panel.rs` — Panel impl boilerplate, render structure, dock persistence.
- `agent_ui/src/message_editor.rs` — input `Editor` + completion provider.
- `agent_ui/src/conversation_view/` — message/tool-result rendering.
- `agent_ui/src/context_server_configuration.rs` — MCP server list UI (catalog style).
- `crates/ui/src/components/{list,tab_bar,data_table,context_menu}.rs` — components.
- `crates/ui/src/styles/{color,typography,spacing,units}.rs` — design tokens.

---

## 13. Composition & Connection Surfaces (zoom-out review)

The connection surfaces use established patterns (ports-and-adapters, decorator, composition-root DI, zed `Panel`, zed settings/credentials) — correct. This section names them as **one coherent, minimal composition** so the seams are explicit.

### 13.1 Governing invariant (dependency direction)
**hKask crates NEVER depend on zed-kask; zed-kask depends on hKask crates.** The **single bidirectional seam** is the zed-kask-side **bridge crate** (`crates/kask_bridge` = D8), which depends on both hKask port traits and zed-kask types and implements every adapter. Every other divergence (D1, D2, D3, D6, D9a, D10) *consumes* a port implemented by the bridge; no hKask crate reaches into zed-kask internals. (Reconciles R9/D9b.)

### 13.2 The complete port set (ports-and-adapters)
All zed↔kask connection surfaces are a small set of **port traits** (in `hkask-types` + `hkask-capability`), each implemented by the **bridge crate** over a zed-kask facility:

| Port (hKask side) | Implemented over (zed-kask side) | Used by | D |
|---|---|---|---|
| `InferencePort` (hkask-types; non-streaming) | `LanguageModel` (streaming) via collect→`InferenceResult`; `GuardedInferencePort` wraps it | ManifestExecutor (D1), Curator (D2), kask panel scoped inference (D10) | D4/D8 |
| `ToolPort` (hkask-capability; OCAP+gas) | the in-process MCP tool registry (D3) | ManifestExecutor FlowDef `execute`, kask panel direct invoke (D10) | D3/D8 |
| `SecretsPort` (hkask-types; NEW) | `CredentialsProvider` (kask namespace) | data-service keys (companies/scenarios/wallet) + sovereignty keys (D5) | D5/D9b |
| `CuratorTurnPort` (hkask-types; NEW) | zed native-agent turn → in-process `CuratorAgent` (tokio via bridge) | Curator as native agent (D2) | D2/D8 |
| `MemoryPort` (hkask-types; NEW) | in-process `EpisodicMemory`/`SemanticMemory` handles | thread→memory ingestion (D6) | D6 |

Hexagonal pattern: hKask defines the ports; the bridge crate is the adapter; the composition root wires them. **No hKask crate imports a zed-kask crate.**

### 13.3 Composition root (startup — DI pattern)
zed-kask app startup constructs **one `KaskCore`** (the hKask runtime singleton) and wires the adapters:
1. **Load `KaskSettings` (D9a)** → bind to `KaskCore` construction params (regulation set-points, gas defaults, consolidation cadence, guard strategy, MCP load set = the 12, §2.4). **Settings→config is construction-time, not a runtime port** (config-struct-validated-on-construction).
2. **Construct `KaskCore`:** the single local UserPod (identity bound to the signed-in Zed account — §0; no hKask OAuth/roles), per-pod encrypted storage, Regulation runtime, memory, the singleton Curator (`CuratorHandle` mpsc in-process), the 12 MCP servers (with **direct storage/regulation/memory handles** — R4), the `ManifestExecutor`.
3. **Build the bridge:** `InferencePort`-over-`LanguageModel` (+guard), `ToolPort`-over-tool-registry, `SecretsPort`-over-`CredentialsProvider`, `CuratorTurnPort`, `MemoryPort`; inject into `ManifestExecutor`/Curator/MCP servers/kask panel.
4. **Spawn** the regulation + Curator metacognition tokio loops on the `gpui_tokio` runtime (R1) — the loop driver.
5. **Register** the **UserPod** + **Curator** native agents (D2) and the **KaskPanel** (D10); add `ToggleKaskPanel` + `workspace.add_panel`.
6. **Migrate** config (T6.3) on first launch.

`KaskCore` is the "shared core" R4 referred to — the single owner of storage/regulation/memory the MCP servers take handles from (prevents the two-instance pitfall).

### 13.4 Consolidated divergence map (D1–D10)
| D | Surface | zed-kask file | Connection (port/adapter) |
|---|---|---|---|
| D1 | skill execution | `agent_skills` + `agent/tools/skill_tool.rs` | skill_tool → bridge.ManifestExecutor(InferencePort, ToolPort) |
| D2 | Curator agent | `agent.rs` + `native_agent_server` + `agent_servers` | native agent → CuratorTurnPort → in-process Curator |
| D3 | tools in-process | `context_server/client.rs` + `transport/` | in-process transport → ToolPort; MCP servers take `KaskCore` handles (R4) |
| D4 | guard | `language_model_core`/`language_model` | `GuardedInferencePort` wraps `InferencePort`-over-`LanguageModel` |
| D5 | sovereignty keys | `credentials_provider` | **via `SecretsPort` adapter** (reconciles D9b) |
| D6 | thread→memory | `agent/thread.rs`/`thread_store.rs` | thread hook → `MemoryPort` → `KaskCore` memory |
| D7 | app-identity | `paths.rs`, `release_channel`, `mac_only_instance`, `Cargo.toml`, scripts | (zed-kask self-change; not a zed↔kask seam) |
| D8 | bridge + adapters | new `crates/kask_bridge` + `gpui_tokio` | **THE bidirectional seam** — implements all ports |
| D9 | settings + credentials | new `KaskSettings` section + `CredentialsProvider` namespace + `kask_page` | `KaskSettings` → `KaskCore` params; `SecretsPort` |
| D10 | kask panel | new `crates/kask_panel` (Panel) | panel → `ToolPort` + `InferencePort` (via bridge) + tool registry |

### 13.5 Cleanups — DONE (cleanup pass)
- ✅ **Stale "15" MCP counts** in §1.4, D3, T3.3 → **12** (now reads 12; §2.4).
- ✅ **§6 Phase 5** now matches the refinements: T5.2 gated on T3.0; T5.3 deletes the entire `hkask-repl/tui` incl. `mcp_scoped` (reimplemented natively, §12.4); T5.7 notes filesystem-MCP decided deleted (§2.4).
- ✅ **D5 text** now reads "via `SecretsPort` adapter (D9b)" (matches the dependency invariant).
- ✅ **§3 intro** now references D8–D10 (consolidated in §13.4).

### 13.6 grill-me on the composition
- **Is the bridge crate the only bidirectional seam?** Yes — by invariant. Audit: grep hKask for any `use` of a zed-kask crate — must be zero. (Open Q 27.)
- **Is `KaskCore` constructed once?** Yes — singleton; MCP servers + Curator + memory ingestion share its handles (prevents R1's two-instance pitfall).
- **Lifecycle:** `KaskCore` constructs at zed-kask startup; the UserPod is created on first-launch onboarding and thereafter loaded — confirm construction order (Open Q 28).
- **Do all surfaces follow established patterns?** ports-and-adapters (D8), decorator (guard), composition-root DI (startup), `Panel` (D10 copies `agent_panel.rs`), settings-section + credentials-namespace (D9 copies zed's own). Yes.

**Open Q 27. — DONE:** `scripts/check-hkask-no-zed-deps.sh` enforces the §13.1 invariant (no hKask crate depends on a zed-kask crate) and is wired into `.github/workflows/ci.yml` (invariants job).
**Open Q 28.** `KaskCore` lifecycle — constructed at zed-kask startup (singleton) vs UserPod created on first-launch onboarding; confirm ordering and per-pod storage open/close.

---

## 14. Repository Consolidation — full merge into zed-kask

**Decision (§0):** fully merge hKask into the `zed-kask` fork. zed-kask becomes the **single source of truth** for everything hKask is becoming — code, skills, scripts, and docs. The `mdz-axo/hKask` repo is **archived** (read-only reference). This replaces the earlier path-dep/submodule wiring (T0.6), which dissolved once hKask could no longer compile or run standalone (daemon/ACP/REPL/inference deleted; keep-crates need the in-process bridge + `gpui_tokio`).

### 14.1 Why (essentialist)
- **hKask crates are not independently shippable** after the deletions — they only compile inside zed-kask. A separate repo for non-standalone crates is friction (cross-repo path-deps, R10 hermeticity, two-clone dev, ownership ambiguity) with no value. P5: a module/repo that can't stand alone shouldn't be kept apart.
- **Removes R10 entirely** — no path-deps, no submodule, one clone/build/CI.
- **Strengthens minimal divergence + upstream sync:** under a `kask/` namespace, hKask's crates/skills/scripts/docs are **additive paths upstream doesn't have** → `git merge upstream/main` never touches `kask/` → near-zero conflict. The only upstream-merge surfaces are the D-seams (in zed's tree) and the `[workspace.members]`/`[workspace.dependencies]` arrays.

### 14.2 The `kask/` namespace (ours vs upstream)
Everything hKask lives under one top-level dir so "ours" is isolated from "upstream":
```
zed-kask/
├── crates/            # upstream zed (unchanged) + D-seam edits
├── mcp-servers/       # (none — hKask's are under kask/)
├── extensions/        # upstream
└── kask/              # ── OURS (additive; upstream never touches here) ──
    ├── crates/        # hkask-types, hkask-storage, hkask-memory, hkask-regulation,
    │                  # hkask-templates, hkask-pods, hkask-guard, hkask-capability,
    │                  # hkask-identity, hkask-keystore, hkask-wallet, hkask-ledger,
    │                  # hkask-mcp-server, kask_bridge (D8), kask_panel (D10)
    ├── mcp-servers/   # the 12 default-load + curator + skill (hkask-mcp-*)
    ├── skills/        # the skills registry (manifest.yaml + *.j2; Pattern A source of truth)
    ├── scripts/       # check-hkask-no-zed-deps.sh + hKask admin/build scripts
    └── docs/          # ← documentation home (see 14.3)
```
zed-kask's `Cargo.toml` adds `kask/crates/*` + `kask/mcp-servers/*` as workspace members and merges hKask's `[workspace.dependencies]` into its own. The bridge crate `kask_bridge` and panel `kask_panel` (D8/D10) live under `kask/crates/` too — they're ours, not upstream's.

### 14.3 Documentation home (`kask/docs/`)
All Kask documentation lives **inside zed-kask** under `kask/docs/`:
- `kask/docs/architecture/` — this plan (`zed-host-architecture-plan.md`), the four-pattern architecture, principles, ADRs.
- `kask/docs/specs/` — integration specifications (the D1–D10 seam specs), the port/adapter contracts, the MCP load set.
- `kask/docs/plans/` — phased migration plans, the upstream-sync runbook.
- `DIVERGENCE.md` stays at the zed-kask **repo root** (the fork's headline doc, referenced on every sync) and points into `kask/docs/` for detail.
The current plan at `Clones/hKask/docs/architecture/zed-host-architecture-plan.md` **moves to `kask/docs/architecture/`** during the migration (T0.6). The `mdz-axo/hKask` repo is archived with a `README.md` pointing to `zed-kask/kask/`.

### 14.4 Migration steps (T0.6 expansion)
1. Create `kask/{crates,mcp-servers,skills,scripts,docs}` in zed-kask.
2. Move hKask keep-crates → `kask/crates/hkask-*`; MCP servers → `kask/mcp-servers/hkask-mcp-*`; skills registry → `kask/skills/`; scripts → `kask/scripts/`; docs → `kask/docs/`.
3. Merge hKask `[workspace.dependencies]` into zed-kask's; add the `kask/*` members to zed-kask's `Cargo.toml`.
4. Decide history preservation: `git filter-repo --subdirectory-filter` to bring hKask history under `kask/` (preserves blame), or a clean copy (simpler, loses history). (Open Q 29.)
5. Archive `mdz-axo/hKask` (GitHub archive + root `README.md` → `zed-kask/kask/`).
6. Move `DIVERGENCE.md` references from `../../Clones/hKask/...` to `kask/...`.

### 14.5 What changes for the connection surfaces (§13)
- **§13.1 invariant still holds:** hKask crates (under `kask/crates/hkask-*`) still must NOT depend on zed crates (under `crates/`); `kask_bridge` (under `kask/crates/`) is still the sole bidirectional seam. One repo, same rule.
- **CI script tweak:** `scripts/check-hkask-no-zed-deps.sh` (now at `kask/scripts/`) — under the merge, the path-dep check's `zed-kask` string no longer matches (paths are intra-repo). The **denylist-name check is the real gate** (a `kask/crates/hkask-*` dep on `gpui`/`context_server`/etc. still fires). Broaden the path-dep check to flag any `kask/crates/**` path-dep pointing outside `kask/` (into `crates/`/`mcp-servers/`). (Open Q 32.)
- **Upstream sync (Phase 7):** conflicts only in the D-seam files + `[workspace.members]`/`[workspace.dependencies]`. `kask/` is additive → never conflicts. DIVERGENCE.md gains a line: "everything under `kask/` is ours; everything else is upstream."

### 14.6 Open questions (consolidation)
29. **History preservation** — `git filter-repo` (keep blame) vs clean copy (simpler)? (T0.6.)
30. **Namespace** — `kask/` top-level (recommended: isolates ours-vs-upstream) vs spreading hKask crates into zed's `crates/`/`mcp-servers/` (mixes; worse for divergence).
31. **DIVERGENCE.md location** — repo root (visibility) vs `kask/docs/` (grouped). Lean: root + a `kask/docs/divergence.md` mirror or pointer.
32. **CI script** — broaden `check-hkask-no-zed-deps.sh` path-dep check for intra-repo paths (denylist-name check already covers the real case).
