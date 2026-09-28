---
name: onto-anchor
description: Anchor domain terms via the onto_anchor tool and the P8.3 ladder — pinned domain sources (including RDF Data Cube `qb:`), derived, full SUMO, full schema.org, published DC/BIBO/CiTO state-axis senses, 5W1H core. Returns source definitions when supplied and alternative senses; nothing is ever untagged.
---

# Onto-Anchor

Ontological anchoring in zed-kask follows one canonical pattern, defined in
`hkask-bridge-ontology` and implemented across the fleet. This skill is that
pattern's process surface for term anchoring; the `onto_anchor` tool is its
mechanical step.

## Initial and target condition

- **Initial condition:** the term as it will appear in the output, and whether the claim depends on a relation to another term.
- **Target condition (per term):** the term carries a non-core anchor (rungs 1–5) cited in the output, or its core anchor plus a filed ruling request; a relational claim cites the returned directed edge path, or states `no_supported_path` within the bound.
- **PDCA exemption:** a resolution is one deterministic tool call, so there is no in-session loop. The improvement loop is the operator ruling → `derived.rs` entry → crate test → rebuild, after which the term resolves at rung 2.

## Step types

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Resolve a term; traverse a relation | D | `onto_anchor` tool output |
| Decide which words are domain terms worth anchoring | P | operator, via the ruling request and review of the output |
| Record a ruling | D | `cargo test -p hkask-bridge-ontology` (`all_terms_are_official`) |

## When to Use

- Before naming, classifying, or computing with a domain concept (a ratio,
  a process, an entity type, a risk) in an answer, a report, or code.
- When a claim depends on a relation between two concepts ("is ROE a
  constituent of the sustainable growth rate?") — use `relation_query`.
- When a term lands on the core rung and needs an operator ruling recorded in
  the derived registry.

## When NOT to Use

- Everyday words with no domain meaning, file paths, identifiers, or tool
  names — anchoring them adds noise, not checkability.
- Facts about particular instances ("does this company have a moat?") — the
  graph relates concepts, never instances; answer from data tools.
- Adding or correcting vocabulary — that is a build-gated code change to
  `kask/crates/hkask-bridge-ontology/src/derived.rs` after an operator ruling,
  not something this skill does at run time.

## The canonical pattern

The bridge crate is the single source of truth for ontology vocabulary
(`kask/crates/hkask-bridge-ontology/README.md`). Its published index is
compiled from SHA-256-pinned sources; named constants are only a consumer
selection. Current coverage includes SUMO, schema.org, DCMI/BIBO/CiTO,
PKO/P-Plan/PROV, SEPIO, GOLEM/CIDOC-CRM/LRMoo, OMC, ML-Schema, RDF/RDFS,
W3C RDF Data Cube (`qb:`) and FIBO's Q2 **Release** set (157 modules, 6,443
terms), not Provisional. The Cube source is pinned as `sources/rdf-data-cube/cube.ttl`
with its PDDL 1.0 license; FIBO's selected modules are in `fibo-release-modules.tsv`.
Former local `sdmx:` class-name aliases and five unlicensed `dlp:` constants
are removed, not silently translated into published IDs. Never invent a
publisher definition where the source supplies none. No
ontology vocabulary lives inside an MCP server.

Anchoring is a **scope-broadening walk, never a single pick** — the fallback
ladder (P8.3, `axis.rs` and the bridge root docs):

1. **Domain supplement** — exact terms in pinned sources (FIBO Q2 Release,
   RDF Data Cube, OMC, PKO/P-Plan/PROV, SEPIO, GOLEM/CIDOC-CRM/LRMoo,
   ML-Schema, RDF/RDFS). A statistical provider hint does not prove its
   outputs are RDF cubes; unmatched SDMX Information Model names do not resolve here.
   Never force a term into an ontology that has no place for it.
2. **Derived concepts** — recorded compositions over anchored constituents,
   each carrying its identity and its authority citation
   (`derived::DERIVED_CONCEPTS`). This is where operator rulings become
   durable anchors.
3. **Upper ontology** — the full SUMO distribution: formal categorization
   when no domain or derived concept fits. A financial metric with no FIBO
   term is a `sumo:Quantity`.
4. **General vocabulary** — the full schema.org release, after SUMO so a
   formal category is preferred.
5. **State-axis senses** — published Dublin Core, BIBO and CiTO after
   schema.org; artifact-axis selection is separate from this term walk.
6. **Interrogative ground** — the 5W1H core: the guaranteed final rung.

