---
name: attack-taxonomy-mapper
visibility: public
description: >
  Attack taxonomy mapping skill for hKask (v0.31.0). Consumes findings from
  supply-chain-sentinel (CWE-mapped manifest evidence) and kali-audit
  (OWASP/ATLAS-mapped findings) as input — does NOT generate new findings.
  Maps each finding to the OSC&R attack taxonomy (dependency confusion,
  typosquatting, malicious commit injection, build pipeline compromise,
  unmaintained component, unverified registry). Adds an optional,
  backward-compatible `taxonomy_mapping` field to existing `surface:
  supply-chain` regression YAML entries. Emits reg.taxonomy.* spans (P9 —
  all registered in CANONICAL_NAMESPACES). P8 evidence-backed: every
  mapping includes concrete evidence (finding reference, CWE category,
  OSC&R tactic + technique). P12 userpod_host mandatory. Decomposed into
  4 phases matching bug-hunt and supply-chain-sentinel pipeline. Minimal
  (P5): answers all 5W1H; single skill, no bundle; complements
  supply-chain-sentinel (adds taxonomy layer) and adversarial-red-team
  (parallel taxonomy discipline: ATLAS for LLM, OSC&R for supply chain —
  zero overlap).
---

# Attack Taxonomy Mapper

{# goal: Consume findings from supply-chain-sentinel and kali-audit (P4 — consumes workspace findings, no external download). Map each finding to OSC&R attack taxonomy (verified against github.com/pbom-dev/OSCAR matrix.json). Propose backward-compatible taxonomy_mapping field additions to existing surface: supply-chain regression entries (status: pending, concrete OSC&R tactic + technique). Emit reg.taxonomy.* spans (P9). Compute convergence metric from real mapping evidence only. No synthetic findings; no invented OSC&R tactics/techniques; userpod_host mandatory (P12). #}

Attack taxonomy mapping. Consumes findings from `supply-chain-sentinel`
(CWE-mapped manifest evidence) and `kali-audit` (OWASP/ATLAS-mapped
findings) as concrete evidence. Maps each finding to the OSC&R attack
taxonomy (verified against `github.com/pbom-dev/OSCAR` `matrix.json` —
organized by tactics + techniques, NOT numeric IDs). Proposes backward-
compatible `taxonomy_mapping` field additions to existing
`surface: supply-chain` regression entries. Tracks OSC&R tactic coverage
and computes a taxonomy mapping convergence metric.

**Important:** This skill does NOT generate new findings. It consumes
existing findings from `supply-chain-sentinel` and `kali-audit` and adds
a taxonomy mapping layer. The OSC&R tactic and technique names are
VERIFIED against the live OSC&R framework (`github.com/pbom-dev/OSCAR`
`matrix.json`, verified 2026-07-18). OSC&R uses tactic + technique names,
NOT numeric IDs.

## When to Use

- Mapping supply chain findings to structured attack taxonomy (OSC&R).
- Adding taxonomy context to existing `surface: supply-chain` regressions.
- Investigating incidents with structured attack-pattern taxonomy.
- Producing pattern signatures for detecting similar supply chain attacks.
- Computing taxonomy coverage convergence across audit cycles.

## Design Constraints (Grounded in Project Principles)

- **P5 Essentialism (5W1H gate):** Who = supply chain attacker / threat
  actor taxonomy (the subject — the agent is the userpod host, P12);
  What = finding to map / OSC&R taxonomy entry; Where = workspace /
  regression library / CI logs; When = audit cycle / incident
  investigation; Why = P3.1 requires structured taxonomy for supply
  chain threats (like MITRE ATLAS for LLM); P8 semantic grounding
  (findings need taxonomy context for severity and remediation priority);
  How = discover findings → read evidence → map to OSC&R → taxonomize →
  propose taxonomy_mapping field → emit Regulation span → compute convergence.
  All 6 present — passes gate.
- **P5.1 Registry canonical:** Registry (`manifest.yaml` + `.j2`) is
  source of truth. SKILL.md derived from it.
- **P5.3 Minimalist test:** No new findings generated; no external
  taxonomy download; consumes existing workspace findings only (P4
  boundary — consumes findings, does not generate them).
- **P5.4 Dual-axis:** Each mapping has state identity (finding reference +
  CWE category) and process identity (`map-taxonomy` flow).
- **P7 Evolutionary:** Pattern signatures compound value over time —
  future audits can detect similar attack patterns automatically.
- **P8 Semantic grounding:** Every mapping: finding reference, CWE
  category, OSC&R tactic + technique (verified), evidence snippet, source
  citation (`github.com/pbom-dev/OSCAR`, MITRE CWE,
  supply-chain-sentinel SKILL.md, kali-audit SKILL.md). No fabricated
  findings or invented OSC&R tactics/techniques.
- **P9 Regulation regulation:** Emits `reg.taxonomy.select`, `reg.taxonomy.map`,
  `reg.taxonomy.report`, `reg.taxonomy.convergence` spans. All four are
  registered in `CANONICAL_NAMESPACES` (`crates/hkask-types/src/event.rs`)
  and emitted unconditionally.
- **P10 Bot/userpod taxonomy:** `visibility: public` — transparent
  taxonomy mapping.
- **P11 Visibility:** `taxonomy_mapping` field proposals default `status:
  pending` (human-curated ratchet, per `security/regressions/README.md`).
- **P12 Authenticated host mandate:** Every action includes `userpod_host`.
- **P3.1 Safety floor:** Structured supply chain taxonomy protects the
  Generative Space container — unstructured findings leave attack patterns
  ambiguous.
- **P4 OCAP boundaries:** Consumes existing workspace findings only; no
  external taxonomy download; no external package download.

## Instructions

### attack-taxonomy-mapper/select-evidence

1. Discover evidence sources in the workspace: `supply-chain-sentinel`
   findings (CWE-mapped manifest evidence), `kali-audit` findings with
   `surface: supply-chain` (OWASP/ATLAS-mapped), CI logs, manifest
   evidence.
2. If zero findings to map, return empty `findings_to_map` (do NOT invent
   findings) and recommend running `supply-chain-sentinel` first.
3. Read `security/regressions/` for entries with `surface: supply-chain`
   and any existing `taxonomy_mapping` field. List
   `existing_taxonomy_mappings` (skipping entries that already have
   mappings when proposing new ones).
4. Return JSON: `{source, findings_to_map: [...], existing_taxonomy_mappings:
   [...], evidence_sources: [...], userpod_host}`.
5. Emit `reg.taxonomy.select` Regulation span (P9) with discovered evidence
   sources, findings to map, existing mapping count, host identity,
   latency metric.

### attack-taxonomy-mapper/map-taxonomy

1. Read each finding in `findings_to_map`. Extract: finding reference
   (regression ID or finding ID), CWE category, manifest path, evidence
   snippet, severity.
2. For each finding, determine the OSC&R taxonomy category via CWE
   disambiguation:
   - CWE-829 + git/path dependency → dependency confusion / typosquatting /
     unverified registry (disambiguate by evidence pattern).
   - CWE-1104 + no recent registry updates → unmaintained component.
   - CWE-1357 + CI workflow untrusted action → build pipeline compromise.
   - CWE-829 + git dependency unverified commit SHA → malicious commit
     injection.
3. Apply pragmatic-cybernetics (embedded in instructions — like `bug-hunt`
   `oracle` phase):
   - IS vs OUGHT: describe what the finding IS (CWE + evidence) vs what it
     OUGHT to map to (OSC&R tactic + technique).
   - Epistemic mode: `Declarative` (finding observed), `Probabilistic`
     (OSC&R inference from CWE + pattern), `Subjunctive` (alternative
     mappings — labeled clearly).
   - Provenance: `Direct measurement` (read finding), `Inference` (CWE →
     OSC&R disambiguation).
4. Apply grill-me self-challenge: Could this finding map to a different
   OSC&R technique? Is the CWE → OSC&R mapping ambiguous? Would a reviewer
   dispute? If yes, note alternative mappings and downgrade confidence.
5. Apply pragmatic-cybernetics analysis (feedback loops):
   - Feedback loop: does the OSC&R attack pattern have a corresponding
     defense layer in `supply-chain-sentinel`?
   - Variety: alternative attack vectors in the same OSC&R tactic?
   - Good Regulator: is the defense layer mapped to the correct OSC&R
     technique?
6. For each mapping, produce structured result:
   `finding_reference`, `cwe_category`, `evidence_snippet`,
   `osc_r_tactic` (verified name), `osc_r_technique` (verified name),
   `osc_r_categories` (verified tags), `mapping_confidence`
   (confirmed/probable/possible), `alternative_mappings`,
   `defense_layer_mapped`, `provenance`, `epistemic_mode`,
   `userpod_host`.
7. Emit `reg.taxonomy.map` Regulation span per mapping (`target:
   "reg.taxonomy.map"`, message: `"Regulation"`, operation: `"map_taxonomy"`,
   finding_reference, osc_r_tactic, osc_r_technique,
   mapping_confidence, userpod_host, latency_ms).

CONSTRAINT — Evidence integrity (P8):
- No synthetic findings. Every `evidence_snippet` must be verifiable by
  reading the cited finding from `supply-chain-sentinel` or `kali-audit`
  output or `security/regressions/` YAML.
- No invented OSC&R tactics or techniques. Only map to existing entries
  in the OSC&R framework (verified against `github.com/pbom-dev/OSCAR`
  `matrix.json`, 2026-07-18). OSC&R uses tactic + technique names, NOT
  numeric IDs.
- Source citations must reference concrete URLs or documents actually
  consulted: OSC&R framework (`github.com/pbom-dev/OSCAR`), MITRE CWE
  definitions, supply-chain-sentinel SKILL.md, kali-audit SKILL.md.
- Every mapping must include `userpod_host` identity (P12) — no
  anonymous taxonomy mapping.
- This skill complements `supply-chain-sentinel` (adds taxonomy layer to
  its findings) and `kali-audit` (parallel taxonomy discipline). State
  relationship explicitly in reports.
- Minimal (P5): 4 templates, no bundle, no sub-agent delegation. Each
  template answers specific 5W1H: select (Where), map (What + How),
  taxonomize (Why + What), convergence (When + Why).

### attack-taxonomy-mapper/taxonomize

1. Synthesize `mappings` array from `map-taxonomy` phase. Group by OSC&R
   category: dependency_confusion, typosquatting, malicious_commit_injection,
   build_pipeline_compromise, unmaintained_component, unverified_registry.
2. For each mapping with confidence >= probable and concrete evidence,
   propose `taxonomy_mapping` field addition to the corresponding regression
   entry. Format (backward-compatible optional field):
   ```yaml
   taxonomy_mapping:
     osc_r_tactic: "Initial Access"      # verified OSC&R tactic
     osc_r_technique: "Dependency confusion"  # verified OSC&R technique
     osc_r_categories: ["ArtifactSecurity", "ContainerSecurity", "OpenSourceSecurity"]  # verified tags
   ```
3. Produce pattern signatures for each OSC&R technique — concrete grep
   patterns for detecting similar attack patterns in future manifests.
4. Identify top 3 taxonomy coverage gaps (OSC&R tactics with no mapped
   findings).
5. Produce verdict:
   - Pass: all findings mapped, >= 4 OSC&R categories covered.
   - Conditional: some findings unmapped, 2-3 OSC&R categories covered.
   - Fail: majority unmapped, < 2 OSC&R categories covered.
6. Emit `reg.taxonomy.report` Regulation span with mappings count by OSC&R
   category, proposed taxonomy_mapping count, pattern signatures count,
   verdict, userpod_host, latency.

### attack-taxonomy-mapper/convergence-check

1. Compute normalized convergence metric [0, 1] where 0 = fully converged.
2. Score dimensions (weighted):
   - Unmapped findings (critical/high) (0.45): 0 = +0.00; 1+ = +0.45.
   - OSC&R tactic coverage (0.30): all 12 tactics = +0.00; partial = +0.05
     per missing tactic; 0 = +0.30.
   - Regression `taxonomy_mapping` field growth (0.15): new field proposed
     = +0.00; stagnation = +0.15.
   - Residual unmapped risk (0.10): zero unmapped = +0.00; any = +0.10.
3. Start at 0.00, add contributions, clamp to [0, 1].
4. Converged: metric ≤ 0.10 AND relative improvement ≥ 5% from previous
   cycle.
5. Return JSON: `{convergence_metric, dimensions, rationale, blockers,
   oscr_categories_covered, oscr_categories_missing, existing_taxonomy_mappings,
   proposed_taxonomy_mappings, reg_span_emitted: true}`.
6. Emit `reg.taxonomy.convergence` Regulation span (registered in
   `CANONICAL_NAMESPACES` — emitted unconditionally).

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `select-evidence.j2` | KnowAct | Discover evidence sources; read regression library for existing taxonomy_mappings; emit `reg.taxonomy.select` span. |
| `map-taxonomy.j2` | KnowAct | Map findings to OSC&R taxonomy via CWE disambiguation; apply pragmatic-cybernetics; emit `reg.taxonomy.map` spans. |
| `taxonomize.j2` | KnowAct | Classify mappings by OSC&R tactic; produce pattern signatures; propose backward-compatible `taxonomy_mapping` field; emit `reg.taxonomy.report` span. |
| `convergence-check.j2` | KnowAct | Compute taxonomy coverage convergence metric (OSC&R tactic coverage + mapping growth). Emit `reg.taxonomy.convergence` span. |

## OSC&R Taxonomy Reference (VERIFIED against github.com/pbom-dev/OSCAR)

**Source:** `github.com/pbom-dev/OSCAR` — `matrix.json` (verified 2026-07-18).

OSC&R is organized by **tactics** (like MITRE ATT&CK), with each tactic
containing **techniques**. Techniques are tagged with supply chain
**categories** (the surface the technique targets). There are NO numeric
IDs — techniques are referenced by name within tactics.

### Verified tactic → technique → CWE mapping

| OSC&R Tactic | OSC&R Technique (verified) | CWE Mapping | Detection Pattern |
|--------------|---------------------------|-------------|-------------------|
| Initial Access | Dependency confusion | CWE-829 | Package name exists in both private and public registry |
| Initial Access | Typosquatting | CWE-829 | Dependency name differs from canonical by ≤2 chars (Levenshtein) |
| Initial Access | Combosquatting | CWE-829 | Dependency name contains legitimate package name as substring |
| Initial Access | Brandjacking | CWE-829 | Dependency name mimics legitimate publisher/brand |
| Initial Access | Repojacking | CWE-829 | Git dependency URL points to repo whose ownership changed |
| Initial Access | Vulnerability in third-party dependency | CWE-1104 | Dependency with known vulnerability in manifest metadata |
| Initial Access | Vulnerable CICD system | CWE-1357 | CI workflow references vulnerable CICD system version |
| Initial Access | Vulnerable CICD plugins | CWE-1357 | CI workflow references vulnerable CICD plugin version |
| Initial Access | Vulnerable CICD template | CWE-1357 | CI workflow references vulnerable CICD template |
| Resource Development | Malicious code contribution to an open-source repository | CWE-829 | Git dependency references unverified commit SHA |
| Resource Development | Publish malicious artifact | CWE-829 | Dependency published by unverified account to public registry |
| Resource Development | Compromised legitimate artifact | CWE-829 | Dependency version differs from publisher's legitimate release |
| Resource Development | Accounts in public registry | CWE-829 | Dependency source is `git = ...` or `path = ...` without registry reference |
| Execution | Trigger pipeline execution | CWE-1357 | CI workflow triggers on untrusted event source |
| Persistence | Backdoor in code | CWE-829 | Dependency contains known backdoor pattern |
| Privilege Escalation | Inject malicious dependency to privileged user repository | CWE-829 | Dependency injected into repo with elevated permissions |

### OSC&R supply chain categories (tags)

Techniques are tagged with one or more of these supply chain categories:

| Category | Description |
|----------|-------------|
| ArtifactSecurity | Security of build artifacts (binaries, packages) |
| CI/CDPosture | CI/CD pipeline security posture |
| CloudSecurity | Cloud infrastructure security |
| CodeSecurity | Source code security |
| ContainerSecurity | Container image security |
| Infrastructureascode | IaC (Terraform, etc.) security |
| OpenSourceSecurity | Open-source dependency security |
| SCMPosture | Source Code Management (Git) posture |
| SecretsHygiene | Secrets management hygiene |

New OSC&R tactics/techniques can be added as real findings justify them
(P7) — not speculatively. This taxonomy reference is distinct from
`kali-audit`'s OWASP LLM / ATLAS catalog and `supply-chain-sentinel`'s
CWE catalog.

## Relationship to Existing Skills

- **`supply-chain-sentinel`:** `supply-chain-sentinel` performs manifest-
  level audit and proposes regressions with CWE mappings.
  `attack-taxonomy-mapper` consumes those findings and adds OSC&R
  taxonomy layer. Complementary — finding generation vs taxonomy
  mapping. Zero overlap.
- **`kali-audit`:** `kali-audit` maps to OWASP LLM / ATLAS for LLM/code.
  `attack-taxonomy-mapper` maps to OSC&R for supply chain. Parallel
  taxonomy discipline, distinct taxonomies. Zero overlap.
- **`adversarial-red-team`:** Uses MITRE ATLAS for LLM adversarial.
  `attack-taxonomy-mapper` uses OSC&R for supply chain adversarial. Same
  taxonomy discipline, different domain. Zero overlap.
- **`bug-hunt`:** Provides decomposed pipeline structure (`Charter` →
  `Probe` → `Oracle` → `Taxonomize` → `Report`). This skill replicates
  that structure (`select-evidence` ≈ charter; `map-taxonomy` ≈ probe +
  oracle; `taxonomize` ≈ taxonomize + report; `convergence-check` ≈
  convergence). Uses same pragmatic-cybernetics and pragmatic-semantics
  reasoning embedded in instructions.
- **`attack-taxonomy-mapper` does NOT replace any of these:** It fills
  the taxonomy mapping gap. No existing skill maps supply chain findings
  to the OSC&R attack taxonomy.

## Constraints (Concrete — Not Aspirational)

- `select-evidence.j2`: `visibility: public`.
- `map-taxonomy.j2`: `visibility: public`.
- `taxonomize.j2`: `visibility: public`.
- `convergence-check.j2`: `visibility: public`.
- Every mapping includes concrete finding reference, CWE category, OSC&R
  category, evidence snippet, source citation — not
  summary description.
- Every proposed `taxonomy_mapping` field uses backward-compatible
  optional YAML format with `osc_r` keys.
- No synthetic findings — only map findings from `supply-chain-sentinel`
  or `kali-audit`.
- No invented OSC&R tactics or techniques — only map to existing entries in the OSC&R framework (verified against `github.com/pbom-dev/OSCAR` `matrix.json`, 2026-07-18). OSC&R uses tactic + technique names, NOT numeric IDs.
- No fabricated mappings; read finding evidence before mapping.
- Registry (`manifest.yaml` + `.j2`) is authoritative over this SKILL.md
  (P5.1).
- Do NOT invent OSC&R tactic or technique names not in the OSC&R framework (verified at `github.com/pbom-dev/OSCAR` `matrix.json`).
- Do NOT claim taxonomy coverage that hasn't been verified through actual
  finding mapping.
- Every mapping action includes `userpod_host` identity (P12).
- Every taxonomy mapping operation emits `reg.taxonomy.*` span. All four
  namespaces are registered in `CANONICAL_NAMESPACES`
  (`crates/hkask-types/src/event.rs`) and emitted unconditionally.
- Apply pragmatic-cybernetics feedback loop analysis: mapping polarity,
  variety of OSC&R categories, Good Regulator (defense layer mapped
  correctly?).
- Apply `grill-me` self-challenge before proposing mappings.
- Apply `IS/OUGHT` classification and label `epistemic_mode` and
  `provenance` for every mapping.
- Convergence metric computed from real evidence: unmapped findings (0.45),
  OSC&R coverage (0.30), mapping growth (0.15),
  residual unmapped risk (0.10).
- Do NOT fabricate mappings — only report what was discovered through
  actual finding analysis.
- Source citations must reference concrete sources: OSC&R framework
  (`github.com/pbom-dev/OSCAR`), MITRE CWE definitions (mitre.org),
  `security/regressions/README.md`, `supply-chain-sentinel` SKILL.md,
  `kali-audit` SKILL.md.
- If evidence discovery finds zero findings to map, return empty
  `findings_to_map` and recommend running `supply-chain-sentinel` first —
  do NOT invent findings.
- Before proposing any `taxonomy_mapping` field, verify the finding
  reference exists and evidence snippet can be quoted from the actual
  finding.
- This skill does NOT generate new findings or download external packages.
  It maps existing findings to taxonomy entries only (P4 boundary —
  consumes workspace findings, no external taxonomy download).
- The `taxonomy_mapping` field is backward-compatible (optional) — existing
  regressions without it remain valid. `scripts/check-kali-regressions.sh`
  should not break (verify before first `status: enforced` flip).
- OSC&R tactic and technique names are VERIFIED against the live OSC&R
  framework (`github.com/pbom-dev/OSCAR` `matrix.json`, verified
  2026-07-18). OSC&R uses tactic + technique names, NOT numeric IDs.

## Source References and Taxonomy Anchors

This skill is anchored to concrete, verifiable taxonomy sources (P8):

- **OSC&R Framework:** Open Software Supply Chain Attack Reference —
  ATT&CK-like taxonomy for supply chain threats. Organized by **tactics**
  (Initial Access, Resource Development, Execution, etc.) containing
  **techniques** (Dependency confusion, Typosquatting, etc.). Techniques
  are tagged with supply chain **categories** (OpenSourceSecurity,
  ContainerSecurity, CI/CDPosture, etc.). There are NO numeric IDs —
  techniques are referenced by name within tactics. Source:
  `github.com/pbom-dev/OSCAR` — `matrix.json` (verified 2026-07-18).
- **MITRE CWE:** CWE-1104 (Unmaintained Third-Party Components), CWE-829
  (Inclusion from Untrusted Control Sphere), CWE-1357 (Reliance on
  Component Not Updateable). Source: `mitre.org/data/definitions/`.
- **`supply-chain-sentinel` SKILL.md:** Source of findings to map
  (CWE-mapped manifest evidence).
- **`kali-audit` SKILL.md:** Source of OWASP/ATLAS-mapped findings;
  parallel taxonomy discipline reference.
- **`adversarial-red-team` SKILL.md:** Parallel taxonomy discipline
  reference (ATLAS for LLM, OSC&R for supply chain).
- **`bug-hunt` SKILL.md:** `taxonomize` phase pattern (classify findings
  into taxonomy, assign severity, produce pattern signatures).
- **`security/regressions/README.md`:** Regression YAML format — the
  `taxonomy_mapping` field extends this format (backward-compatible).
