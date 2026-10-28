---
title: "Tree-sitter Code-Graph Capability Research — zed-kask Adoption Proposal"
audience: [architects, developers, agents]
last_updated: 2026-10-11
version: "1.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# Tree-sitter Code-Graph Capability Research — zed-kask Adoption Proposal

**Decision (2026-10-11): accepted — "proceed as recommended."** Scope
answers carried by that acceptance: the proposal persists here; ast-grep
joins the pinned external tools (the jscpd precedent); use cases land in
priority order UC-1 → UC-2 → UC-3; the first rule pack is Rust-only.
Implementation landed the same day and is recorded in §7 with measured
experiment outcomes. **The four §7.3 follow-ups were accepted and landed
the same day** ("proceed as recommended with items 1–4"), including the
correction of this document's own false-absence citation reading (§7.2).

## 1. Premise verdict

**Partially confirmed.** The underuse is real and measured: kask-side
surfaces held **zero** tree-sitter-derived structural analysis while
upstream Zed vendors a rich substrate, and the fork's own `.rules` record
repeated failure classes of textual analysis that structural analysis
catches. The honest scope is narrower than "code-graph understanding":
tree-sitter is a **parser, not a graph engine** (§2, §6) — it earns
adoption for the structural pattern layer, not for call graphs or
semantic resolution.

## 2. Phase 0 — usage inventory (reproducible)

Commands (run from the repo root):

```bash
rg -l "tree.sitter" --glob "**/Cargo.toml"          # 20 manifests
rg --count-matches "tree_sitter::" --type rust crates/ | awk -F: '{n=split($1,p,"/"); c[p[2]]+=$NF; f[p[2]]++} END {for (k in c) printf "%d refs %d files %s\n", c[k], f[k], k}' | sort -rn
rg -n -i "tree.sitter|ast.grep" kask/ .agents/ .rules DIVERGENCE.md AGENTS.md crates/agent/
```

Measured (2026-10-11):

| Surface | tree-sitter usage |
|---|---|
| Upstream Zed | 19 crates declare deps; **131 `tree_sitter::` refs** (`language` 66/6 files, `migrator` 30/27, `language_core` 12, `settings_json` 11, rest 1–3) |
| Representative call sites | `crates/language/src/syntax_map.rs` (35 refs — the incremental-parsing wrapper), `buffer.rs`, `language_core/src/grammar.rs`, `language.rs`, `language_registry.rs` |
| `kask/` subtree | **0 code refs** — `kask/deny.toml` license URLs + one prose mention (`DIVERGENCE.md:250`) |
| `crates/agent` | **0 API refs**; 2 comments in `read_file_tool.rs:510,739` explicitly *avoiding* tree-sitter on pseudo-code |
| `.agents/skills` (60 skills) | **0** structural mentions |
| Instruments present | all textual: ~70 bash/jq check scripts, `rg`, jscpd |
| `ast-grep` installed before this change | **no** (`/usr/bin/sg` is util-linux setgid, not ast-grep) |

## 3. Capability matrix (each row cited; core sources annotated `verified` under research run `d967a050155a79f4`)

| # | Capability | What it does | Citation | (a) pattern queries | (b) graph construction | (c) cross-file resolution | (d) incremental |
|---|---|---|---|---|---|---|---|
| 1 | General parsing → concrete syntax tree, error-tolerant | Full parse tree even amid syntax errors (error start/end marked) | tree-sitter docs, *using-parsers* (verified) | substrate | — | — | — |
| 2 | Node API | Node types, byte/point ranges, named vs anonymous nodes, fields, traversal | tree-sitter docs, *Basic Parsing* (verified, extracted) | substrate | — | — | — |
| 3 | Query language | S-expression patterns over tree nodes; captures, quantifiers, predicates | tree-sitter docs, *queries/1-syntax*, *2-operators* (verified) | **yes** | — | — | — |
| 4 | Incremental parsing | `old_tree` reuse: reparse only edited spans | tree-sitter docs (the `old_tree` param, verified); live instance: upstream `syntax_map.rs` | — | — | — | **yes** |
| 5 | Structural search/lint/rewrite at scale | ast-grep: AST search with code-like patterns, YAML rules, relational constraints | ast-grep.github.io + github.com/ast-grep/ast-grep (both verified); Vercel lints Rust with custom rules (*how-ast-grep-works*) | **yes** | — | — | n/a (CLI cold parse) |
| 6 | Graph construction DSL | `tree-sitter-graph`: arbitrary graph structures from parsed source | github.com/tree-sitter/tree-sitter-graph (verified) | — | **yes** | — | no |
| 7 | Name resolution via stack graphs | def/ref resolution within and across repositories (GitHub precise code nav) | docs.rs `tree-sitter-stack-graphs` (verified); "Stack Graphs: Name Resolution at Scale," OASIcs EVCS 2023 (10.4230/OASIcs.EVCS.2023.8) | — | yes | **yes** | partial/unverified |

