---
name: supply-chain-sentinel
visibility: public
description: >
  Dependency and software supply chain audit skill for hKask (v0.31.0).
  Probes dependency manifests (Cargo.toml, deny.toml, package-lock.json,
  go.sum) for version pinning, registry verification, license conflicts,
  unmaintained indicators, and transitive dependency visibility.
  Anchored to MITRE CWE (CWE-1104, CWE-829, CWE-1357), OWASP Supply
  Chain taxonomy, OSC&R framework (Open Software Supply Chain Attack
  Reference — `github.com/pbom-dev/OSCAR`).
  Consumes security/regressions/; proposes RR-NNNN.yaml entries with
  surface: supply-chain. Emits reg.supply_chain.* spans (P9). Decomposed
  into 4 phases matching bug-hunt and kali-audit pipeline.
  Minimal (P5): answers all 5W1H; single skill, no bundle; complements
  kali-audit (deeper supply-chain focus) and adversarial-red-team (LLM
  boundary — zero overlap).
---

# Supply-Chain Sentinel

{# goal: Audit dependency manifest declarations (Cargo.toml, deny.toml, lockfiles) within user workspace boundaries (P4 OCAP). Verify version pinning, registry trust, license compatibility, SBOM visibility. Map findings to MITRE CWE-1104/CWE-829/CWE-1357, OWASP Supply Chain, OSC&R taxonomy. Propose concrete RR-NNNN.yaml entries (surface: supply-chain, status: pending, concrete grep pattern against manifest content). Emit reg.supply_chain.* spans (P9). Compute convergence metric from real manifest evidence only. No synthetic CVE claims; no external dependency download; userpod_host mandatory (P12). #}

Dependency and software supply chain audit. Reads workspace manifest files (manifest-level dependency tree only — no external package download, P4 boundary enforcement).
(`Cargo.toml`, `deny.toml`, lockfiles) as concrete evidence. Maps findings
to MITRE CWE / OWASP Supply Chain / OSC&R taxonomy. Proposes CI-enforced
regressions (`surface: supply-chain`). Tracks defense-layer coverage
(pinning, registry trust, SBOM visibility, license compatibility) and
computes a supply-chain convergence metric.

## When to Use

- Auditing dependency integrity before deploying new crates/packages.
- Investigating manifest-level supply chain signals (`deny.toml` gaps,
unpinned versions, missing lockfile tracking).
- Verifying defense-layer presence for supply chain (P9 observable via
`reg.supply_chain.*` spans).
- Proposing `security/regressions/` entries backed by manifest evidence.
- Computing supply-chain-specific convergence across audit cycles.

## Design Constraints (Grounded in Project Principles)

- **P5 Essentialism (5W1H gate):** Who = dependency maintainer / userpod
  host (P12); What = manifest entry / dependency graph; Where = workspace
  file / registry URL; When = scan cycle / manifest version; Why = P3.1
  safe container / P1 user sovereignty over dependencies / P4 explicit
  dependency boundaries; How = discover → read → verify → classify →
  report → propose regression → emit Regulation span → compute convergence. All 6
  present — passes gate.
- **P5.1 Registry canonical:** Registry (`manifest.yaml` + `.j2`) is source
  of truth. SKILL.md derived from it.
- **P5.3 Minimalist test:** No speculative dependency download; manifest
  analysis only (P4 boundary). No extra abstractions.
- **P5.4 Dual-axis:** Each finding has state identity (`Cargo.toml` line)
  and process identity (`probe` flow).
- **P7 Evolutionary:** Defense layers emerge from real manifest patterns,
  not speculation.
- **P8 Semantic grounding:** Every claim: file path, manifest line,
  dependency name+version, evidence snippet, source citation (MITRE CWE
  URL, deny.toml spec, crates.io advisory format, OWASP Supply Chain,
  OSC&R framework URL). No fabricated CVEs or synthetic quotes.
- **P9 Regulation regulation:** Emits `reg.supply_chain.select`, `reg.supply_chain.probe`,
  `reg.supply_chain.report`, `reg.supply_chain.convergence` spans. All four
  are registered in `CANONICAL_NAMESPACES` (`crates/hkask-types/src/event.rs`)
  and emitted unconditionally.
- **P10 Bot/userpod taxonomy:** `visibility: public` — transparent audit.
- **P11 Visibility:** Regression proposals default `status: pending`
  (human-curated ratchet, per `security/regressions/README.md`).
- **P12 Authenticated host mandate:** Every action includes `userpod_host`.
- **P3.1 Safety floor:** Supply chain integrity protects the Generative
  Space container.
- **P4 OCAP boundaries:** Reads only declared workspace manifest paths;
  no ambient dependency scanning; no external package download without
  explicit consent (P2).

## Instructions

### supply-chain-sentinel/select-surface

1. Discover dependency surfaces in workspace root: `Cargo.toml`,
   `Cargo.lock`, `package-lock.json`, `go.sum`, `deny.toml`,
   `go.mod`, `go.sum`, `requirements.txt`.
2. If zero manifest files found, return empty `manifest_paths` (do NOT
   invent files) and recommend `surface: cargo` or `surface: deny` based
   on workspace evidence (`.rs` source presence, `rust-toolchain`).
3. Read `security/regressions/` for entries with `surface: supply-chain`.
   List `existing_regressions` (skipping `enforced` duplicates when
   proposing new entries).
4. Verify defense layers to check: `dependency_pinning` (exact version +
   lockfile reference), `registry_trust` (official registry URL, not
   arbitrary git/path), `license_compatibility` (`deny.toml` reference),
   `sbom_presence` (manifest or lockfile tracks dependency metadata).
5. Return JSON: `{surface, manifest_paths: [...], registry_sources: [...],
   defense_layers: [...], existing_regressions: [...], userpod_host}`.
6. Emit `reg.supply_chain.select` Regulation span (P9) with discovered files,
   surface selection, regression count, defense layers, host identity,
   latency metric.

### supply-chain-sentinel/probe

1. Read each file in `manifest_paths`. Quote manifest lines for evidence
   (not synthetic). Record concrete line numbers.
2. For `Cargo.toml`: extract dependency entries. Note version spec:
   exact (`=1.2.3`), caret (`^1.0`), bounded range (`>=0.5, <2.0`),
   unbounded (`*`, `>=0.5` without upper), git/path (`git = ...`,
   `path = ...`).
3. For `deny.toml`: read `advisory` and `license` sections. Note conflicts:
   dependency with banned license reference; advisory deny without
   matching manifest reference; missing license reference for dependency.
4. For lockfiles (`Cargo.lock`, `package-lock.json`, `go.sum`): verify
   that manifest dependencies have transitive tracking. Note missing
   transitive visibility (`defense_layers_missing`).
5. Apply pragmatic-cybernetics (embedded in instructions — like
   `bug-hunt` `oracle` phase):
   - IS vs OUGHT: describe manifest content (`IS`) vs security posture
     (`OUGHT` — pinned, verified, licensed, tracked).
   - Epistemic mode: `Declarative` (file read), `Probabilistic` (version
     age inference — only when manifest provides version metadata
     explicitly), `Subjunctive` (potential supply-chain risk — labeled
     clearly, not presented as fact).
   - Provenance: `Direct measurement` (read file), `Inference`
     (version analysis), `Assessment` (security taxonomy mapping) —
     label each finding explicitly.
6. Apply grill-me self-challenge: Could this dependency spec be
   intentional? Would a reviewer dismiss? If yes, downgrade or omit.
   Only propose concrete findings with quoted manifest evidence.
7. Apply pragmatic-cybernetics analysis (feedback loops): trace
   dependency update polarity (does newer version reduce risk?), check
   variety (alternative package available?), Good Regulator (is version
   pinned / lockfile enforcing stability?).
8. For each dependency entry, produce structured finding:
   `dependency`, `version_spec`, `source_line`, `manifest_path`,
   `registry`, `version_pinned` (bool), `registry_trusted` (bool),
   `license_conflict` (bool + evidence), `unmaintained_indicator`
   (bool + evidence — only when manifest provides explicit age
   indicator; never fabricated from external database), `transitive_mentioned`
   (bool + lockfile reference), `severity` (critical/high/medium/low
   — justified by evidence, not assumption), `provenance`,
   `epistemic_mode`, `defense_layers_present`, `defense_layers_missing`,
   `evidence_snippet` (quoted manifest line + file path),
   `source_citation` (MITRE CWE reference URL, deny.toml spec reference,
   crates.io advisory format, OWASP Supply Chain reference, OSC&R
   framework URL).
9. Emit `reg.supply_chain.probe` Regulation span per dependency entry probed
   (`target: "reg.supply_chain.probe"`, message: `"Regulation"`, operation:
   `"probe_dependency"`, dependency, manifest_path, registry, version_pinned,
   registry_trusted, userpod_host, latency_ms).
10. Apply pragmatic-cybernetics feedback loop analysis: dependency update
    polarity (newer version = lower/higher risk based on pinning?), variety
    of alternatives (alternative crate/package available?), Good Regulator
    check (is lockfile enforcing stability?), delay (how stale is pinned
    version relative to registry if manifest references registry?).

CONSTRAINT — Evidence integrity (P8):
- No synthetic manifest quotes. Every `evidence_snippet` verifiable by
  reading cited manifest file at cited line.
- No synthetic CVE numbers. Only reference MITRE CWE taxonomy categories:
  CWE-1104 (Unmaintained Third-Party Components), CWE-829 (Inclusion from
  Untrusted Control Sphere — git/path dependencies), CWE-1357 (Reliance
  on Component Not Updateable). These are taxonomy mappings, not
  vulnerability claims.
- Source citations must reference concrete URLs or documents actually
  consulted: MITRE CWE definitions, deny.toml specification, crates.io
  advisory database docs, OWASP Supply Chain security reference, OSC&R
  framework (`github.com/pbom-dev/OSCAR`), cargo-deny documentation.
- Every finding must include `userpod_host` identity (P12) — no anonymous
  dependency scanning.
- When referencing `security/regressions/`, read actual YAML files; do not
  invent regression entries. Only propose new entries when concrete
  evidence supports them.
- This skill complements `kali-audit` (surface-level supply chain check) by
  providing deeper dependency graph audit (version spec analysis,
  registry verification, SBOM visibility). It complements
  `adversarial-red-team` (LLM boundary — zero overlap). State relationship
  explicitly in reports.
- Minimal (P5): 4 templates (`select-surface`, `probe`, `report`,
  `convergence-check`), no bundle, no sub-agent delegation, no abstract
  dependency resolver. Each template answers specific 5W1H: select (Where),
  probe (What + How), report (Why + What), convergence (When + Why).

### supply-chain-sentinel/report

1. Synthesize `findings` array from `probe` phase. Group by severity:
   critical (unverified registry + unpinned + no lockfile + license
   conflict), high (unverified registry or unpinned with no SBOM), medium
   (unbounded version without upper bound, missing transitive tracking),
   low (minor license reference gap, no explicit advisory match).
2. For each finding: include `dependency`, `manifest_path`, `line`,
   `evidence_snippet`, `severity`, `cwe_reference` (e.g., CWE-1104),
   `taxonomy_mapping` (OWASP Supply Chain / OSC&R), `defense_layers_present`,
   `defense_layers_missing`, `remediation_recommendation` (citing concrete
   fix pattern: pin exact version, add deny.toml advisory/license entry,
   add lockfile tracking, verify registry URL), `userpod_host`.
3. Propose regression entry for findings with severity >= medium (only
   when evidence is concrete — no synthetic findings). Use exact YAML
   format from `security/regressions/README.md`:
   `surface: supply-chain`, `cwe: CWE-XXX`, `discovered_in: <manifest_path>`,
   `status: pending`, `detection: kind: grep`, `pattern: "..."` or
   `detection: kind: cargo-test | manifest-check`. Each proposal must
   include concrete `pattern` referencing manifest content (e.g., regex
   for unbounded version spec `>=` without `<`, or `git = ` dependency
   reference) — not vague description.
4. Identify defense-layer gaps (e.g., missing `dependency_pinning`, missing
   `registry_trust`, missing `license_compatibility`, missing
   `sbom_presence`). Propose top 3 highest-priority fixes based on severity.
5. Produce verdict:
   - Pass: zero critical/high findings, >= 3 defense layers present.
   - Conditional: medium findings present or 2 defense layers missing.
   - Fail: critical/high findings present or < 2 defense layers present.
6. Emit `reg.supply_chain.report` Regulation span with findings count by
   severity, defense layers present/missing, proposed regression count,
   userpod host, verdict, latency metric.

### supply-chain-sentinel/convergence-check

1. Compute normalized convergence metric [0, 1] where 0 = fully converged.
2. Score dimensions (weighted):
   - Critical + high findings resolved (0.40): 0 critical/high = +0.00;
     1+ critical/high unresolved = +0.40; partial resolution = proportional.
   - Defense-layer coverage (0.25): 4 layers present = +0.00; 3 = +0.06;
     2 = +0.12; 1 = +0.19; 0 = +0.25.
   - CWE / taxonomy coverage (0.15): CWE-1104, CWE-829, CWE-1357
     covered by at least one finding/proposed regression = +0.00; 1-2
     covered = +0.08; 0 covered = +0.15.
   - Regression library growth (0.10): new `surface: supply-chain`
     regression proposed and accepted in current cycle = +0.00; no new
     regression proposed despite evidence = +0.10 (stagnation).
   - Residual dependency risk (0.10): unpinned/unverified/untracked
     dependencies remaining = +0.10; all verified and pinned = +0.00.
3. Start at 0.00, add contributions, clamp to [0, 1].
4. Converged: metric ≤ 0.10 AND relative improvement ≥ 5% from previous
   cycle. If metric has not improved by ≥5%, identify blocker (missing
   defense layer, unfixed finding, no regression growth, evidence gap).
5. Return JSON: `{convergence_metric, dimensions, rationale, blockers,
   defense_layers_present, defense_layers_missing, existing_regressions,
   proposed_regressions}`.
6. Emit `reg.supply_chain.convergence` Regulation span (registered in
   `CANONICAL_NAMESPACES` — `crates/hkask-types/src/event.rs`).

## Registry Templates

| Template | Type | Purpose |
|----------|------|----------|
| `select-surface.j2` | KnowAct | Discover manifest surfaces; read regression library; emit `reg.supply_chain.select` span. |
| `probe.j2` | KnowAct | Read manifest evidence; verify dependency specs; apply pragmatic-cybernetics; emit `reg.supply_chain.probe` spans. |
| `report.j2` | KnowAct | Synthesize findings with CWE/OWASP/OSC&R taxonomy; propose `RR-NNNN.yaml` entries (`surface: supply-chain`); emit `reg.supply_chain.report` span. |
| `convergence-check.j2` | KnowAct | Compute supply-chain-specific convergence metric (defense-layer coverage + regression growth + residual risk). Emit `reg.supply_chain.convergence` span. |

## Defense-Layer Catalog (Supply Chain Specific)

| Layer | Name | Evidence Source | Source Citation |
|-------|------|-----------------|-----------------|
| 1 | Dependency pinning | Exact version (`=...`) + lockfile reference | `Cargo.lock`, `package-lock.json` |
| 2 | Registry verification | Official registry URL (`crates.io`, `npm`, `pypi`); not arbitrary `git`/`path` | `Cargo.toml` registry field; `deny.toml` advisory source |
| 3 | License compatibility | `deny.toml` license section; manifest license metadata | `deny.toml` spec; crates.io license metadata |
| 4 | SBOM / transitive visibility | Lockfile references transitive entries; dependency graph depth visible | `Cargo.lock`; `go.sum`; `package-lock.json` |

New layers can be added as real manifest patterns justify them (P7) —
not speculatively.

## Relationship to Existing Skills

- **`kali-audit`:** `kali-audit` covers `surface: supply-chain` at manifest-
  discovery and advisory-check level (`deny.toml`, cargo-audit). This skill
  provides deeper dependency graph audit: version spec analysis (exact vs
  unbounded), registry verification (official vs git/path), transitive
  dependency visibility (lockfile depth), license conflict detection (deny
  rules vs manifest entries), defense-layer coverage metric specific to
  supply chain (4 layers vs 8 for LLM/code). They are complementary —
  `kali-audit` proposes supply-chain regressions; this skill performs
  deeper audit and proposes its own `surface: supply-chain` entries with
  concrete manifest line evidence. Both consume `security/regressions/`
  as input; both propose new entries for human review.
- **`adversarial-red-team`:** Covers LLM I/O adversarial robustness
  (prompt injection, exfiltration, 7 attack categories, persistence modes).
  Zero overlap with dependency manifest security. They address different
  threat surfaces: `adversarial-red-team` = dynamic LLM boundary; this skill
  = static dependency supply chain.
- **`bug-hunt`:** Provides decomposed pipeline structure (`Charter` →
  `Probe` → `Oracle` → `Taxonomize` → `Report`). This skill replicates
  that structure (`select-surface` ≈ charter; `probe` ≈ probe + oracle;
  `report` ≈ taxonomize + report; `convergence-check` ≈ convergence).
  Uses same pragmatic-cybernetics and pragmatic-semantics reasoning
  embedded in instructions (`IS/OUGHT`, `epistemic mode`, `provenance`,
  `grill-me` self-challenge). This skill applies those patterns to
  dependency manifest analysis rather than runtime code probing.
- **`supply-chain-sentinel` does NOT replace any of these:** It fills the
  gap between `kali-audit` (broad surface audit including supply chain at
  shallow depth) and external dependency scanners (Snyk SCA, Semgrep SSC,
  Trivy) by providing a native hKask audit mechanism: manifest reading,
  taxonomy mapping (MITRE CWE / OWASP / OSC&R), regression proposal
  (`surface: supply-chain`), Regulation span emission (`reg.supply_chain.*`),
  and convergence tracking — all within user sovereignty (P1), consent
  (P2), generative space (P3), OCAP boundaries (P4), essentialism (P5),
  userpod space (P6), evolutionary architecture (P7), semantic grounding
  (P8), homeostatic regulation (P9), explicit taxonomy (P10), visibility
  governance (P11), and host mandate (P12).

## Constraints (Concrete — Not Aspirational)

- `select-surface.j2`: `visibility: public`.
- `probe.j2`: `visibility: public`.
- `report.j2`: `visibility: public`.
- `convergence-check.j2`: `visibility: public`.
- Every finding includes concrete file path, manifest line, dependency
  name+version, quoted evidence snippet, source citation — not summary
  description.
- Every proposed regression uses exact YAML format (`security/regressions/`)
  with `surface: supply-chain`, concrete `pattern` (grep regex against
  manifest content), `status: pending`, `cwe: CWE-XXX`.
- No synthetic manifest quotes; read file before quoting.
- No synthetic CVE references; only MITRE CWE taxonomy categories
  (CWE-1104, CWE-829, CWE-1357) as mappings, not vulnerability claims.
- No fabricated unmaintained indicators; only propose when manifest
  provides explicit version metadata or reference (e.g., `deny.toml`
  advisory reference without manifest match, or version spec referencing
  very old registry entry with explicit reference).
- Registry (`manifest.yaml` + `.j2`) is authoritative over this SKILL.md
  (P5.1).
- Do NOT invent dependency entries not present in manifest.
- Do NOT claim external package download or container scan capability —
  manifest analysis only (P4 boundary enforcement).
- Every scan action includes `userpod_host` identity (P12).
- Every security-sensitive dependency operation emits `reg.supply_chain.*`
  span. All four namespaces (`select`, `probe`, `report`, `convergence`)
  are registered in `CANONICAL_NAMESPACES` (`crates/hkask-types/src/event.rs`).
- Apply pragmatic-cybernetics feedback loop analysis: dependency update
  polarity, variety of alternatives, Good Regulator (pinning/enforcement),
  delay (version age — only when manifest provides reference).
- Apply `grill-me` self-challenge before proposing findings.
- Apply `IS/OUGHT` classification and label `epistemic_mode` and
  `provenance` for every finding.
- Convergence metric computed from real evidence: unresolved critical/high
  findings (0.40), defense-layer coverage (0.25), CWE/taxonomy coverage
  (0.15), regression library growth (0.10), residual dependency risk (0.10).
- Do NOT fabricate findings — only report what was discovered through
  actual manifest reading (like `kali-audit` constraint).
- Source citations must reference concrete sources (not aspirational):
  MITRE CWE definitions (mitre.org), deny.toml specification (embarkstudios
  / deny documentation), crates.io advisory database format, cargo-deny
  docs, OWASP Supply Chain reference, OSC&R framework
  (`github.com/pbom-dev/OSCAR`),
  `security/regressions/README.md` for regression format.
- If manifest discovery finds zero dependency files, return empty
  `manifest_paths` and recommend `surface: cargo` or `surface: deny`
  based on workspace evidence (`.rs` source, `rust-toolchain`) — do NOT
  invent manifest content.
- Before proposing any regression entry, verify manifest line exists and
  evidence snippet can be quoted from actual file content.
- This skill does NOT download dependencies or build containers. It audits
  manifest declarations within user-defined workspace boundaries (P4 OCAP
  enforcement perimeter — dependency must be explicitly declared, not
  ambient authority).
- Propose `surface: supply-chain` regression entries only; do NOT reuse
  `surface: code`, `surface: template`, `surface: mcp`, or `surface: config`
  — supply chain findings have distinct defense-layer catalog (4 layers:
  pinning, registry trust, license compatibility, SBOM visibility) distinct
  from `kali-audit`'s 8-layer LLM/code defense catalog.
- Convergence metric must reflect actual coverage, not aspirational:
  defense layers only count as present when manifest evidence confirms
  them (exact version + lockfile reference = pinning; registry URL match
  = registry trust; deny.toml license section + manifest reference =
  license compatibility; lockfile transitive references = SBOM visibility).

## Source References and Taxonomy Anchors

This skill is anchored to concrete, verifiable taxonomy sources (P8):

- **MITRE CWE:** CWE-1104 (Use of Unmaintained Third-Party Components),
  CWE-829 (Inclusion of Functionality from Untrusted Control Sphere —
  applies to `git`/`path` dependencies without registry verification),
  CWE-1357 (Reliance on Component That is Not Updateable). Source:
  `mitre.org/data/definitions/1104.html` (and related CWE pages).
- **OWASP Supply Chain:** Supply chain security taxonomy for dependency
  management. Source: OWASP Software Supply Chain Security reference
  (owasp.org/www-project-software-supply-chain-security/ or equivalent
  documentation).
- **OSC&R Framework:** Open Software Supply Chain Attack Reference —
  ATT&CK-like taxonomy for supply chain threats. Source:
  `github.com/pbom-dev/OSCAR` (verified 2026-07-18 — uses tactic +
  technique names, NOT numeric IDs).
- **deny.toml Specification:** License and advisory policy specification for
  Rust dependency auditing. Source: `embarkstudios/deny` documentation.
- **crates.io Advisory Database:** Rust crate advisory format and advisory
  ID reference. Source: `crates.io/advisory-db` documentation.
- **cargo-deny / cargo-audit:** Dependency auditing tools that `kali-audit`
  references for supply-chain surface checks. Source: `EmbarkStudios`
  cargo-deny repository documentation.
- **Snyk Supply Chain / SCA:** Developer security platform combining SCA,
  dependency reachability, and SBOM tracking (referenced as external context,
  not as a replacement for this native skill). Source: `docs.snyk.io/`.
- **Semgrep Supply Chain (SSC):** Dependency reachability scanning. Source:
