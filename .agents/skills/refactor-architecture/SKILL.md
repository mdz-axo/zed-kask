---
name: refactor-architecture
visibility: public
description: "End-to-end architecture refactoring: discover friction, rank deepening candidates, walk the design tree, audit duplication, plan strangler-fig migration, and verify integrity. Merged from improve-codebase-architecture (discover phase) and refactor-service-layer (execution phase). Includes the migration-strategy phase (folded from the standalone strangler-fig skill). Composes tdd, coding-guidelines, pragmatic-semantics, graph-audit (code mode, context-expansion), deep-module, essentialist, and pragmatic-cybernetics."
---

# Refactor Architecture

End-to-end architecture refactoring skill. Merges the discovery phase (formerly `improve-codebase-architecture`) and the execution phase (formerly `refactor-service-layer`) into a single skill with a unified PDCA loop:

1. **Explore** — Walk the codebase to find architectural friction: shallow modules, tight coupling, missing locality, wide import surfaces, untestable code.
2. **Candidates** — Rank deepening candidates by leverage, locality, and testability. Ask the user which to explore.
3. **Deepen** — Walk the design tree for the selected candidate: define the deepened module shape, identify seams and adapters, confirm the test surface.
4. **Audit** — Audit cross-surface duplication (CLI, API, MCP) and classify each operation as Identical, Divergent, Surface-only, or Pass-through. Apply the deletion test.
5. **Strangle** — Plan and execute the strangler-fig migration for the selected domain: write failing tests, implement the service, wire adapters, delete duplicate logic. One domain per commit.
6. **Verify** — Verify surgical completeness: dependency direction, depth test, P6/P7/P8 compliance, clippy, test suite, surface adapter thinness.

Includes the migration-strategy phase (folded from the standalone `strangler-fig` skill). Composes `tdd`, `coding-guidelines`, `pragmatic-semantics`, `graph-audit` (code mode, context-expansion), `deep-module`, `essentialist`, and `pragmatic-cybernetics` as methodological guidance.

## When to Use

- When architectural friction is suspected in a codebase — shallow modules, tight coupling, missing locality, wide import surfaces, or code that is hard to test through its current interface.
- When you need to surface shallow modules and apply the deletion test to determine whether they are pass-throughs or earn their keep.
- When you want to propose deep modules with small interfaces and large implementations, ranked by recommendation strength and explained in terms of leverage and locality.
- When a candidate has been selected and you need to walk the design tree to define the deepened module shape, identify seams and adapters, and confirm the test surface.
- When duplicated domain operations exist across multiple surfaces (CLI, API, MCP) and need to be audited, classified, and assessed for extraction.
- When planning a strangler-fig migration to extract a shared service layer from duplicated surface logic for a specific domain.
- When verifying surgical completeness after a domain migration or full extraction to ensure dependency direction, module depth, and P6/P7/P8 compliance.

## Instructions

### ra-explore

1. Walk the codebase organically to find architectural friction — note where you experience difficulty understanding, navigating, or testing rather than applying rigid heuristics.
2. Classify each friction point by signal: understanding one concept requires bouncing between many small modules; interface nearly as complex as the implementation; pure functions extracted for testability while real bugs hide in how they are called; tightly-coupled modules leaking across seams; untestable code through the current interface; wide import surfaces or circular dependencies.
3. Apply the deletion test to suspected shallow modules — if complexity vanishes, the module was a pass-through; if complexity reappears across N callers, it earns its keep.
4. Use the project's domain vocabulary throughout.
5. Flag friction points that contradict known ADRs, but do not recommend revisiting them unless the friction is severe.
6. Do not propose interfaces or refactors — this template is exploration only.

### ra-candidates

1. Assess each friction point and shallow module to propose deepening candidates — refactors that turn shallow modules into deep ones.
2. For each candidate, specify the files involved, the problem (why the current architecture causes friction), the solution (what would change), the benefits, and the recommendation strength (`Strong`, `Worth exploring`, or `Speculative`).
3. Explain every candidate's benefits in terms of locality (how change, bugs, and knowledge concentrate), leverage (how callers benefit from the deeper interface), and testability (how tests would improve).
4. Surface ADR conflicts only when the friction is real enough to warrant revisiting — mark them clearly.
5. Rank the candidates and identify the top recommendation with rationale.
6. Ask the user which candidate to explore.
7. Do not propose interfaces yet — wait for the user to select a candidate.

### ra-deepen

