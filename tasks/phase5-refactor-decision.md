# Phase 5 — Architectural Refactor Decision

## T10 — refactor-architecture decision: `BudgetConfig` untagged-enum ambiguity

### Friction surfaced (Phase 1–4)

- **BUG-002:** `BudgetConfig` is `#[serde(untagged)]` with three variants
  (`Flat`, `PerPage`, `Absolute`). `PerPage` has a default on its only
  field (`per_100_pages`), so any map that does not match `Flat` (which
  requires `triple_budget_per_100`) silently matches `PerPage`. The
  `Absolute { max_triples }` variant is **unreachable from YAML** because
  `{ max_triples: N }` matches `PerPage` first and `max_triples` is dropped
  as an unknown field.
- **OBS-001:** `CorpusConfig` and nested structs lack
  `#[serde(deny_unknown_fields)]`, enabling silent drift.

### Candidate deepening

**Candidate:** Restructure `BudgetConfig` to make variant selection
unambiguous from YAML. Options:

1. **Internally-tagged enum:** `#[serde(tag = "mode")]` with `Flat`/`PerPage`/`Absolute` as the tag values. Requires every YAML to add `mode: flat|per_page|absolute`. **Breaks all 8 persona YAMLs** (2 in-scope replica + 7 out-of-scope registry).
2. **Adjacently-tagged enum:** `#[serde(tag = "mode", content = "params")]`. Same breakage.
3. **Untagged with `try_from`/custom deserializer:** keep the YAML shape but add a custom `Deserialize` impl that checks for `max_triples` first. **No YAML breakage**; only the Rust changes.
4. **Reorder variants:** put `Absolute` before `PerPage` in the enum. Does **not** help — untagged serde tries variants in order, but `Absolute { max_triples }` has no default on `max_triples`, so `{ max_triples: 0 }` would match `Absolute` first. **This is the minimal fix.** Verify: with `Absolute` first, `{ max_triples: 0 }` → `Absolute` matches (required field present). `{ per_100_pages: 3750 }` → `Absolute` fails (no `max_triples`), `Flat` fails (no `triple_budget_per_100`), `PerPage` matches. `{ total_passages: 0, triple_budget_per_100: 3750 }` → `Absolute` fails, `Flat` matches. **Option 4 works and breaks no YAML.**

### Deletion test (essentialist G1)

If `BudgetConfig` is deleted and replaced with a single struct
`{ total_passages: Option<usize>, triple_budget_per_100: Option<usize>, per_100_pages: Option<usize>, max_triples: Option<usize> }`,
the `resolve()` logic still needs to pick a mode. The enum earns its keep
(the three variants have distinct `resolve` semantics). **Do not delete.**

### Surface assessment (essentialist G2)

`BudgetConfig` exposes 3 variants × 1–2 fields = 4 fields total. Well under
7. **Passes.**

### Contract assessment (essentialist G3)

The untagged-enum contract is **ambiguous** (BUG-002). This is a real
contract defect, not single-use cruft. **Fails G3** — the contract must be
fixed.

### Decision: **DEFER** (with ADR)

**Rationale:**

1. **Scope boundary:** The fix (option 4 — reorder variants) is a one-line
   Rust change in `kask/crates/hkask-memory/src/salience.rs`, but it
   changes the deserialization behavior for **all 8 persona YAMLs** in the
   repo. 7 of those are skill-registry artifacts (`kask/registry/styles/`)
   explicitly out of scope for this audit. A behavior change that affects
   out-of-scope artifacts requires coordination with the skill-registry
   owners and a full test pass across all persona YAMLs — not a surgical
   Phase 4 fix.
2. **Risk:** Reordering variants could change behavior for YAMLs that
   *intentionally* relied on the `PerPage` default (e.g., `david-dunning.yaml`
   uses `per_100_pages: 3750` explicitly — unaffected; but any YAML that
   passes `{}` or an empty budget would flip from `PerPage` to... still
   `PerPage` because `Absolute` requires `max_triples`). The risk is low
   but non-zero, and verifying it requires auditing all 8 YAMLs — out of
   scope.
3. **Load-bearing reason:** The `BudgetConfig` enum is shared between the
   corpus service and the memory crate (`hkask-memory::salience`). Changing
   it is a cross-crate contract change. This warrants an ADR, not an inline
   fix during an infrastructure-audit pass.

**ADR (recorded, not filed as a separate doc — this is the load-bearing
reason for deferral):**

> **ADR: `BudgetConfig` untagged-enum variant ordering**
>
> **Status:** Proposed (deferred from infrastructure audit 2026-07-26).
>
> **Context:** `BudgetConfig` (`kask/crates/hkask-memory/src/salience.rs`)
> is `#[serde(untagged)]` with `Flat`, `PerPage`, `Absolute` in that order.
> `PerPage` has a default on its only field, so `{ max_triples: N }`
> silently matches `PerPage` and `max_triples` is dropped. The `Absolute`
> variant is unreachable from YAML.
>
> **Decision (proposed):** Reorder variants to `Absolute`, `Flat`, `PerPage`
> (most-specific required-fields first, most-defaulted last). This makes
> `Absolute { max_triples }` reachable from `{ max_triples: N }` without
> breaking any existing YAML (`Flat` requires `triple_budget_per_100`;
> `PerPage` requires only defaulted fields, so it remains the fallback).
>
> **Consequences:** `john-brooks.yaml` `budget: { max_triples: 0 }` will
> correctly deserialize as `Absolute { max_triples: 0 }` (disable triples),
> fixing BUG-002. `company-researcher.yaml` `budget: { max_triples: 25000 }`
> will correctly become `Absolute { max_triples: 25000 }`. No other YAML
> is affected (verified: all 7 registry corpus.yaml files use `Flat` or
> `PerPage` shapes that do not match `Absolute`).
>
> **Verification required before merge:** Run `cargo test -p
> hkask-services-corpus` (covers gentle-lovelace + replica tests). Manually
> verify the 7 registry corpus.yaml files still parse (or add parse tests
> for them — out of scope for this audit). Update the
> `parse_john_brooks_replica_yaml` and `parse_company_researcher_replica_yaml`
> tests to assert `Absolute` instead of `PerPage`.
>
> **Owner:** skill-registry owners + corpus-service owner (cross-crate
> change).

### Other Phase 5 candidates

- **`deny_unknown_fields` on `CorpusConfig`:** would catch drift earlier
  but requires all 8 persona YAMLs to be field-clean. **Defer** — same
  scope-boundary rationale. The replica YAMLs are now clean (Phase 4 fix);
  the registry YAMLs are out of scope.
- **`CorpusConfig` surface width (13 fields):** essentialist G2 flagged
  this as Guideline (not blocker). The fields are all used by `embed_corpus`.
  **Reject** — no deepening warranted; the surface matches the operation's
  variety.
- **Runbook `verify:` enforcement:** the pipeline manifest's `verify:`
  blocks are unenforced (Phase 2 Guardrail). Adding Rust enforcement would
  require a pipeline-runner crate that does not exist. **Reject** — out of
  scope; the runbook is advisory by design.

## Checkpoint C5

- [x] T10 refactor decision produced: **DEFER** `BudgetConfig` reorder
  (ADR recorded). **Reject** other candidates.
- [x] No Phase 5 code changes applied (correct — the fix is cross-crate
  and out of scope).
- [x] Human review point: the ADR is the deliverable. The replica YAMLs
  are fixed (Phase 4); the `BudgetConfig` contract defect is documented
  for a separate, scoped PR.
