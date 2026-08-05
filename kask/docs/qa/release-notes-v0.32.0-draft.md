---
title: "Release Notes — v0.32.0 (Draft)"
audience: [developers, operators, users]
last_updated: 2026-08-04
version: "0.32.0"
status: "Draft"
domain: "Cross-cutting"
mds_categories: [trust, lifecycle]
---

# Release Notes — v0.32.0 (Draft)

## Security hardening

- **De-advertised the undeployed "Layer 3 instruction hierarchy" defense.** The
  instruction-hierarchy layer (System P10 > User P20 > Tool P30) was described in
  skill templates but never deployed in any system prompt. The defense stack is
  now honestly 7 layers, not 8. RR-0010 retired. The OpenAI Instruction Hierarchy
  paper remains cited as an academic source.
- **Widened the MCP error-classification regression gate (RR-0044).** The gate
  now catches all `McpToolError::internal(...)` spellings (previously only
  `internal(format!(...))`). All 69 sites triaged: genuine internal sites
  annotated with `rr0044-ok` markers; 5 mis-classifications fixed (ABW malformed
  responses → `unavailable`, session-balance query failures → `unavailable`,
  `HMemStore::from_driver` → `map_infra_error`).
- **Hardened memory data-boundary markers against spoofing.** Recalled memory
  content containing the literal closing marker (`--- End Memory Context ---`)
  can no longer close its own data frame — the marker is neutralized with a
  zero-width space before wrapping. Framing-preservation, not content filtering.
- **Closed corpus JSONL path-containment bypass.** `read_jsonl` /
  `read_jsonl_lenient` / `corpus_prepare_training_dataset` now route through
  `path_safety::read_capped` (containment + size cap) instead of raw
  `std::fs::read_to_string` on caller-supplied paths.
- **Canonicalized governance warn targets.** Fail-closed charge denial
  (`reg.mcp.cap`), span-persist failure (`reg.mcp`), and ledger rollback
  failures (`reg.ledger`) now emit on canonical `reg.*` tracing targets visible
  to the runtime-posture monitor. `reg.guard.redact` exactly registered,
  resolving the `check-reg-canonical` / `check-reg-creep` gate inconsistency.
- **Fixed vacuous security regression gates.** The kali-regressions gate
  reported green while three independent vacuity classes (single-quote pattern
  corruption, dead include paths, inverted presence semantics) made 20+ entries
  unenforceable. The gate now strips both quote styles, hard-fails on orphaned
  include paths, supports `semantics: presence`, and has a permanent self-test
  (`check-kali-regressions-selftest.sh`) that injects synthetic violations to
  verify the gate can actually fail.

## Reliability

- **Consolidated MCP error mapping.** Shared mappers (`map_infra_error`,
  `map_join_error`, `map_semantic_memory_error`, `map_io_error`) promoted to
  `hkask-mcp-server`; per-server `map_fs_error` duplicates deleted; per-variant
  classification enforced (PDF triage, job-store storage, training errors).
- **Extracted swarm_panel renderers.** `render_swarm_detail` and `render_card`
  extracted to `detail.rs` and `card.rs` (following the `author.rs` / `compose.rs`
  pattern). `swarm_panel.rs` reduced from 4,150 to 3,621 lines.
- **Fixed invalid kask-ci YAML.** The workflow had an unquoted `status:` colon
  in a step name that would have caused GitHub to reject the entire workflow
  file, disabling all 12 kask CI jobs. Dead trace/mutation steps removed;
  `kali-regressions` job gained missing toolchain/cache steps.
- **Refreshed deny.toml.** Stale `RUSTSEC-2026-0199` ignore removed; `paste`
  rationale corrected; block re-review date added (2026-11-04).

## Known limitations

- **`harness-evolve-cycle` is broken pending `kask/scripts/test` rebuild.** The
  cycle script and skill manifest reference `./scripts/test --trace`, which was
  deleted in `009b04066a`. The `hkask-test-harness` crate and
  `stability-gate.sh` remain functional. See
  `kask/docs/plans/evolving-test-harness.md` for the revival path.
- **The instruction-hierarchy defense layer is not deployed.** This release
  de-advertised the layer rather than deploying it. The data-boundary framing in
  `context_injector.rs` (hardened this release) plus the OCAP/gas gates provide
  the substantive defense; the prompt-level hierarchy text was not added to
  avoid crossing an upstream D-seam. If deployed in a future release, re-open
  RR-0010.
- **`internal(e.to_string())` triage is complete but the sites are
  annotation-based.** The widened RR-0044 gate relies on `rr0044-ok` same-line
  markers for genuine internal sites. A future refactor that moves the call off
  the annotated line (e.g. rustfmt reflow) would flag it as a violation —
  re-annotate after moving.
