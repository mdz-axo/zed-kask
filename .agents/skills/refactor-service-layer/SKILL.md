---
name: refactor-service-layer
visibility: public
description: "Extract a shared service layer from duplicated surface logic using the strangler fig pattern, deep-module discipline, and vertical tracer-bullet TDD. Classifies duplication hotspots, designs deep service modules, migrates one domain at a time with both surfaces functional at every step. Composes improve-codebase-architecture, tdd, coding-guidelines, pragmatic-semantics, zoom-out, and pragmatic-cybernetics.
"
---

# Refactor Service Layer

Extract a shared service layer from duplicated surface logic using the strangler fig pattern, deep-module discipline, and vertical tracer-bullet TDD. Classifies duplication hotspots, designs deep service modules, migrates one domain at a time with both surfaces functional at every step. Composes improve-codebase-architecture, tdd, coding-guidelines, pragmatic-semantics, zoom-out, and pragmatic-cybernetics.


## When to Use

- When duplicated domain operations exist across multiple surfaces (CLI, API, MCP) and need to be audited, classified, and assessed for extraction.
- When planning a strangler fig migration to extract a shared service layer from duplicated surface logic for a specific domain.
- When verifying surgical completeness after a domain migration or full extraction to ensure dependency direction, module depth, and P6/P7/P8 compliance.

## Instructions

### rsl-audit

1. Find every domain operation that exists in more than one surface (CLI commands, API routes, MCP servers).
2. Classify the duplication for each operation as Identical, Divergent, Surface-only, or Pass-through.
3. Assess whether extraction is justified by applying the deletion test to each candidate.
4. Produce RDF triples referencing actual file paths, a classification table, and a mermaid entity-relationship diagram of the duplication landscape.
5. Classify design decisions using the five-force hierarchy and map them to Magna Carta principles where applicable.
6. Provide a top recommendation for which domain to migrate first and why.

### rsl-strangle

1. Write one failing test per service operation in the service crate, using a `ServiceContext` and verifying a domain behavior with a contract annotation.
2. Implement the minimal code to pass the test, calling domain crates directly and returning domain types.
3. Wire the CLI adapter to call the service operation and format terminal output, deleting duplicate business logic from the CLI command file.
4. Wire the API adapter to call the same service operation and serialize to JSON, deleting duplicate business logic from the API route file.
5. Delete all remaining duplicated business logic from both surfaces so they contain only I/O framing.
6. Verify the full workspace by running `cargo check`, `cargo test`, and `cargo clippy` across all crates.
7. Enforce one-domain-per-commit discipline, surgical change scope, and inviolable dependency direction throughout the migration.

### rsl-verify

1. Verify dependency direction to ensure CLI/API route to services, services route to domain crates, and no circular dependencies exist.
2. Apply the depth test to each module in the service crate by deleting it mentally and checking if complexity vanishes or reappears across callers.
3. Check P6/P7/P8 compliance by ensuring no stubs, no deprecation attributes, and that all tests verify stated behavioral properties.
4. Run clippy and the test suite across the service, CLI, API, and workspace crates.
5. Verify surface adapter thinness by ensuring CLI and API adapters contain only service calls, formatting, and error mapping.
6. Produce a structured pass/fail report with evidence, including command outputs and file paths, for any failures.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `rsl-audit.j2` | KnowAct | Audit and classify all duplicated operations across CLI, API, and MCP surfaces. Apply the deletion test to each candidate. Produce RDF triples, classification table, and mermaid entity-relationship diagram of the duplication landscape.  |
| `rsl-strangle.j2` | KnowAct | Plan the strangler fig migration for a selected domain: define the new service operation, design CLI/API adapters, identify duplication to delete, and list verification steps. Enforces one-domain-per-commit discipline, dependency direction checks, and surgical change scope.  |
| `rsl-verify.j2` | KnowAct | Verify surgical completeness after a domain migration or full extraction: dependency direction, depth test, P6/P7/P8 compliance, clippy, test suite, deletion test on service modules. Produces a structured pass/fail report.  |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **pi mode** — Plan → implement
matches strangler fig pattern.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `rsl-audit.j2`: Public.
- `rsl-strangle.j2`: Public.
- `rsl-verify.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