**Category boundary (required):** derivable from tree-sitter output are
structural pattern queries (3, 5), syntax-anchored graphs (6), and — only
with per-language rules authoring — name resolution (7). Not derivable:
type-based call graphs, semantic dataflow, type checking — parse trees
carry no type/symbol information. In this Rust-first repo the semantic
layer is already served by rust-analyzer/LSP inside Zed; this proposal
does not duplicate it.

## 4. Use cases (essentialist 3-gate verdicts recorded)

**UC-1 — pinned ast-grep instrument. SURVIVES. Priority 1.**
Surface: `kask/scripts/ast-grep.sh` + `kask/ast-grep/` (config + rules).
Capability: #1/#3 via ast-grep. Gap closed: zero structural instruments
kask-side; the agent's textual instruments have named miss classes
(`.rules`: "a single-line grep missed a multi-line call"). Alignment:
Karpathy Simplicity First (reuse a maintained tool); jscpd external-tool
pinning precedent (`check-duplication-ratchet.sh`); Vercel's production
Rust linting precedent. Value claim (pre-registered): on a seeded corpus
of ~20 trap instances, ast-grep detects ≥90% with 0 FPs where single-line
grep detects ≤60% with ≥5 FPs. **Measured (§7.1): ast-grep 20/20 = 100%,
0 FPs; grep 12/20 = 60%, 8 FPs — claim confirmed.**

**UC-2 — structural trap lint pack. SURVIVES. Priority 2.**
Surface: `kask/scripts/check-structural-traps.sh` +
`structural-traps-baseline.txt` + the `no-unwrap-in-production` rule.
Capability: #3 + relational `inside`/`has` (composite
`kind: mod_item` + `has: '#[cfg(test)]'` — the single
`#[cfg(test)] mod …` pattern does not parse: multiple AST roots).
Gap closed: the `.rules` "No `unwrap()` in production code" had **no
dedicated enforcement** — a textual grep cannot exclude cfg(test) mods,
literals, or comments. Alignment: the `.rules` trap itself; the
dead-code-allow-baseline ratchet format; the `check-*-selftest.sh` CI
glob. Value claim: 0 FPs on the seeded corpus where textual grep yields
≥N decoy hits; CI runtime within budget. **Measured: 0 FPs (grep: 8);
gate runs ~2s over the real surface; the real surface holds 468 existing
instances — the rule was aspirational, never enforced, and the ratchet
makes the debt countable (no net-new) instead of invisible.**

