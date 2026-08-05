# Pre-Release Final Summary — Pass 3

Date: 2026-08-04. Third hardening pass over the pre-release continuation
prompt. Pass 2 was READY with one open operator decision (D1) and five
deferred items (D2–D6). This pass landed the gate-self-test institutionalization,
the D1 decision, the marker-spoofing hardening (S3), the governance warn-target
canonicalization (D4), and the B3 doc comment. D2/D3 and the evolving-test-harness
decision remain open (see Open items).

## Release-readiness verdict: **READY**, no open blockers

The D1 operator decision (instruction hierarchy: deploy or de-advertise) was
resolved in favor of **de-advertise**. The advertised-but-undeployed "Layer 3
instruction hierarchy" defense is now removed from all skill templates and
RR-0010 is `retired`. The defense stack is honestly 7 layers, not 8.

## Tasks completed this pass

### Task 1 — Pass-2 remainder landed

The pass-2 uncommitted work (`.github/workflows/kask-ci.yml`, `kask/deny.toml`,
four pass-2 QA reports) was already committed in `fee5976846` before this pass
began. Re-validated before continuing: YAML parses, kali-regressions
`37 enforced, 0 violations`, `cargo deny advisories` ok.

### Task 2 — Gate self-test institutionalized

`kask/scripts/check-kali-regressions-selftest.sh` — injects three synthetic
violations into a temp copy of `security/regressions/` (never the real dir, so
a parallel CI run cannot pick them up) and asserts the gate detects each:

1. **Orphaned include path** → gate fails with "orphaned" (dead-path detection)
2. **Presence semantics with unmatchable pattern** → gate fails with "presence"
3. **Absence semantics with a present banned pattern** → gate fails with "violated"

`trap` cleans up the temp dir on all exit paths. The real `security/regressions/`
directory is copied to `mktemp -d` under a `KASK_REGRESSIONS_DIR` env override
(new in `lib-regressions.sh` — backward-compatible, defaults to the old value),
so the synthetic entries inherit a realistic sibling set without polluting the
real gate.

CI job `kali-regressions-selftest` added to `.github/workflows/kask-ci.yml`
(no toolchain needed — grep-only synthetic entries). This institutionalizes the
`.rules` trap "A CI gate must be shown to fail before its `status: enforced` is
trusted" — the gate that reported green for weeks while three vacuity classes
made 20+ entries unenforceable can no longer silently re-vacate.

### Task 3 — Instruction hierarchy de-advertised (D1 resolved)

Operator decision: **de-advertise**. The "Layer 3 instruction hierarchy
(System P10 > User P20 > Tool P30)" was advertised in skill templates but never
deployed — no hierarchy text exists in any kask system prompt, registry
manifest, or Rust source (pass-2 finding S2). Rather than cross an upstream
D-seam to deploy a soft defense, the references were removed:

- `adversarial-red-team/select-target.j2` — 8-layer stack renumbered to 7;
  table rows attributing `goal_hijacking`/`authority_override` to "Layer 3"
  reworded to point at Layers 1+2 (the real input-filter + spotlighting defenses)
- `adversarial-red-team/generate-adversarial.j2` — 8-layer stack → 7-layer
- `adversarial-red-team/test-against-target.j2` — resistance-criteria table and
  bypass-detection table renumbered; "Layer 7 caveat" → "Layer 6 caveat"
- `adversarial-red-team/manifest.yaml` — "8-layer defense-in-depth stack" → 7
- `adversarial-red-team/manifests/adversarial-red-team.yaml` — "standard 8-layer
  stack" → 7
- `kali-audit/select-surface.j2` — defense-layer catalog table renumbered
  (Layer 3 row removed; Layers 4-8 → 3-7)
- `kali-audit/audit.j2` — removed the "Instruction hierarchy text in system
  prompts" checklist item from the `mcp` surface checks
- `supply-chain-sentinel/probe.j2` — "kali-audit 8-layer catalog" → 7
- `runtime-posture-monitor/classify-threat.j2` — "kali-audit covers 8 layers" → 7
- `docs/explanation/security-skills-smoke-test.md`,
  `docs/reference/mcp-servers/swarm.md`,
  `docs/explanation/abw-swarm-orchestration.md` — "8 layers" → 7

The OpenAI Instruction Hierarchy paper (arXiv:2404.13208) remains cited as an
academic source in `kask/registry/manifests/kali-audit.yaml` and
`kask/registry/templates/kali-audit/audit.j2` — that is a citation, not a
deployment claim.

`kask/security/regressions/RR-0010.yaml` → `status: retired` with a full
provenance note. `lib-regressions.sh` gained explicit `retired` status handling
(acknowledged in the summary line, not silently dropped). Gate now reports
`37 enforced, 0 pending, 1 retired, 0 violations`.

