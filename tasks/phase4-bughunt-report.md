# Phase 4 — Bug Hunt & Diagnosis Report

## T7 — Bug-hunt expedition report (JSON-shaped markdown)

```json
{
  "expedition": "replica-yaml-contract-audit",
  "target_crates": [
    "kask/crates/hkask-services-corpus",
    "kask/corpus/replica/company-researcher.yaml",
    "kask/corpus/replica/john-brooks.yaml"
  ],
  "strategy": "Bach HTSM — configuration/interface/data",
  "beizer_categories": ["configuration", "interface", "data"],
  "probe": {
    "test_file": "kask/crates/hkask-services-corpus/tests/replica_persona_parse_test.rs",
    "command": "cargo test -p hkask-services-corpus --test replica_persona_parse_test -- --nocapture",
    "result": "2 passed; 0 failed (tests pin current buggy behavior)"
  },
  "findings": [
    {
      "id": "BUG-001",
      "tier": "BUG",
      "confidence": 0.95,
      "beizer": "configuration",
      "severity": "high",
      "pattern": "stale-config-schema",
      "artifact": "kask/corpus/replica/company-researcher.yaml",
      "consumer": "hkask_services_corpus::EmbedService::parse_config -> serde_yaml_neo::from_str::<CorpusConfig>",
      "symptom": "Parse fails: 'missing field exemplar_count_min at line 32 column 3' (serde fails fast at first missing required field).",
      "root_causes": [
        "centroid_entity_ref nested under embedding (must be top-level) — silently dropped as unknown under embedding, missing at top level",
        "validation.exemplar_count_min omitted (required, no default)",
        "validation.exemplar_count_max omitted (required, no default)",
        "foundational_rules omitted (required, no default)",
        "entities.concepts are bare strings (must be Entity { name, appears_in })",
        "methods[].threshold: {} (must be Option<f64>)",
        "dimension_centroids[].dimension (must be name; ref_name and weight required)"
      ],
      "evidence": "[DIAG-0001] parse error: ServiceUnavailable (Wallet): Failed to parse corpus config YAML: validation: missing field `exemplar_count_min` at line 32 column 3"
    },
    {
      "id": "BUG-002",
      "tier": "BUG",
      "confidence": 0.90,
      "beizer": "interface",
      "severity": "medium",
      "pattern": "untagged-enum-silent-variant-mismatch",
      "artifact": "kask/corpus/replica/john-brooks.yaml",
      "consumer": "hkask_services_corpus::EmbedService::embed_corpus -> config.budget.resolve(total_passages)",
      "symptom": "budget: { max_triples: 0 } deserializes as BudgetConfig::PerPage { per_100_pages: 3750 } (the default), not Absolute { max_triples: 0 }. The 'disable triples' intent is silently defeated; the persona would store ~3750 triples per 100 pages despite the YAML saying 0.",
      "root_causes": [
        "BudgetConfig is #[serde(untagged)] and PerPage has #[serde(default = default_budget_per_100_pages)] on its only field, so an untagged match of { max_triples: 0 } tries Flat (fails: triple_budget_per_100 required), then PerPage (succeeds via all-default fields), never reaching Absolute. max_triples is dropped as an unknown field on the PerPage variant."
      ],
      "evidence": "test parse_john_brooks_replica_yaml pins PerPage{per_100_pages:3750}; BudgetConfig enum at kask/crates/hkask-memory/src/salience.rs:825 is #[serde(untagged)] with PerPage default at line 838.",
      "fix_location": "kask/crates/hkask-memory/src/salience.rs::BudgetConfig (Phase 5 — contract design change, not a YAML fix). The YAML is correct by intent; the contract is ambiguous."
    },
    {
      "id": "OBS-001",
      "tier": "OBSERVATION",
      "confidence": 0.80,
      "beizer": "configuration",
      "severity": "low",
      "pattern": "missing-deny-unknown-fields",
      "artifact": "kask/crates/hkask-services-corpus/src/embed/types.rs::CorpusConfig",
      "consumer": "serde_yaml_neo::from_str::<CorpusConfig>",
      "symptom": "CorpusConfig and all nested structs lack #[serde(deny_unknown_fields)]. Unknown fields (e.g., company-researcher.yaml's validation.per_dimension, budget.mode, embedding.centroid_entity_ref) are silently dropped. This is the enabling condition for BUG-001's silent drift: the schema evolved but stale YAMLs parse partially instead of failing loudly at the unknown field.",
      "fix_location": "Phase 5 decision — adding deny_unknown_fields would catch drift earlier but requires all existing YAMLs (gentle-lovelace, etc.) to be field-clean."
    },
    {
      "id": "OBS-002",
      "tier": "OBSERVATION",
      "confidence": 0.70,
      "beizer": "data",
      "severity": "low",
      "pattern": "dead-artifact",
      "artifact": "kask/corpus/replica/company-researcher.yaml",
      "consumer": "none (grep for company-researcher.yaml in *.rs/*.md/*.sh/*.toml returns zero references outside the artifact itself and tasks/)",
      "symptom": "The persona may be dead — no code or doc references it. The header references a replica_build tool that no longer exists (current tool is corpus_build_persona).",
      "fix_location": "Human decision — fix (Phase 4 T8) or delete (follow-up)."
    }
  ],
  "taxonomized_counts": {
    "BUG": 2,
    "POTENTIAL_BUG": 0,
    "OBSERVATION": 2
  }
}
```

## T8 — Diagnose post-mortem: `company-researcher.yaml` contract drift

### Phase 0 — Anchor to functional requirements

**FR (functional requirement, paraphrased from the consumer contract):**
The artifact must deserialize into `CorpusConfig` via
`EmbedService::parse_config` and produce a valid `company-researcher`
persona embedding configuration. No fabricated FR# ref — the contract is
the `CorpusConfig` struct.