**UC-3 — symbol-anchored citation re-anchoring. SURVIVES. Priority 3.**
Surface: `kask/scripts/resolve-citation-anchor.sh` + the advisory
re-anchor suggestion in `check-mcp-review-citations.sh`.
Capability: #3 definition-site queries (`function_item`/`struct_item`/
`enum_item`/`trait_item`/`mod_item`, name captures, spans). Gap closed:
positional `file:line` anchors rot on line shifts (`.rules`: "a
line-shifting edit … silently invalidates every doc citing those
lines"); re-anchoring was manual. Alignment: `doc-update`'s
evidence-grounded citation discipline; ISA 500 vouching (claim→source)
via `grounding-verify`; the stale-comments trap. Value claim
(pre-registered): ≥90% of ≥30 sampled kask/docs citations survive a
simulated 10-line insertion above the cited line. **Measured (§7.2):
falsified as stated — 44/60 = 73% overall; refined and confirmed for
the anchorable class: 44/47 = 93.6% of citations with an enclosing
definition re-resolve to the correct current line; 13/60 cite
top-level lines no definition encloses (not symbol-anchorable by the
v1 rule); 3 are ambiguous (same-named methods in multiple impls — the
documented exit-2 path). Raw line anchors: 0% survive (47/47 moved
exactly +10, outside the ±3 window). The initial corpus reading —
"671 (41%) already citing files that no longer exist" — was FALSE
ABSENCE (a root-only resolution sweep; corrected in §7.2), and the
accepted follow-up landing lifts survival to 52/60 = 86% overall,
52/55 = 94% of anchorable, residual classes documented (§7.3).**

**UC-4 — AST clone detection for the duplication ratchet. REJECTED
(Exist gate):** the observed ratchet trap was report-name *resolution*,
not detection quality; ast-grep is pattern-based, not a clone detector —
new tooling for an undemonstrated gap.

**UC-5 — structural design-tree metrics for `refactor-architecture`.
REJECTED (Surface gate):** interface-width counting is textual
(`rg -c "pub fn"`); a structural instrument adds nothing load-bearing.

**UC-6 — structural-search MCP server tool. DEFERRED (Contract gate):**
the grep-miss classes are real but served by UC-1's CLI route via the
terminal; a server + schema + seam is not justified yet. **Trigger:**
adopt only if grep-miss classes recur after UC-1 lands, or a skill needs
in-process structural queries.

## 5. Composition and maintenance plan (as landed)

- **Query-time, ephemeral composition.** No durable graph store: each run
  parses → matches → emits `file:line` findings. A persistent code-graph
  database has no demonstrated consumer here, and its freshness machinery
  would cost more than the value.
- **Layered instrument stack:** textual (`rg`/`grep`/`jq` — retained,
  working) → **structural (ast-grep — this change)** → semantic
  (rust-analyzer/LSP — already present via Zed, out of scope). Each trap
  class routes to the cheapest layer that catches it; a rule is admitted
  only where the textual layer demonstrably fails.
- **Freshness:** no incremental parsing at repo-analysis scale — cold
  parse per run (~2s measured over the full kask-owned surface);
  incremental parsing (#4) is an editor-scale capability upstream already
  provides in `crates/language/src/syntax_map.rs`.
- **Maintenance:** rules are data under `kask/ast-grep/rules/`, reviewed
  like code; the version pin lives once, in `ast-grep.sh`
  (`AST_GREP_VERSION`, the `JSCPD_VERSION` precedent); each rule's
  admission bar is the `.rules` bar; the trap baseline is a ratchet
  (removals never fail, net-new does); every gate carries a selftest on
  the `check-*-selftest.sh` CI glob.
- **Implementation notes follow the four Karpathy principles:** Think
  Before Coding (the experiments ran before the pack grew), Simplicity
  First (external CLI over in-tree engine; four rules at day's end, each
  admitted at the `.rules` bar by a measured gap — not a speculative
  pack), Surgical Changes (the citations checker gained an advisory
  suggestion — its exit contract unchanged; the string-errors gate's
  line-based matching was replaced by the structural rule in the
  accepted follow-up, its exclusions preserved), Goal-Driven Execution
  (each use case shipped with its falsifiable claim as the acceptance
  test — and one claim was falsified, corrected, and is reported as
  such).

## 6. Explicit non-uses

1. **Type-based call graphs / semantic dataflow** — outside tree-sitter's
   category (no types in parse trees); rust-analyzer/LSP owns this here.
2. **Cross-file name resolution via stack graphs** — real capability,
   but per-language rules authoring is a project unto itself;
   rust-analyzer covers this repo at far lower cost.
3. **Persistent code-graph database** — no consumer; query-time
   composition (§5).
4. **Replacing working textual checks** — Surgical Changes; textual
   suffices where the trap is textual (`check-no-mod-rs.sh`,
   `check-no-smol-timer.sh`, `check-unsafe-forbid.sh`).
5. **In-tree tree-sitter dependency in hKask crates** — forbidden
   coupling: `check-hkask-no-zed-deps.sh` enforces that hKask crates
   never depend on zed-kask crates, and a direct tree-sitter dep would
   duplicate the grammar lifecycle the pinned external CLI already
   carries.
6. **Type-dependent rules** (e.g., `let _ =` on `Result`) — parse trees
   cannot know a call returns `Result`; those stay textual/approximate.

## 7. Implementation record (2026-10-11; the §7.3 follow-ups accepted and landed the same day)

Files (all kask-side; no DIVERGENCE.md seam — no zed-side edit):

- `kask/ast-grep/sgconfig.yml` — the rule index (`ruleDirs: [rules]`).
- `kask/ast-grep/rules/no-unwrap-in-production.yml` — UC-2's rule
  (composite `not.inside kind: mod_item / has: '#[cfg(test)]'`).
- `kask/ast-grep/rules/rust-definitions.yml` — UC-3's definition-site
  rule (name captures + spans; widened 2026-10-11 to the top-level item
  vocabulary — functions incl. trait signatures, structs, enums, traits,
  mods, consts, statics, type aliases, macro_rules, extern crate, use
  paths; `crate_attributes` is not a tree-sitter-rust kind, verified).
- `kask/scripts/ast-grep.sh` — UC-1's pinned wrapper
  (`AST_GREP_VERSION=0.50.0`, `npx --package @ast-grep/cli@…`; the
  package exposes two bins so plain `npx @ast-grep/cli` cannot pick one —
  the `--package … ast-grep` form is load-bearing).
- `kask/scripts/check-structural-traps.sh` — the ratchet gate
  (`--refresh`; `SCAN_DIRS`/`BASELINE_FILE` overrides for the selftest;
  stream JSON parsed with jq from a temp file, never a pipe — the
  `.rules` pipe/SIGPIPE trap).
- `kask/scripts/structural-traps-baseline.txt` — 468 measured current
  instances (the honest pre-existing debt, now ratcheted).
- `kask/scripts/check-structural-traps-selftest.sh` — 5 cases green
  (refresh, ratchet pass + decoy exclusion, net-new fail, empty surface,
  stale note).
- `kask/scripts/resolve-citation-anchor.sh` — UC-3's resolver (enclosing
  mode + `--find` mode + ambiguity exit 2; widened 2026-10-11: enclosing
  mode returns re-findable qualified names — `Type::method` for methods
  inside impls, `impl Type` for impl blocks — and `--find` learns both
  forms, with generic normalization).
- `kask/ast-grep/rules/rust-impls.yml` — the impl-block index for
  `--find Type::method` disambiguation.
- `kask/ast-grep/rules/no-string-errors.yml` — the `Result<_, String>`
  structural rule (context+selector form — the naive pattern parses as
  a comparison and matches nothing; the trap is recorded in the rule).
- `kask/scripts/check-string-errors.sh` — the string-errors gate,
  structural since 2026-10-11 (the line-based grep is replaced; its
  exclusions — tests/examples/target, main.rs, `string-error-ok` — are
  preserved in the jq filter).
- `kask/scripts/check-docs-citations.sh` — the docs-citation gate: every
  `path.rs:line` citation in kask/docs resolves via a ladder (root →
  doc-relative → unique suffix → unique basename); unresolvable files
  fail, line drift and ambiguous bare names warn.
- `kask/scripts/check-mcp-review-citations.sh` — extended with the
  advisory `re-anchor:` suggestion (exit contract unchanged).
- `kask/scripts/check-mcp-review-citations-selftest.sh` — new; 16 cases
  green (pass contract, resolver modes incl. the widened kinds and
  impl-qualified pins, drift + verbatim re-anchor, vanished excerpt +
  structural re-anchor, out-of-range).

### 7.1 Seeded-corpus experiment (UC-1/UC-2 claim — confirmed)

Corpus: 20 real instances (8 production unwraps incl. multi-line
receivers; 12 `register_tool("lisp_eval")` call sites — 4 one-line,
8 split across lines) + decoys (4 cfg(test), 2 string literals,
2 comments). Measured: **ast-grep 20/20 = 100% detection, 0 FPs**
(rule: 8/8 unwraps, excludes every decoy; pattern: 12/12 call sites
including all 8 split forms). **Single-line grep 12/20 = 60% detection,
8 FPs** (`.unwrap()` hits literals/comments/test mods; the fixed-string
call pattern misses every split form). Pre-registered thresholds
(≥90%/0 FP vs ≤60%/≥5 FP): all met.

### 7.2 Citation-shift experiment (UC-3 claim — falsified as stated, refined and confirmed for the anchorable class)

Sample: 60 of 985 repo-root-resolvable unique `.rs` citations in
kask/docs (1656 total). **Correction (2026-10-11): the initial "671 =
41% already cite deleted files" reading was FALSE ABSENCE — a root-only
resolution sweep; docs cite crate-relative paths (`src/hkask_mcp_media.rs`)
and bare names (`thread.rs:160`) that resolve from doc context. The
resolution ladder (`check-docs-citations.sh`) resolves every citation:
0 rotted; the real defect class was 9 line-drift citations (re-pointed
the same day, 13 textual sites across 6 docs) and 223 ambiguous bare
names (warnings, resolved by doc context).** Per citation: resolve the enclosing definition pre-shift,
insert 10 lines immediately above the cited line in a temp copy,
re-resolve by name, compare against the expected current line.
**Measured: 44/60 = 73% overall — the ≥90% claim over all citations is
falsified.** Refined: **44/47 = 93.6% of symbol-anchorable citations**
re-resolve correctly; 13/60 cite top-level lines no definition encloses
(imports, crate attrs — outside the v1 rule's item kinds); 3 are
ambiguous (same-named methods in multiple impls, the documented exit-2
path). Raw line anchors: **0% survive** (47/47 original content sits
exactly +10 post-shift, outside the ±3 excerpt window). Harness note:
the first run measured 0% from two harness defects (extensionless temp
files defeat ast-grep's language inference; top-of-file insertion
simulated with mid-file expected-line arithmetic) — diagnosed, fixed,
re-measured; the mechanism was sound, the harness was not.

### 7.3 Follow-ups (accepted 2026-10-11 — all four landed the same day)

- **Widened kinds — LANDED.** `rust-definitions.yml` now covers the
  top-level item vocabulary. The 13/60 non-anchorable class was mostly
  const items, type aliases, use paths, and doc comments inside impl
  blocks. Re-measured: noenc 13 → 5 (top-level doc comments and a blank
  line — no node encloses them; the documented limit).
- **Impl-qualified `--find` — LANDED.** `rust-impls.yml` + the resolver's
  qualified names (`Type::method`, `impl Type`; enclosing mode returns
  re-findable names). Re-measured: **52/60 = 86% overall, 52/55 = 94% of
  anchorable**; residuals documented (2 ambiguous impl blocks — 4 impls
  of `NativeAgentConnection`, 3 of `Thread`; 1 trait signature
  `ResultExt::log_err`).
- **The "671 rotted citations" worklist — RESOLVED AS FALSE ABSENCE.**
  The ladder gate measures **0 rotted**; the real class was 9 line-drift
  citations, re-pointed the same day (13 textual sites across 6 docs,
  plus one stale count corrected: 42/35 → 43/36 non-cloud tools in the
  swarm explanation). The gate lands green: 1664 resolved, 0 rotted,
  0 line-drift, 223 ambiguous-name warnings.
- **`Result<_, String>` structural rule — LANDED.** `no-string-errors.yml`
  + the `check-string-errors.sh` structural rewrite (line-based pass
  replaced, exclusions preserved). Measured: **0 instances** on the
  real surface — a *pinned* zero (the selftest's 4 cases include the
  multi-line violation the old pass could not see and the nested-String
  Ok type it false-positived). The naive-pattern trap (a bare
  `Result<$T, String>` pattern parses as a comparison and matches
  nothing) is recorded in the rule — the selftest caught it after a
  real-surface run had read "0 instances" consistently with both
  "clean" and "broken".

## 8. Skill orchestration (all eight, none decorative)

| Skill | Phase | Role played |
|---|---|---|
| hypothesis-framer | 0 | H1/H0 falsifiable form + FINER gates (PICO used only as loose analogy — noted) |
| kata-improvement | 0→2 | grasp current (inventory) → establish target (use cases); coaching-kata unused (no learner) |
| metacognition | all gates | per-phase predictions vs measured gaps (§7.2 records a falsified claim and its diagnosis) |
| grill-me | 3 | Recall→Mechanism→Rationale→Edge→Synthesis interrogation structure; its own contract routes *self*-review to `falsifiability` — noted, the elimination was carried by essentialist + the falsifiable claims |
| essentialist | 3 | 3-gate per use case: 3 survive / 2 reject / 1 defer with reasons |
| coding-guidelines | 2→3 | four Karpathy principles on §5/§7's implementation notes |
| refactor-architecture | 2→3 | surface walk + deep-module/deletion-test notes per survivor |
| improv | decision points | Plussing discipline for open questions; category-boundary disagreement surfaced with evidence, never smoothed |