### Task 4 — Marker-spoofing hardening (S3)

`kask/crates/kask_bridge/src/context_injector.rs` — `format_recall_context`
now neutralizes occurrences of the literal `MEMORY_CONTEXT_CLOSE` string
(`--- End Memory Context ---`) inside snippet text before wrapping, by inserting
a zero-width space (`\u{200b}`) that breaks the exact byte sequence the model
is told to treat as a boundary. A recalled memory whose content contains the
closing marker can no longer close its own data frame and inject instructions
into the surrounding system message.

This is framing-preservation, not content filtering: the snippet body is
otherwise preserved verbatim, and the opening marker is not neutralized (an
extra opening marker is harmless — it re-asserts the data frame — while an
extra closing marker escapes it). The existing test
`format_recall_context_does_not_redact_injection_phrases` still passes
(injection phrases survive verbatim). New test
`format_recall_context_neutralizes_embedded_close_marker` asserts: (a) exactly
one real closing marker in the output (the formatter-added one), (b) the
neutralized marker words survive, (c) the injection payload after the embedded
marker remains inside the data frame.

### Task 5 — Canonicalize governance warn targets (D4 resolved)

Non-`reg.*` tracing targets on regulation/governance failure paths were
invisible to the runtime-posture-monitor skill (broken feedback loop — same
class as the pass-2 `reg.guard.redact` fix). Migrated:

- `kask/crates/hkask-mcp/src/runtime.rs:561` — `hkask.mcp.cap` → `reg.mcp.cap`
  (fail-closed charge denial)
- `kask/crates/hkask-mcp/src/runtime.rs:590` — `hkask.mcp` → `reg.mcp`
  (span-persist failure)
- `kask/crates/hkask-ledger/src/hkask_ledger.rs:160,271` — `hkask.ledger` →
  `reg.ledger` (rollback failures)

