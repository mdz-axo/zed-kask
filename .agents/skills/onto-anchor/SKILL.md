---
name: onto-anchor
description: Anchor domain terms on published ontology concepts by walking the canonical fallback ladder (P8.3) — domain supplement, derived concepts, SUMO upper, 5W1H core — via the onto_anchor tool. Nothing is ever untagged. Use before naming, categorizing, or computing any domain concept.
---

# Onto-Anchor

Ontological anchoring in zed-kask follows one canonical pattern, defined in
`hkask-bridge-ontology` and implemented across the fleet. This skill is that
pattern's process surface for term anchoring; the `onto_anchor` tool is its
mechanical step.

## The canonical pattern

The bridge crate is the single source of truth for ontology vocabulary
(`hkask/crates/hkask-bridge-ontology/README.md`): every concept is a
fixture-pinned constant verified against its published standard, and
`all_terms_are_official` fails the build if a term drifts. No ontology
vocabulary lives inside an MCP server; no fabricated URIs; no private
definitions.

Anchoring is a **scope-broadening walk, never a single pick** — the fallback
ladder (P8.3, `axis.rs` and the bridge root docs):

1. **Domain supplement** — the domain's published ontology (FIBO, PKO, SEPIO,
   GOLEM, SDMX, ML-Schema, OMC, schema.org, RDF). Never force a term into an
   ontology that has no place for it in its graph.
2. **Derived concepts** — recorded compositions over anchored constituents,
   each carrying its identity and its authority citation
   (`derived::DERIVED_CONCEPTS`). This is where operator rulings become
   durable anchors.
3. **Upper ontology** — SUMO: formal categorization when no domain or
   derived concept fits. A financial metric with no FIBO term is a
   `sumo:Quantity`.
4. **Interrogative ground** — the 5W1H core: the guaranteed final rung.

**The invariant: nothing is ever untagged.** The walk always terminates on a
real anchor. There is no "unanchored" verdict — a term that lands on the core
rung carries the ruling path: the anchor is real but coarse, and an operator
ruling (recorded in the derived registry) improves it.

## Where the pattern is implemented

- **Per-tool span anchors**: every MCP server carries an
  `ontology_anchor(tool)` fn mapping each registered tool to a bridge
  concept, called through `execute_tool_semantic`
  (`docs/reports/mcp-ontology-tagging-proposal.md` — implemented; coverage
  and stub-collapse tests enforce it).
- **Per-term anchors**: the portfolio widget is the reference
  implementation (`crates/hkask-portfolio-widget/src/view.rs`) — IRR anchors
  on its real FIBO term (rung 1); FIBO-less metrics anchor on
  `sumo:Quantity` (rung 3).
- **Term resolution for the agent**: the `onto_anchor` tool walks the same
  ladder for any term and returns the rung, namespace, concept, and — on the
  derived rung — the recorded identity and authority.

## Process

1. **Resolve before reasoning.** Call `onto_anchor` with the term before
   naming, categorizing, or computing with it.
2. **Domain or derived rung** → compute from the published identity (rung 1)
   or the recorded identity (rung 2). Cite the concept in your output so
   the claim is redeemable against the published anchor.
3. **Core rung** → the anchor is real but coarse. Surface the term with its
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

## Verification

Before sending work that names domain terms: every term is either anchored
(cite the concept) or carries its core anchor with the ruling requested. A
term silently used with a private meaning is the failure this pattern exists
to prevent.