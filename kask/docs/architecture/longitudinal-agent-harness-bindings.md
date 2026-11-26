---
title: "Longitudinal Agent Harness Bindings (Adoption Bar Conditions 2, 3, 6)"
audience: [architects, developers, agents]
last_updated: 2026-11-25
version: "0.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, trust, lifecycle, curation]
---

# Longitudinal Agent Harness Bindings

**Purpose:** Close the explicit open gap documented in *The Adoption Bar for Longitudinal Agents* (Labra & Pfeifer, 2026, p. 5; ingested in `zk-ref-open` as `the-adoption-bar-for-longitudinal-agents.pdf`): *"Legibility of agent memory and runtime authority enforcement belong to the harness. leggy.fish should state which harness bindings it expects to meet those conditions."*

This specification maps `zed-kask`'s live, verified Rust and MCP seams onto the three Adoption Bar conditions that a commitment protocol alone only covers partially — while preserving `zed-kask`'s non-delegable human PI closure boundary (`kask/docs/architecture/core/co-research-environment.md:17,54`).

---

## 1. Condition 2 — Bounded Authority (Runtime Enforcement Outside the Agent)

**Bar Test:** *"Does a forbidden action fail even when the agent tries it?"*

`leggy-fish`'s invariant compiler governs *acceptance* of deliverables, not runtime execution. `zed-kask` enforces runtime authority outside the model across four concrete boundaries:

| Runtime Perimeter | Mechanism | Live Seam (`file:line`) |
| --- | --- | --- |
| **Per-user storage isolation (P4.1)** | Dedicated SQLCipher DB per user data directory; no cross-user handle exists | `kask/docs/architecture/core/PRINCIPLES.md:67` |
| **Delegated tool grant intersection (P4.2)** | Parent-minted `delegation_grants` intersected with caller narrowing; missing token or poisoned lock fails closed | `kask/crates/kask_bridge/src/delegation_grants.rs:19,57,85`; `kask/crates/kask_bridge/src/inference_ipc_server.rs:954-960` |
| **Per-server credential & env allowlist (P4.2)** | Fail-closed server env allowlist; unknown server id rejected | `kask/crates/kask_bridge/src/mcp_servers/catalog.rs:26-424`; `kask/crates/kask_bridge/src/mcp_servers/env.rs:284` |
| **Mandatory spend & publish approval (D110 / P2)** | `hkask-mcp-wallet` routes every spend through `ApprovalPort::request_spend_approval` (native GPUI approval + `per_tx_cap_sats`, `daily_cap_sats`, `allowed_mints`); `hkask-mcp-nostr-client` holds no secret key and cannot sign locally | `kask/mcp-servers/hkask-mcp-wallet/src/approval.rs:36-48,99-158`; `kask/mcp-servers/hkask-mcp-wallet/src/config.rs:32-42`; `kask/mcp-servers/hkask-mcp-nostr-client/src/hkask_mcp_nostr_client.rs:11-12,177-182` |

**Honest limit:** Information flow control (taint tracking, Defense Layer 5) is absent by decision (`PRINCIPLES.md:75`); treat tool paths as taint-unaware.

---

## 2. Condition 3 — Legible State (Inspectable, Correctable, Deletable Memory)

**Bar Test:** *"Can the adopter see what the agent currently holds about them, and change it?"*

| Memory / State Surface | Inspection & Correction Mechanism | Live Seam (`file:line`) |
| --- | --- | --- |
| **Curator durable memory (`curator.db`)** | `curator_memory_recall` (exact entity or ontology axis), `curator_semantic_search`, `memory_update` (Bayesian confidence + value replacement), `memory_resolve_contradiction` (`forget` deletes row), `curator_memory_prune`, `curator_memory_dedup` | `kask/docs/architecture/core/PRINCIPLES.md:132,263`; `.agents/skills/therapy/SKILL.md` |
| **Canonical goals (`kanban:goal`)** | `kanban_goal_get` / `kanban_goal_list` expose full immutable spec, `spec_digest`, assessment attempts, and append-only human corrections (`kanban_goal_correct_outcome`); no silent edits | `kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md:61-107` |
| **Experimentation lineage** | `lineage_read` and `population_query` return sealed eval contracts, rubrics (including `disqualifier` and pinned `authority`), fitness records (including `declared_assumptions`), held-out final evaluations, and selection fossils | `kask/mcp-servers/hkask-mcp-experimentation/src/server.rs:169-319` |
| **Skill & regulation observability** | `curator_status`, `curator_algedonic_log`, `reg_query`, and `Algedonic review` board cards expose every threshold breach and skill outcome | `kask/crates/hkask-regulation/src/metacognition.rs:330-375`; `kask/crates/hkask-regulation/src/runtime.rs:543-545` |

---

## 3. Condition 6 — Recoverability (Staged Actions, Stop, Rollback & Portability)

**Bar Test:** *"When did you last exercise the stop and the rollback?"*

| Recovery Mechanism | Guarantee | Live Seam (`file:line`) |
| --- | --- | --- |
| **Code & artifact rollback** | Git working-tree discipline (`.rules`: pathspec commits, no working-tree-wide destructive commands); `zk-ref-open-sync` mandates `.pre-<revision>-<YYYY-MM-DD>` snapshots before mutating calibration runs | `.rules`; `.agents/skills/zk-ref-open-sync/SKILL.md` Step 3 |
| **Data portability & exit (P1 / P11.1)** | Per-user SQLCipher file (`{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`) + visible artifacts under `~/Documents/zk-data/`; backup/export is file copy with no vendor lock-in | `kask/docs/architecture/core/PRINCIPLES.md:263` |
| **Staged financial & public writes** | `hkask-mcp-wallet` requires native editor `ApprovalPort` confirmation + `idempotency_key` before every Cashu/NWC spend; `hkask-mcp-nostr-client` stages media locally before editor-signed publication | `kask/mcp-servers/hkask-mcp-wallet/src/approval.rs:36-48`; `kask/mcp-servers/hkask-mcp-nostr-client/src/blossom.rs:947-948` |
| **Non-delegable human closure** | `NativeGoalHost` requires an authenticated human `ActionChoice` for goal closure — no automated verdict or escrow release can close a human goal | `kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md:109-120` |

---

## 4. Verification Status

Every `file:line` reference above was verified against the local working tree on 2026-11-25 (`A6`). Runtime enforcement tests live in `hkask-mcp-wallet`, `hkask-mcp-nostr-client`, `hkask-mcp-kata-kanban`, and `hkask-mcp-experimentation`.
