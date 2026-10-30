---
name: refactor-architecture
core: true
description: "End-to-end architecture refactoring: discover friction, rank deepening candidates, walk the design tree, audit duplication, plan strangler-fig migration, and verify integrity. Composes tdd, coding-guidelines, pragmatic-semantics, deep-module."
audience: user
---

# Refactor Architecture

End-to-end architecture refactoring skill. Merges the discovery phase (formerly improve-codebase-architecture) and the execution phase (formerly refactor-service-layer) into a single skill with a unified PDCA loop:

1. **Explore & Candidates** (`ra-explore`) — Walk the codebase to find architectural friction (shallow modules, tight coupling, missing locality, wide import surfaces, untestable code), apply the deletion test, and rank deepening candidates by leverage, locality, and testability for the user to select.
2. **Deepen** (`ra-deepen`) — Walk the design tree for the selected candidate: define the deepened module shape, identify seams and adapters, confirm the test surface.
3. **Route** (`RA-1` `lisp_eval`) — Deterministically route the user's explicit decision signal (`proceed_to_refactor`, `need_more_data`, `defer_or_reject`) to `ra-audit`, `diagnose`, or ADR recording.
4. **Audit** (`ra-audit`) — Audit cross-surface duplication (CLI, API, MCP) and classify each operation as Identical, Divergent, Surface-only, or Pass-through. Apply the deletion test.
5. **Strangle** (`ra-strangle`) — Plan and execute the strangler-fig migration for the selected domain: write failing tests, implement the service, wire adapters, delete duplicate logic. One domain per commit.
6. **Verify** (`ra-verify` + `RA-2` `lisp_eval`) — Verify surgical completeness: dependency direction, depth test, code-hygiene compliance (no stubs, no deprecation attributes, behavioral tests), clippy, test suite, surface adapter thinness, with `overall_status` derived deterministically via `RA-2`.

Includes the migration-strategy phase (folded from the standalone strangler-fig skill). Composes `tdd` (ra-strangle's failing-then-passing seam tests), `coding-guidelines` (surgical change scope), `pragmatic-semantics` (the five-force constraint classification in ra-audit/ra-strangle), and `deep-module` (the depth test and the deletion test applied throughout; the design tree is this skill's own — ra-deepen walks it) as methodological guidance.

## Reference models

