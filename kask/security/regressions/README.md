# hKask Training-Config Regression Library

Confirmed training-configuration defects become permanent, CI-enforced
regression tests. This is a **human-curated checklist** — the `lora-training`
skill proposes entries, but humans review and merge them.

> **History.** This directory previously held the `kali-audit` security
> regression library (`surface: code | template | supply-chain | mcp | config |
> runtime`). That skill and its gate (`scripts/check-kali-regressions.sh`) were
> removed on 2026-08-20; the entries were deleted with them. Source comments
> that cite an `RR-NNNN` id are historical rationale markers, not live gates.

## Format

Each regression is a YAML file named `RR-NNNN.yaml` (zero-padded, monotonically
incrementing). The schema:

```yaml
id: RR-0001                          # matches filename
title: "short description"
surface: training                    # the only enforced surface
gate: G-XX                           # the lora-training gate that owns the invariant
cwe: CWE-XXX                         # MITRE CWE classification (if applicable)
discovered_in: path/to/file          # where the defect was found
discovered_by: lora-training | manual
discovered_at: YYYY-MM-DD
severity: critical | high | medium | low
detection:
  kind: grep | cargo-test | runtime-assert
  pattern: "regex or test name"      # grep: regex; cargo-test: test name
  include: "glob pattern"            # grep: file scope
  semantics: absence | presence      # default absence
mitigation: "what the fix looks like"
source: "upstream reference (paper, docs)"
ci_gate: scripts/check-lora-training-regressions.sh
status: pending | enforced | deferred | obsolete | retired
```

## Status lifecycle

1. **pending** — defect found, not yet fixed. Recorded, but the CI gate does not
   fail (ratcheted). This keeps an in-progress fix from blocking the build.
2. **enforced** — defect fixed. The CI gate now fails if the pattern re-appears.
3. **deferred** — acknowledged but not mechanically enforced because the
   required infrastructure is absent. The `note` field must name the missing
   infrastructure. A deferred regression is a documented gap, not a silent pass.
4. **obsolete** — the machinery the regression checked was deleted. The `note`
   field records what was deleted and when.
5. **retired** — de-advertised rather than deployed: the defense it checked for
   was never present in this repo. The `note` field records the decision.

## CI integration

`scripts/check-lora-training-regressions.sh` runs every `surface: training`
entry with `status: enforced` and `detection.kind: grep` or `cargo-test`.
Ratcheted: `pending` entries are warnings, not failures.

`detection.kind: runtime-assert` entries are acknowledged but NOT mechanically
enforced — they need instrumentation during a training run that CI does not
have. The gate prints them as `deferred` so the gap stays visible.

Run it from the `kask/` directory:

```
bash scripts/check-lora-training-regressions.sh
```