1. Walk the design tree for the selected candidate through each decision in sequence.
2. Define the deepened module shape: specify the public interface items and what complexity the implementation now hides.
3. Design the seam: identify where the interface lives and what adapters satisfy it (production, test, or mock).
4. Confirm the test surface: identify what the module looks like from the outside, which tests survive a refactor, and which tests become easier.
5. Use domain vocabulary in every public interface item — not implementation jargon.
6. Add new glossary terms when naming a deepened module after a concept not yet in the glossary; update the glossary when sharpening a fuzzy term.
7. Propose ADRs sparingly — only when the decision is hard to reverse, surprising without context, and the result of a real trade-off.
8. Offer to record an ADR when the user rejects a candidate with a load-bearing reason that a future explorer would need.

### ra-route

1. Route the deepened design to the correct follow-up based on the decision signal.
2. If proceeding to refactor: continue to the audit phase (ra-audit) to classify duplication and plan the strangler-fig migration.
3. If more data is needed: recommend the `diagnose` skill, and specify what measurements are needed, how to instrument, and what thresholds would confirm or refute the hypothesis.
4. If deferring or rejecting: produce a decision summary with reasoning, and if a load-bearing reason was given, recommend recording it as an ADR with a suggested title and body.

### ra-audit

1. Find every domain operation that exists in more than one surface (CLI commands, API routes, MCP servers).
2. Classify the duplication for each operation as Identical, Divergent, Surface-only, or Pass-through.
3. Assess whether extraction is justified by applying the deletion test to each candidate.
4. Produce RDF triples referencing actual file paths, a classification table, and a mermaid entity-relationship diagram of the duplication landscape.
5. Classify design decisions using the five-force hierarchy and map them to Magna Carta principles where applicable.
6. Provide a top recommendation for which domain to migrate first and why.

### ra-strangle

1. Write one failing test per service operation in the service crate, using a `ServiceContext` and verifying a domain behavior with a contract annotation.
2. Implement the minimal code to pass the test, calling domain crates directly and returning domain types.
3. Wire the CLI adapter to call the service operation and format terminal output, deleting duplicate business logic from the CLI command file.
4. Wire the API adapter to call the same service operation and serialize to JSON, deleting duplicate business logic from the API route file.
5. Delete all remaining duplicated business logic from both surfaces so they contain only I/O framing.
6. Verify the full workspace by running `cargo check`, `cargo test`, and `cargo clippy` across all crates.
7. Enforce one-domain-per-commit discipline, surgical change scope, and inviolable dependency direction throughout the migration.

### ra-verify

1. Verify dependency direction to ensure CLI/API route to services, services route to domain crates, and no circular dependencies exist.
2. Apply the depth test to each module in the service crate by deleting it mentally and checking if complexity vanishes or reappears across callers.
3. Check P6/P7/P8 compliance by ensuring no stubs, no deprecation attributes, and that all tests verify stated behavioral properties.
4. Run clippy and the test suite across the service, CLI, API, and workspace crates.
5. Verify surface adapter thinness by ensuring CLI and API adapters contain only service calls, formatting, and error mapping.
6. Produce a structured pass/fail report with evidence, including command outputs and file paths, for any failures.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `ra-explore.j2` | KnowAct | Explore the codebase for architectural friction: shallow modules, tight coupling, missing locality, wide import surfaces, untested code. Apply the deletion test to suspected shallowness. |
| `ra-candidates.j2` | KnowAct | Present deepening candidates with files, problem, solution, benefits (leverage and locality), and recommendation strength. Use hKask domain vocabulary. Ask user which to explore. |
| `ra-deepen.j2` | KnowAct | Grill loop for a selected candidate: walk the design tree, define the deepened module shape, identify seams and adapters, confirm test surface. Update glossary and ADRs inline as decisions crystallize. |
| `ra-route.j2` | KnowAct | Route a deepened architecture design to the appropriate follow-up action (proceed_to_refactor, need_more_data, defer_or_reject). |
| `ra-audit.j2` | KnowAct | Audit and classify all duplicated operations across CLI, API, and MCP surfaces. Apply the deletion test to each candidate. Produce RDF triples, classification table, and mermaid entity-relationship diagram of the duplication landscape. |
| `ra-strangle.j2` | KnowAct | Plan the strangler-fig migration for a selected domain: define the new service operation, design CLI/API adapters, identify duplication to delete, and list verification steps. Enforces one-domain-per-commit discipline, dependency direction checks, and surgical change scope. |
| `ra-verify.j2` | KnowAct | Verify surgical completeness after a domain migration or full extraction: dependency direction, depth test, P6/P7/P8 compliance, clippy, test suite, deletion test on service modules. Produces a structured pass/fail report. |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest. When enabled, all analysis steps route through a multi-model panel. This skill uses **pi mode** (Plan-Implement) — Phase 1 synthesizes strategy (explore, candidates, deepen), Phase 2 synthesizes execution plan (audit, strangle, verify).

The convergence check step has `fusion: false` to ensure deterministic rubric evaluation uses single-model inference.

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