Before renaming: grepped the whole repo for consumers filtering these exact
target strings — no consumers in `kask/scripts/`, `kask/registry/templates/`,
`crates/zed/src/main.rs`, or `kask_bridge`. The only references were in docs
(prose). The 5 remaining `hkask.mcp` targets in `runtime.rs` (L126, L214, L262,
L336, L407) are `info!`-level performative telemetry (server lifecycle), NOT
governance failure paths — correctly left as `hkask.*` per the
`check-reg-canonical.sh` rationale ("Performative telemetry MUST use `hkask.*`
targets, not `reg.*`").

`reg.mcp.cap` and `reg.ledger` registered in `CANONICAL_NAMESPACES`
(`kask/crates/hkask-types/src/event.rs`). `check-reg-canonical.sh` green.
`check-reg-creep.sh` shows only the pre-existing `reg.guard.redact` finding
(not from this pass — `reg.guard.redact` is used as a target but not
exactly-registered; `check-reg-canonical.sh` accepts it via ancestor matching
but `check-reg-creep.sh` requires exact registration; this gate-vs-gate
inconsistency is pre-existing and out of scope here).

### Task 6 — B3 doc comment

`kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs` — added a `///` doc
comment to `corpus_query` explaining the poisoned-lock recovery is a deliberate
availability-over-consistency choice (serving possibly-stale results rather than
taking the corpus offline), with the threat model (worst case is stale/incomplete
results, not corruption — the index is rebuilt from source JSONL on restart).

## Open items

| # | Item | Owner | Status |
|---|------|-------|--------|
| D1 | Instruction hierarchy | Operator | **Resolved** — de-advertised; RR-0010 retired |
| D2 | `internal(e.to_string())` evasion spelling: 62 unmarked sites remain (down from 82) | Post-release | Deferred — substantial multi-server triage |
| D3 | `swarm_panel.rs` still 4,112 lines; `render_swarm_detail`/`render_card` next extraction | Post-release | Deferred — ~500 lines of extraction |
| D4 | Non-canonical warn targets | This pass | **Resolved** — `reg.mcp.cap`, `reg.mcp`, `reg.ledger` |
| D5 | Marker-spoofable memory data boundaries | This pass | **Hardened** (Task 4) — embedded close marker neutralized |
| D6 | Release version/changelog scope | User | Still open |
| D7 | Evolving-test-harness plan doc claims "Implemented" while CI steps were removed as dead | User | Awaiting decision (rebuild script or retire doc status) |
| D8 | `reg.guard.redact` exact-registration inconsistency between `check-reg-canonical.sh` (ancestor match) and `check-reg-creep.sh` (exact match) | Post-release | Pre-existing; noted, not fixed |

## New findings this pass

- **D7 (LOW)**: `kask/docs/plans/evolving-test-harness.md` (1,178 lines) claims
  "Status: Implemented (all 6 slices + TDD orchestration)" while its CI steps
  were removed as dead in pass 2 (referenced the deleted `kask/scripts/test`,
  had been silently no-op'ing behind `|| true`). The `hkask-test-harness` crate
  and `harness-evolve-cycle.sh` still exist. This is the advertised-invariant-
  without-enforcement class. Awaiting operator decision: rebuild the script
  properly or retire the doc's status claim.
- **D8 (NIT)**: `reg.guard.redact` is used as a tracing target but is not
  exactly registered in `CANONICAL_NAMESPACES` (only `reg.guard` ancestor is).
  `check-reg-canonical.sh` accepts it (ancestor matching); `check-reg-creep.sh`
  flags it (exact match required). Pre-existing gate-vs-gate inconsistency,
  not introduced this pass.

## Validation run

- `bash scripts/check-kali-regressions.sh` → `37 enforced, 0 pending, 1 retired,
  0 violations`, exit 0
- `bash scripts/check-kali-regressions-selftest.sh` → all 3 synthetic violations
  detected, exit 0
- `bash scripts/check-unsafe-forbid.sh` → 38/38
- `bash scripts/check-reg-canonical.sh` → OK
- `bash scripts/check-skill-span-namespace.sh` → 95 manifests conform
- `bash scripts/check-lora-training-regressions.sh` → 1 enforced, 0 violations
- `cargo test -p kask_bridge --lib` → 118 passed
- `cargo test -p hkask-mcp --lib` → 0 (no lib tests in this crate)
- `cargo test -p hkask-ledger --lib` → 20 passed
- `cargo test -p hkask-types --lib` → 96 passed
- `cargo test -p hkask-mcp-corpus --lib` → 176 passed, 1 ignored
- `cargo fmt --check` on all touched crates → clean
- `cargo deny --config kask/deny.toml check advisories` → ok
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/kask-ci.yml'))"`
  → parses (12 jobs including new `kali-regressions-selftest`)
- `bash -n` on both modified scripts → syntax OK

## Commit / working-tree state

All pass-3 work is uncommitted, ready for your review. Files touched:

- `kask/scripts/check-kali-regressions-selftest.sh` (new)
- `kask/scripts/lib-regressions.sh` (KASK_REGRESSIONS_DIR override + retired status)
- `.github/workflows/kask-ci.yml` (new kali-regressions-selftest job)
- `kask/crates/kask_bridge/src/context_injector.rs` (marker neutralization + test)
- `kask/crates/hkask-mcp/src/runtime.rs` (2 warn targets → reg.*)
- `kask/crates/hkask-ledger/src/hkask_ledger.rs` (2 warn targets → reg.ledger)
- `kask/crates/hkask-types/src/event.rs` (reg.mcp.cap + reg.ledger registered)
- `kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs` (B3 doc comment)
- `kask/security/regressions/RR-0010.yaml` (status: retired)
- `kask/registry/templates/adversarial-red-team/*.j2` (8→7 layer renumbering)
- `kask/registry/templates/adversarial-red-team/manifest.yaml` (8→7)
- `kask/registry/manifests/adversarial-red-team.yaml` (8→7)
- `kask/registry/templates/kali-audit/select-surface.j2`, `audit.j2` (8→7)
- `kask/registry/templates/supply-chain-sentinel/probe.j2` (8→7)
- `kask/registry/templates/runtime-posture-monitor/classify-threat.j2` (8→7)
- `kask/docs/explanation/security-skills-smoke-test.md`,
  `kask/docs/reference/mcp-servers/swarm.md`,
  `kask/docs/explanation/abw-swarm-orchestration.md` (8→7)
- `kask/docs/qa/pre-release-final-summary-pass3.md` (this file)

No commit was made (the task brief did not request one). User WIP files
(`tasks/plan.md`, `tasks/todo.md`,
`docs/reports/prediction-markets/02-zed-kask-integration.md`,
`kask/docs/plans/prediction-plan.md`) were not touched.

## Suggested .rules additions

1. **A retired RR entry must be acknowledged in the gate summary, not silently
   dropped.** `lib-regressions.sh` previously had no `retired` status branch —
   a retired entry was silently skipped, making it indistinguishable from a
   deleted file. The fix adds explicit `retired` handling so the summary
   reports `N retired`. (Non-obvious: a retired entry and a deleted entry are
   observationally identical without this; actionable: add the status branch
   when retiring an entry.)

2. **Synthetic gate self-tests must not pollute the real regressions directory.**
   The self-test copies `security/regressions/` to a `mktemp -d` and points the
   gate at the copy via `KASK_REGRESSIONS_DIR`. A self-test that writes
   `RR-TEST-*.yaml` into the real `security/regressions/` would be picked up by
   a parallel real-gate CI run, flapping it. (Non-obvious: CI jobs run in
   parallel; repeatedly encountered: pass-2 ran synthetic tests manually and
   had to remember to delete them; actionable: always use a temp dir + env
   override.)