- **Ousterhout, *A Philosophy of Software Design*** (via `deep-module`) — deep modules (small interface, large implementation), the deletion test (delete the module: does complexity vanish or reappear across callers), and shallow-module diagnosis.
- **Fowler, "StranglerFigApplication"** (2004, martinfowler.com; cited at `kask/docs/architecture/core/MDS.md`'s fowler-strangler footnote) — introduce new alongside old, migrate one domain at a time, delete duplication only after both surfaces delegate.
- **The five-force hierarchy** (via `pragmatic-semantics`) — Prohibition / Guardrail / Guideline / Evidence / Hypothesis constraint-force classification for design decisions.
- **The Magna Carta principles** (the project's first four architecture principles, `kask/docs/architecture/core/PRINCIPLES.md`) — the mapping target for classified decisions.

## Initial and target condition

- **Initial condition:** the selected friction's concrete reaching paths, current interfaces/callers, observed behavior and the user's stated reason to change it. Exploration without a selected candidate remains advisory, not migration authority.
- **Target condition:** the same user-visible behavior reaches every affected surface through the agreed deeper interface; the selected friction is measurably reduced (locality, leverage or testability), dependency direction holds and duplicate business logic is gone. Tests and builds are necessary checks, not proof the original friction disappeared.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| ra-explore, ra-deepen, ra-audit | P | the user's candidate selection; ra-verify's observed outputs |
| RA-1 decision router | D (`lisp_eval` over the operator's explicit `decision` string) | the user's explicit decision signal |
| ra-strangle tests and builds; ra-verify + RA-2 `overall_status` | D | the failing-then-passing tests, `cargo check`, `cargo test`, `./script/clippy`, and `RA-2` `lisp_eval` |

## When to Use

- When architectural friction is suspected in a codebase — shallow modules, tight coupling, missing locality, wide import surfaces, or code that is hard to test through its current interface.
- When you need to surface shallow modules and apply the deletion test to determine whether they are pass-throughs or earn their keep.
- When you want to propose deep modules with small interfaces and large implementations, ranked by recommendation strength and explained in terms of leverage and locality.
- When a candidate has been selected and you need to walk the design tree to define the deepened module shape, identify seams and adapters, and confirm the test surface.
- When duplicated domain operations exist across multiple surfaces (CLI, API, MCP) and need to be audited, classified, and assessed for extraction.
- When planning a strangler-fig migration to extract a shared service layer from duplicated surface logic for a specific domain.
- When verifying surgical completeness after a domain migration or full extraction to ensure dependency direction, module depth, and code-hygiene compliance (no stubs, no deprecation attributes, behavioral tests).

## When NOT to Use

- Reviewing a change against its stated spec — use `code-review`.
- Diagnosing a specific bug or regression — use `diagnose`; refactoring presupposes working code.
- Greenfield interface design — use `deep-module`'s design step; this skill deepens what exists.

## Instructions

### ra-explore

1. Walk the codebase organically to find architectural friction — note where you experience difficulty understanding, navigating, or testing rather than applying rigid heuristics.
2. Classify each friction point by signal: understanding one concept requires bouncing between many small modules; interface nearly as complex as the implementation; pure functions extracted for testability while real bugs hide in how they are called; tightly-coupled modules leaking across seams; untestable code through the current interface; wide import surfaces or circular dependencies.
3. Apply the deletion test to suspected shallow modules — if complexity vanishes, the module was a pass-through; if complexity reappears across N callers, it earns its keep.
4. Propose deepening candidates from the observed friction points and shallow modules: specify `files`, `problem`, `solution`, `benefits` (`locality`, `leverage`, `testability`), `recommendation` (`Strong`, `Worth exploring`, or `Speculative`), and `adr_conflict`.
5. Rank the candidates, state `top_recommendation`, and ask the user which candidate to explore (`question_for_user`).
6. Defer concrete public interface items (`pub fn` / `trait` / `type` signatures) to `ra-deepen` until the user selects a candidate.

### ra-deepen

1. Walk the design tree for the selected candidate through each decision in sequence.
2. Define the deepened module shape: specify the public interface items and what complexity the implementation now hides.
3. Design the seam: identify where the interface lives and what adapters satisfy it (production, test, or mock).
4. Confirm the test surface: identify what the module looks like from the outside, which tests survive a refactor, and which tests become easier.
5. Use domain vocabulary in every public interface item — not implementation jargon.
6. Add new glossary terms when naming a deepened module after a concept not yet in the glossary; update the glossary when sharpening a fuzzy term.
7. Propose ADRs sparingly — only when the decision is hard to reverse, surprising without context, and the result of a real trade-off.
8. Offer to record an ADR when the user rejects a candidate with a load-bearing reason that a future explorer would need.

### RA-1 — Decision Routing (`lisp_eval`)

1. Route the user's explicit `decision` signal deterministically via `lisp_eval` `RA-1` (do not use an LLM template to route a 3-way enum):
   - form: `(cond ((string= decision "proceed_to_refactor") (list "proceed_to_refactor" "ra-audit")) ((string= decision "need_more_data") (list "need_more_data" "diagnose")) ((string= decision "defer_or_reject") (list "defer_or_reject" "record_adr")) (true (list "invalid" "ask_operator")))`
   - env: `{ "decision": <user decision string> }`
2. If `"proceed_to_refactor"`: continue to the audit phase (`ra-audit`) to classify duplication and plan the strangler-fig migration.
3. If `"need_more_data"`: recommend the `diagnose` skill, and specify what measurements are needed, how to instrument, and what thresholds would confirm or refute the hypothesis.
4. If `"defer_or_reject"`: produce a decision summary with reasoning, and if a load-bearing reason was given, recommend recording it as an ADR with a suggested title and body.
5. If `"invalid"`: do not infer rejection or proceed — ask the user for an explicit decision.

### ra-audit

1. Find every domain operation that exists in more than one surface (CLI commands, API routes, MCP servers).
2. Classify the duplication for each operation as Identical, Divergent, Surface-only, or Pass-through.
3. Assess whether extraction is justified by applying the deletion test to each candidate.
4. Produce RDF triples referencing actual file paths, a classification table, and a mermaid entity-relationship diagram of the duplication landscape.
5. Classify design decisions using the five-force hierarchy and map them to Magna Carta principles where applicable.
6. Provide a top recommendation for which domain to migrate first and why.

### ra-strangle

1. Discover the real service and surface packages/paths before rendering `ra-strangle`, passing the selected `target_condition` alongside the observed paths. Write one failing test per service operation at the confirmed seam with an independent oracle; use only the service's existing dependency/context API.
2. Implement the minimal code to pass the test, calling domain crates directly and returning domain types.
3. Wire the CLI adapter to call the service operation and format terminal output, keeping the previous logic reachable until the shared behavior is verified.
4. Wire the API adapter to call the same service operation and serialize to JSON; verify both surfaces delegate without changing their behavior.
5. Only after both adapters pass their behavioral checks, delete duplicated business logic from both surfaces so they contain only I/O framing.
6. Verify affected packages and the workspace with `cargo check`, `cargo test`, and `./script/clippy`; use real package names discovered from the tree, not presumed CLI/API/service crates.
7. Enforce one-domain-per-commit discipline, surgical change scope, and inviolable dependency direction throughout the migration.

### ra-verify

1. Verify dependency direction to ensure CLI/API route to services, services route to domain crates, and no circular dependencies exist.
2. Apply the depth test to each module in the service crate by deleting it mentally and checking if complexity vanishes or reappears across callers.
3. Check code-hygiene compliance: no `todo!()`/`unimplemented!()` stubs, no new `#[deprecated]` attributes (prefer deletion over deprecation), and all tests at the selected seam verify stated behavioral properties, not implementation structure.
4. Run clippy and the test suite across the service, CLI, API, and workspace crates.
5. Verify surface adapter thinness by ensuring CLI and API adapters contain only service calls, formatting, and error mapping.
6. Produce a structured report with observed command outputs and file:line evidence. Missing evidence is `unmeasured`, not a pass. Compute `overall_status` (`"pass"`, `"fail"`, or `"partial"`) deterministically via `lisp_eval` `RA-2` from `target_condition_status` (`"met"`, `"unmet"`, `"unmeasured"`) and the `checklist` statuses (`"done"`, `"not_done"`, `"unmeasured"`):
   - form: `(begin (define any-eq (lambda (v xs) (if (is_null xs) false (if (string= v (car xs)) true (any-eq v (cdr xs)))))) (define all-eq (lambda (v xs) (if (is_null xs) true (if (string= v (car xs)) (all-eq v (cdr xs)) false)))) (cond ((or (string= target_condition_status "unmet") (any-eq "not_done" checklist_statuses)) "fail") ((and (string= target_condition_status "met") (all-eq "done" checklist_statuses)) "pass") (true "partial")))`
   - env: `{ "target_condition_status": <"met"|"unmet"|"unmeasured">, "checklist_statuses": <array of status strings from checklist> }`
7. Compare the result to the initial and target conditions, not only a checklist. Before a migration, pass both as top-level context to `ra-verify`; require observed evidence that the selected friction decreased while all affected surfaces still satisfy the user's behavioral expectation. A passing build with persistent friction is `continue`, not `done`. On a failing check, fix the identified cause and re-verify at most twice for this domain; a third failure stops with its evidence. When the target passes, stop this domain. Explore another only on a new user-selected target, never automatically.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `ra-explore.j2` | Explore the codebase for architectural friction (shallow modules, tight coupling, missing locality, wide import surfaces, untested code), apply the deletion test, and rank deepening candidates with benefits (leverage, locality, testability) for the user to select. |
| `ra-deepen.j2` | Grill loop for a selected candidate: walk the design tree, define the deepened module shape, identify seams and adapters, confirm test surface. Update glossary and ADRs inline as decisions crystallize. |
| `ra-audit.j2` | Audit and classify all duplicated operations across CLI, API, and MCP surfaces. Apply the deletion test to each candidate. Produce RDF triples, classification table, and mermaid entity-relationship diagram of the duplication landscape. |
| `ra-strangle.j2` | Plan the strangler-fig migration for a selected domain: define the new service operation, design CLI/API adapters, identify duplication to delete, and list verification steps. Enforces one-domain-per-commit discipline, dependency direction checks, and surgical change scope. |
| `ra-verify.j2` | Verify surgical completeness after a domain migration or full extraction: dependency direction, depth test, code-hygiene compliance (no stubs, no deprecation attributes, behavioral tests), clippy, test suite, deletion test on service modules; `overall_status` derived via `RA-2` `lisp_eval`. |

To render a template, call the `render_template` tool with the template ref (e.g., `refactor-architecture/ra-explore`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `ra-audit.j2`: `focus_area`, `code_context`, `known_adrs`, `constraint_forces`
- `ra-deepen.j2`: `selected_candidate`, `focus_area`, `known_adrs`, `glossary_terms`
- `ra-explore.j2`: `focus_area`, `code_context`, `known_adrs`
- `ra-strangle.j2`: `domain`, `target_condition`, `service_module_name`, `current_cli_path`, `current_api_path`, `known_adrs`, `constraint_forces`
- `ra-verify.j2`: `domain`, `migration_phase`, `initial_condition`, `target_condition`, `code_context`, `known_adrs`

## Regression case

Receipts executed live through `lisp_eval` and `render_template`:
- `RA-1` decision router (`lisp_eval`): `decision = "proceed_to_refactor"` → `["proceed_to_refactor", "ra-audit"]`; `decision = "need_more_data"` → `["need_more_data", "diagnose"]`; `decision = "defer_or_reject"` → `["defer_or_reject", "record_adr"]`; `decision = "maybe"` → `["invalid", "ask_operator"]`.
- `RA-2` verification status (`lisp_eval`): `target_condition_status = "unmeasured"`, `checklist_statuses = ["done", "done", "unmeasured"]` → `"partial"` (a build-only pass leaves the target condition `unmeasured` and the overall status `partial`); `target_condition_status = "met"` with all `"done"` → `"pass"`; any `"unmet"` or `"not_done"` → `"fail"`.
- `render_template` `refactor-architecture/ra-verify` with a domain/migration context renders the verification checklist whose discipline is unmeasured-never-pass; the cargo commands (`cargo check`, `cargo test`, `./script/clippy`) are run at use time.

## Constraints

- Exploration without a selected candidate remains advisory, never migration authority; the user's explicit signal (`RA-1`) gates any refactor.
- One domain per commit, surgical change scope: every changed line traces to the extraction; no cross-domain refactors, no style changes.
- Dependency direction is inviolable: CLI/API route to services, services route to domain crates, no circular dependencies.
- Duplicate business logic is deleted only after both adapters pass their behavioral checks — never while the previous logic is still the verified path.
- Verification is evidence-based: missing command output is `unmeasured`, never a pass; a passing build with persistent friction is `continue`, not `done`. Re-verify at most twice per domain; a third failure stops with its evidence.
- Real package names from the tree, never presumed CLI/API/service crates; the deletion test decides whether a module earns its keep.

