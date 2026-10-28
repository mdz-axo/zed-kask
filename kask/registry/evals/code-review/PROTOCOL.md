# code-review eval harness — run protocol

Measures the `code-review` skill's detection recall, finding precision, and
severity correctness against planted-defect fixtures. Built as proposal P4 of
the 2026-10-09 improvement plan (card `09bae2d9-7985-4841-b2e4-2d26064432bf`);
ground truth is by construction (defects are planted), annotations are
operator-signed (signed_off 2026-10-10; see manifest.json annotation_status).

## Layout

- `fixtures/` — 13 scored fixtures (9 representative, 2 negative, 1 boundary,
  1 scenario). Each is a self-contained markdown file: spec, unified diff, and
  the surrounding-code context an axis walk needs.
- `heldout/` — 2 held-out fixtures. Never tune the skill against them; run
  them only to report.
- `manifest.json` — ground truth: per fixture, the expected findings
  (class, file, line, severity), expected verdict, and negative-fixture
  ceilings (`max_severity`, `tolerate_below`).
- Scoring: `kask/scripts/evals/code-review-score.sh`.

## Running a fixture

1. Read the fixture file. It is the entire codebase for the run — there is no
   git tree. Construct the scope inputs inline from it (`change_spec` from the
   Spec section, the diff as the change, `size_class` from its size, no
   critical paths unless the fixture context names them). Scope's git steps
   are N/A for fixture runs; fixture 13 is the exception — it IS a scope
   behavior test (the run passes iff scope refuses to call the change empty
   and inspects the working-tree diff).
2. Render `code-review/code-review-perspectives` with the constructed
   context and follow it: walk the axes over the fixture diff, record raw
   findings with file:line + verbatim evidence (no-fiction applies — evidence
   is quoted from the fixture).
3. Render `code-review/code-review-adjudicate` with the raw findings and the
   fixture spec as `change_spec`; follow it, computing each severity with the
   pinned `lisp_eval` form (the D gate — never hand-derived).
4. Record one run line (JSONL) per fixture:

```json
{"fixture_id": "01-unwrap-fallible", "run": "baseline-2026-10-09",
 "executor": "self", "decoupling": "in_thread",
 "verdict": "Request changes",
 "findings": [{"id": "CR-001", "file": "src/services/user_service.rs",
               "line_approx": "15", "severity": "Blocker",
               "mapped_class": "unwrap_fallible"}]}
```

`mapped_class` is assigned by the recorder: a finding maps to an expected
entry iff it names the same file and its evidence overlaps the planted line
range. Unmapped findings are false positives unless the fixture tolerates
them (`tolerate_below`). For fixture 13, record `"behavior_pass": true|false`
instead of findings.

5. Score: `kask/scripts/evals/code-review-score.sh runs.jsonl manifest.json`.

## Execution regimes

- `self` — the session that authored any current edits runs the fixtures.
  Label it; self-run scores are harness mechanics, not calibrated performance.
- `spawn_agent` / `fresh_session` — preferred for reported numbers: a
  decoupled executor runs the fixtures (the P1 discipline). A local agent
  card declaring the `code-review` skill can be run via
  `swarm_eval_agent_local` with one task per fixture.

## Held-out policy

The two `heldout/` fixtures are never used to tune prompts, axes, or
severity. Run them only in report-mode and record their scores alongside the
tuned set.

## Limitations (disclosed, not hidden)

- Recall is computed over distinct expected classes per fixture (true of this
  manifest; a fixture with two same-class plants would need line-level
  matching).
- The recorder's class mapping is judgment (P); the planted line ranges and
  the scoring arithmetic are mechanical (D).
- 13 scored fixtures is a smoke harness, not a benchmark: it detects
  regressions and gross changes in precision/recall, not sub-percent deltas.
  Scale by adding fixtures in the same class vocabulary.
