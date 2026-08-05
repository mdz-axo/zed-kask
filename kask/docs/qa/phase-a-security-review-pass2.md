# Phase A — Security Review (Second Pass)

Date: 2026-08-04. Scope: all kask-owned crates, second hardening pass over the
same continuation prompt. First-pass reports: `phase-a-security-review.md` (and
addendum). This pass re-verified every first-pass claim against HEAD with fresh
eyes and probed what both prior passes missed.

## Headline: the security regression library's enforcement was theater

The single largest finding of this pass. `scripts/check-kali-regressions.sh`
(wired into CI in `e37bef8a3b`, the commit *after* the first pass's review)
reported green while enforcing almost nothing, via **three independent vacuity
classes**:

| # | Class | Mechanism | Affected |
|---|-------|-----------|----------|
| V1 | Quote corruption | `lib-regressions.sh` pattern extraction stripped double quotes only; single-quoted YAML patterns kept literal `'` chars and could never match Rust source | 20 enforced grep entries (RR-0001, 0002, 0005, 0010, 0013, 0015, 0017, 0019, 0021, 0028, 0029, 0037, 0040–0046) |
| V2 | Dead include paths | Script `cd`s to `kask/`; entries used `kask/`-prefixed or pre-merge-layout paths (`kask/crates/…`, `mcp-servers/crates/…`, `crates/hkask-ports/…`, `src/lib.rs` in crates that use named root files); `grep … 2>/dev/null \|\| true` silently returned empty | 8+ entries incl. all of RR-0040–0046 |
| V3 | Inverted semantics | RR-0004/0010/0013/0017 are **presence** invariants (attribute/defense text MUST appear); the script only implemented absence semantics, so even with V1/V2 fixed they would have fired on the *healthy* state or passed on the *removed* state | 4 entries |

**Consequence found live:** RR-0044 (`status: enforced`, promoted by the first
pass's addendum) was violated by **17 unmarked production
`McpToolError::internal(format!` sites** at the moment the gate reported
`0 violations`. The promotion was made in good faith against a first-hand grep
— but the CI gate backing the `enforced` status could never have caught a
regression. This is the `.rules` "advertised invariants need enforcement
points" trap, instantiated by the enforcement mechanism itself.

### Repairs landed (commit `c23f6f9661` + follow-ups)

- `lib-regressions.sh`: single-quote stripping; **orphaned-gate detection**
  (a grep entry whose include paths don't exist on disk is now a hard error —
  stale paths can never again silently pass); `semantics: presence` support;
  `--exclude-dir=regressions` on the per-entry branch (a broadened RR-0015
  would otherwise self-match its own mitigation text); crate-name extraction
  for `mcp-servers/<crate>/` cargo-test includes; `set -e`-safe optional-field
  parsing.
- All 20+ entries repaired: include paths fixed to the post-merge tree,
  presence entries converted (`RR-0004` → real crate-root files, `RR-0013` →
  `hkask-capability/src/tool_port.rs`, `RR-0017` → `extract_json_from_response`
  presence in corpus), comment-tolerant negative-lookahead patterns for
  RR-0043/0045/0046 (doc comments legitimately quote the banned patterns),
  RR-0005's unmatchable multiline pattern replaced with an honest presence
  proxy (`pub fn from_env`), RR-0019 converted to `runtime-assert` (its
  invariant is a cross-line harness+trainer combination grep cannot express;
  enforced at runtime by lora-training G6), RR-0021 extended to the workspace
  root where the calamine pin actually lives.
- Negative tests executed: a synthetic orphaned entry and a synthetic failing
  presence entry both fire correctly (then removed).
- RR-0044: pattern now `^(?!.*rr0044-ok).*McpToolError::internal\(format!`;
  the 17 sites triaged — 6 mis-classifications **fixed** (companies
  providers.rs ×4 and analysis.rs ×2: external-API parse/network failures →
  `unavailable`), 11 genuinely-internal sites annotated with same-line
  `// rr0044-ok: <reason>` markers (swarm serialize ×3, curator match-fallback,
  media template-render ×4, corpus serialize ×4). Gate re-run: green with the
  pattern actually live.

Post-repair: `37 enforced, 1 pending, 0 violations, EXIT=0` — and the gate has
been demonstrated to be *capable of failing*.

## New security findings (this pass)

### S1 — MEDIUM (fixed): corpus JSONL reads bypassed Layer 2 path containment
`read_jsonl` / `read_jsonl_lenient` (`hkask-mcp-corpus/src/helpers.rs`) and the
inline read in `corpus_prepare_training_dataset` (`tools/corpus/mod.rs:516`)
passed **raw MCP tool-argument paths** (`chunks_jsonl`, `prompts_jsonl`,
`tagged_jsonl`, `input_jsonl` — 6 call sites) directly to
`std::fs::read_to_string` with no `contain_for_read` and no size cap. A
single-line JSON credential file (e.g. `~/.config/gcloud/application_default_credentials.json`)
parses as valid JSONL and would flow into tool output. Both prior passes'
"containment at all 12 caller-path sites" claim counted only the
`contain_for_*` call sites and missed this read family.
**Fixed** (commit `61abea787f`): helpers now route through
`path_safety::read_capped` (containment + `MAX_READ_BYTES`); escape tests
added (`/etc/passwd`, `../../escape.jsonl`, positive control).

### S2 — MEDIUM (status corrected): Layer 3 "instruction hierarchy" has no artifact
RR-0010 (`enforced`) pointed at `crates/hkask-types/src/agent_registry.rs` — a
path that **never existed in this repo's git history** (pre-merge hKask
layout). The instruction-hierarchy text (System P10 > User P20 > Tool P30)
exists nowhere in kask Rust source, registry manifests, or system-agent
charters; only the adversarial-red-team and kali-audit skill *templates*
describe it as "Layer 3" of the defense stack. The defense-layer matrix in the
first-pass report did not list it (its 8 layers are real), but the skill
templates advertise a deployed defense that has no enforcement point.
**Action taken:** RR-0010 downgraded to `status: pending` with a full
provenance note. **Open decision for the operator:** either deploy hierarchy
text into a real system prompt (then re-enforce RR-0010 as a presence gate),
or remove "Layer 3 instruction hierarchy" claims from the skill templates.

### S3 — LOW (accepted): memory data-boundary markers are spoofable
`format_recall_context` markers are plain text; a recalled memory whose
*content* contains the closing marker (`--- End Memory Context ---`) escapes
the data framing for the remainder of that snippet. The module honestly
documents the defense as "framing, not filtering" and the injection-phrase
test pins that content is not scrubbed — this is an inherent limit of textual
boundaries, consistent with the documented Layer-7 posture. No change; noting
so the limitation is on record.

### S4 — LOW (noted): non-canonical warn targets on governance denial paths
`runtime.rs:561` (`hkask.mcp.cap`, fail-closed charge denial) and
`runtime.rs:590` (`hkask.mcp`), plus `hkask_ledger.rs` (`hkask.ledger`) emit
operational warns on non-`reg.*` targets — same broken-feedback-loop class as
the first pass's F1-secondary (fixed for `reg.guard.redact`). Pre-existing;
not changed this pass (renaming span targets requires checking consumers);
candidates for the same canonicalization treatment.

## Re-verification of prior-pass fixes (all HOLD)

| Probe | Verdict | Evidence |
|---|---|---|
| `redact_spans` merge-on-overlap | HOLDS | sort → clamp → drop-inverted (canonical `reg.guard.redact` warn) → merge (`start <= last_end`) → single pass; adjacent-span merge is a safe cosmetic change; past-EOF clamps conservative; 10/10 redact tests incl. 2 new deterministic unit tests |
| GuardedStream 256KB cap | HOLDS | cap checked on combined text+reasoning after both accumulations; `scanned=true` on breach/inner-error; double-poll test pins re-entrancy |
| IPC timeout + guard-nulling | HOLDS | `tokio::time::timeout` wraps entire `read_line`; all four error branches (`read`, EOF, parse, ID-mismatch) null the cached stream (`inference_ipc_client.rs:164-193`) |
| `debit_if_funds` atomicity | HOLDS | `BEGIN IMMEDIATE` precedes in-tx balance re-read; rollback path logs; sequential-debit TOCTOU test; no bypassing balance mutation in the crate |
| Memory data boundaries | HOLDS | `format_recall_context` at all 4 injector paths; 4 pinning tests (see S3 for the framing limit) |
| OCAP Layer 4 fail-closed | HOLDS | traced: token mismatch denies pre-dispatch (`runtime.rs:536-540`); absent governance denies (`:594-605`); failed cap-charge denies fail-closed (`:553-573`) |
| Media path handling | HOLDS | `image::open` sites take gallery-index-resolved or gallery-root-joined paths, not raw caller paths |
| WebID redaction | HOLDS | `identity.rs:259` uses `redacted_display()` |

## Supply chain (A2 re-check)

- `cargo deny --config kask/deny.toml check advisories` → **ok** (after this
  pass: stale `RUSTSEC-2026-0199` ignore removed — cargo-deny reported
  `advisory-not-detected`; `RUSTSEC-2024-0436` rationale corrected — the
  claimed `utoipa-axum` chain no longer exists; real parents are
  `pulp ← exr ← image` (touches kask-owned `hkask-media-widget`) and
  `simba ← nalgebra ← manatee`; block-level re-review date added 2026-11-04).
- `cargo machete`: zero unused deps workspace-wide (3 orphaned `log` deps from
  the first pass confirmed removed; new `tokio` in hkask-mcp-server and
  `hkask-memory` used).
- **`internal(e.to_string())` evasion spelling**: 82 sites match the broader
  `McpToolError::internal(` spelling; a 12-site sample showed most are genuine
  (mapper fallback arms, serialize-own-struct) — the 2 confirmed
  mis-classifications (companies `analysis.rs` reqwest failures) were fixed
  this pass. Widening RR-0044's pattern to all spellings requires triaging all
  82 sites first; documented in the RR-0044 entry as a known limitation.

## A3/A4 (deltas only)

- runtime-posture-monitor: first-pass manifest fixes confirmed present. New
  note: the `reg.*` log-scrape fallback becomes marginally more useful now
  that the `redact_spans` warn target is canonical.
- adversarial-red-team: no live target this pass either (documented gap,
  unchanged). New adversarial result: the S2 finding above — the skill's own
  "Layer 3" description is currently an advertised-but-undeployed defense.

## 8-layer defense coverage matrix (post-pass-2)

| Layer | Status | Delta from pass 1 |
|-------|--------|-------------------|
| 1 Input validation | Enforced | — |
| 2 Path containment | **Enforced (gap closed)** | S1: corpus JSONL read family now contained + capped |
| 3 Taint (FIDES) | Enforced | — |
| 4 OCAP gate | Enforced (re-traced) | — |
| 5 Gas/rjoule budget | Enforced | — |
| 6 Output scanning | Enforced | — |
| 7 GuardedStream | Enforced w/ documented caveats | — |
| 8 Secret redaction | Enforced | overlap fix verified + adversarially probed |
| (advertised "instruction hierarchy") | **Not deployed** | S2: templates describe it; no artifact exists (RR-0010 → pending) |