`published-term-resolution-v2` marks the current canonicalization; older
records require re-tagging. A published resolution carries the source file
and its own `definition` **if supplied** (an absent definition is omitted,
not emitted as `null` or inferred). Publisher deprecation/supersession is
reported in a separate `status` field: `"[deprecated]"` must never be treated
as a definition. Read `status` before relying on a published sense. When
several vocabularies publish the same word, every other sense
is listed in `alternatives` (schema.org's `Game` is a creative work; SUMO's
is a contest) — read them before relying on the chosen sense.

**The invariant: nothing is ever untagged.** The walk always terminates on a
real anchor. There is no "unanchored" verdict — a term that lands on the core
rung carries the ruling path: the anchor is real but coarse, and an operator
ruling (recorded in the derived registry) improves it.

## Where the pattern is implemented

- **Per-tool output anchors**: MCP servers that tag their results attach a
  bridge concept to the tool output's `ontology` field — e.g. the companies
  server's `fibo::enrich_with_ontology`
  (`kask/mcp-servers/hkask-mcp-companies/src/fibo.rs`) and the scenarios
  server's `ontology_anchor`
  (`kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs`).
  Coverage is per server, not fleet-wide; an untagged tool output is not an
  anchor — resolve its terms with `onto_anchor`.
- **Per-term anchors**: the portfolio widget is the reference
  implementation (`crates/hkask-portfolio-widget/src/view.rs`) — IRR anchors
  on its real FIBO term (rung 1); FIBO-less metrics anchor on
  `sumo:Quantity` (rung 3).
- **Term resolution and relation traversal for the agent**: `onto_anchor`
  resolves any term as before. An optional `relation_query` asks for outgoing
  neighbors (no `to`; `max_hops` must be 1, its default) or a bounded directed
  path (`to` term; `max_hops` 1–4, default 2). Each edge states its relation and authority. The graph
  includes resolved derived-concept constituents and, from the full SUMO and
  schema.org sources, every directly stated parent (`has_parent`) and inverse
  property (`inverse_of`). It adds no inferred or transitive edges and
  contains no facts about particular instances.

## Process

1. **Resolve before reasoning.** Call `onto_anchor` with the term before
   naming, categorizing, or computing with it.
2. **Relational question** → use `relation_query` on `onto_anchor`. For
   example `{"term":"sustainable growth rate", "relation_query":{"to":"net margin", "max_hops":2}}` traces the recorded constituent chain through return on equity.
   `{"term":"schema:hasPart", "relation_query":{}}` lists supported outgoing
   edges. Only assert the relationship actually stated by the directed,
   provenance-carrying path. `no_supported_path` means this bounded, partial
   graph lacks a path, not that the relation is false; `coarse_anchor` forbids
   traversal. An inverse-property edge connects property *concepts*, never
   proves an instance has or is part of another instance. Do not call the
   graph when the task needs no relation.
3. **Domain or derived rung** → compute from the published identity (rung 1)
   or the recorded identity (rung 2). Cite the concept in your output so
   the claim is redeemable against the published anchor.
4. **Core rung** → the anchor is real but coarse. Surface the term with its
   core anchor and request a ruling from the operator. The ruling closes the
   loop through a deliberate, build-gated code change: it lands as an entry
   in the derived registry (`hkask-bridge-ontology/src/derived.rs`) with its
   identity and authority, tested with the crate's suite — vocabulary
   changes are exactly the kind that should be build-gated, not
   runtime-mutable. The term resolves there ever after. Never assign a
   private definition in the meantime — fabricated ontology terms are a
   documented failure mode (63 of 70 FIBO URIs were invented before the
   2026-08-29 purge), and semantic mislabeling passes every structural check
   (observed 2026-09-10: a "net margin" that was a pre-interest operating
   formula survived two operator corrections and a 112-green test suite —
   the ruling that fixed it is the derived registry's first authority
   citation).

## Constraints

- Use only the anchors and edges `onto_anchor` returns; never invent a URI,
  an intermediate link, or a private definition.
- A core-rung anchor is real but coarse: surface it with the ruling request;
  do not upgrade it yourself.
- `no_supported_path` is not proof a relation is false; `coarse_anchor`
  cannot be traversed; `max_hops` is 1–4 for a path and exactly 1 for
    neighbors (`ontology_graph.rs` returns `InvalidQuery` otherwise).
- Operator-ruling compositions land in the derived registry with tests;
  published-source changes require new pinned files and build verification.
  `all_terms_are_official` checks named constants in the crate's test suite,
  without a local SDMX fixture or DLP exception.

## Verification

Before sending work that names domain terms: every term is either anchored
(cite the concept) or carries its core anchor with the ruling requested. A
term silently used with a private meaning is the failure this pattern exists
to prevent. For relational claims, cite the returned directed edge path and
its authority, or state that no supported path was found within the bound;
labels alone are not graph evidence.