**Spec gap flagged:** There is no written spec for which personas are
still intended to be buildable. `company-researcher.yaml` may be dead
(OBS-002). The fix below assumes the human wants it buildable; if not,
the alternative is deletion (follow-up).

### Feedback loop

The T7 test `parse_company_researcher_replica_yaml_currently_fails` is the
deterministic feedback loop. It is **red** before the fix (parse fails),
**green** after (parse succeeds). The test is written before the fix
(regression guard).

### Hypotheses (falsifiability, ranked)

- **H1 (missing required fields):** The parse fails because `centroid_entity_ref`, `validation.exemplar_count_min`, `validation.exemplar_count_max`, `foundational_rules` are required and omitted. **Confirmed** by [DIAG-0001] error message.
- **H2 (type mismatch on optional fields):** Even if H1's fields were added, `entities.concepts` (bare strings), `methods[].threshold: {}`, `dimension_centroids[].dimension` would fail. **Not directly confirmed** (serde fails fast at H1), but inferred from the struct shapes.
- **H3 (unknown-field rejection):** The parse fails because `deny_unknown_fields` rejects `validation.per_dimension` etc. **Falsified** — `CorpusConfig` has no `deny_unknown_fields`; unknown fields are silently dropped.

### Instrumentation

- `[DIAG-0001]` — print the actual `serde_yaml_neo::Error` from `parse_config`. Mapped 1:1 to H1. Result: `missing field exemplar_count_min at line 32 column 3` — confirms H1, falsifies H3.

### Fix applied

The fix conforms `company-researcher.yaml` to `CorpusConfig` (the contract
of record). The `john-brooks.yaml` shape is the reference. Changes:

1. Move `centroid_entity_ref` from under `embedding:` to top level.
2. Remove `validation.per_dimension` (unknown, dropped).
3. Add `validation.exemplar_count_min` and `validation.exemplar_count_max`.
4. Remove `budget.mode` and `budget.triple_budget_per_100` (unknown, dropped). Keep `max_triples: 25000` — but note BUG-002: with the untagged `BudgetConfig`, `{ max_triples: 25000 }` will silently become `PerPage { per_100_pages: 3750 }`. To actually get `Absolute { max_triples: 25000 }`, the YAML cannot express it unambiguously today. **Decision:** keep `max_triples: 25000` (intent-documented) and flag the `BudgetConfig` ambiguity as Phase 5. The persona will get `PerPage` behavior, which is the same bug as `john-brooks.yaml`. This is consistent (both YAMLs hit the same contract defect) and avoids introducing a third shape.
5. Convert `entities.concepts` from bare strings to `[{name: "..."}]` maps.
6. Convert `methods[].threshold: {}` to `threshold: null` (or remove it; `Option<f64>` defaults to `None` when omitted). Remove `threshold` entirely — it is optional.
7. Convert `dimension_centroids[].dimension` to `name`, add `ref_name` and `weight`.
8. Add `foundational_rules: []` (required, no default; empty satisfies).

### Regression test

The T7 test `parse_company_researcher_replica_yaml_currently_fails` is
**flipped** to a positive parse test mirroring `parse_john_brooks_replica_yaml`
after the fix. The test asserts the fixed YAML parses and populates the
required fields.

### Instrumentation cleanup

`[DIAG-0001]` is removed from the test (the positive test does not need it).
The `eprintln!` is left only in the (now-flipped) failure-pinning test if
retained; otherwise removed.

### Post-mortem

**What happened:** `company-researcher.yaml` was authored against an older
`CorpusConfig` schema. The schema evolved (`DimensionCentroid` gained
`ref_name`/`weight`, `ValidationConfig` gained `exemplar_count_*`,
`Entity` became a struct, `centroid_entity_ref` moved to top level,
`BudgetConfig` became an untagged enum). The YAML was never updated. Because
`CorpusConfig` lacks `deny_unknown_fields`, the drift was silent for the
optional fields, but the required fields caused a hard parse failure.

**What went wrong:** No S3 (audit) layer in the VSM: nothing detects
artifact/contract drift until runtime. The `gentle_lovelace_corpus_test.rs`
integration test exists for one persona but no test guarded the replica
personas.

**What went well:** The T7 test now guards both replica YAMLs. Future drift
will fail in CI, not at operator runtime.

**Follow-up actions:**
1. Phase 5 decision on `BudgetConfig` untagged-enum ambiguity (BUG-002).
2. Phase 5 decision on `deny_unknown_fields` (OBS-001).
3. Human decision on whether `company-researcher.yaml` is still wanted
   (OBS-002). If not, delete it and the positive test.
4. Consider adding `deny_unknown_fields` to `CorpusConfig` and cleaning
   all persona YAMLs (gentle-lovelace, etc.) — out of scope for this audit
   (those are skill-registry artifacts).

## T9 — `john-brooks.yaml` regression

`john-brooks.yaml` parses (T7 test confirms). The `budget` silent-variant
mismatch (BUG-002) is a `BudgetConfig` contract defect, not a YAML defect.
No YAML fix applied to `john-brooks.yaml` in Phase 4. The bug is recorded
for Phase 5.

## Checkpoint C4

- [x] T7 bug-hunt expedition report produced (2 BUG, 2 OBSERVATION).
- [x] T8 fix applied to `company-researcher.yaml` (conformance to CorpusConfig).
- [x] T8 regression test flipped to positive assertion.
- [x] T9 `john-brooks.yaml` regression: parses; BUG-002 deferred to Phase 5.
- [x] `cargo test -p hkask-services-corpus --test replica_persona_parse_test` green.
- [ ] `./script/clippy` — to run after fix.
- [ ] Human review of fix + post-mortem.
