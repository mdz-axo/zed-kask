# hKask Security Regression Library

Every confirmed security finding becomes a permanent, CI-enforced regression test.
This is a **human-curated checklist** — the `kali-audit` skill (Track C) proposes
entries, but humans review and merge them. The "evolving" property comes from the
library growing over time, not from autonomous learning.

## Format

Each regression is a YAML file named `RR-NNNN.yaml` (zero-padded, monotonically
incrementing). The schema:

```yaml
id: RR-0001                          # matches filename
title: "short description"
surface: code | template | supply-chain | mcp | config | runtime   # which kali-audit surface (runtime = runtime-posture-monitor)
cwe: CWE-XXX                         # MITRE CWE classification (if applicable)
owasp_llm_2025: LLMXX                  # OWASP LLM Top 10 2025 risk (if applicable)
atlas_tactic: AML.TAXXXX               # MITRE ATLAS tactic (if applicable)
discovered_in: path/to/file          # where the bug was found
discovered_by: kali-audit | manual   # who found it
discovered_at: YYYY-MM-DD
severity: critical | high | medium | low
detection:
  kind: grep | cargo-test | skill-probe | reg-span
  pattern: "regex or test name"      # for grep: regex; for cargo-test: test path; for reg-span: span target pattern
  include: "glob pattern"            # for grep: file scope; for reg-span: observation window
mitigation: "what the fix looks like"
ci_gate: scripts/check-kali-regressions.sh  # the script that enforces it
status: pending | enforced           # pending = known bug, not yet fixed; enforced = fixed, CI catches re-introduction
```

## Status lifecycle

1. **pending** — bug found, not yet fixed. The regression is recorded but the CI
   gate does not fail (ratcheted). This prevents blocking the build while the fix
   is in progress.
2. **enforced** — bug fixed. The CI gate now fails if the pattern re-appears.
   Flip the status after the fix lands.
3. **deferred** — the regression is acknowledged but not mechanically enforced
   because the required infrastructure is absent. The note field must name the
   missing infrastructure (e.g., 'reg-span kind requires runtime REG span
   history infrastructure not yet implemented'). A deferred regression is a
   documented gap, not a silent pass.
4. **obsolete** — the machinery the regression checked was deleted. The note
   field records what was deleted and when (e.g., 'hkask-guard crate deleted
   2026-08-10'). The entry is retained as historical record.
5. **retired** — the regression was de-advertised rather than deployed: the
   defense layer it checked for was never present in this repo, and the
   catalog references were removed. The note field records the decision.

## CI integration

`scripts/check-kali-regressions.sh` runs all `grep`-kind regressions with
`status: enforced`. Ratcheted: `pending` regressions are warnings, not failures.

`detection.kind: reg-span` regressions (surface: runtime) are acknowledged but
NOT mechanically enforced — they require runtime REG span history infrastructure
that `scripts/check-kali-regressions.sh` does not provide (its header says so).
A reg-span regression with `status: enforced` claims CI enforcement the script
does not deliver; flip such entries to `status: deferred` with a note naming the
missing infrastructure. Do not leave a reg-span entry as `enforced` — that is a
false claim of CI coverage.

## Relationship to security skills

Multiple security skills consume this library as input and propose new
entries as output:

- **`kali-audit`** — consumes the library to avoid re-finding known issues;
  proposes new entries for code/template/MCP/supply-chain/LLM I/O findings.
- **`supply-chain-sentinel`** — proposes `surface: supply-chain` entries for
  dependency manifest findings (version pinning, registry verification,
  license conflicts, SBOM visibility).
- **`runtime-posture-monitor`** — proposes `surface: runtime` entries for
  runtime threat findings (endpoint abuse, bot traffic, LLM usage anomalies).
  Uses `kind: reg-span` detection (not `kind: grep`).
- **`attack-taxonomy-mapper`** — adds `taxonomy_mapping` field to existing
  `surface: supply-chain` entries (OSC&R tactic + technique mapping).

Humans review, merge, and the library grows. The "evolving" property comes
from the library growing over time, not from autonomous learning.
