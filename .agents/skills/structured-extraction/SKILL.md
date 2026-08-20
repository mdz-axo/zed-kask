---
name: structured-extraction
description: "Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.
"
---

# Structured Extraction

Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.


## When to Use

- When you need to identify entities in unstructured text and map them to a target schema using extraction hints.
- When you need to extract binary relations between identified entities as OpenIE `(arg1, relation, arg2)` tuples (Banko et al. 2007) — free-text predicates, not RDF triples. For closed-type RE (ACE2005/TACRED/DocRED inventories), pre-populate the predicate vocabulary in `extraction_hints`.
- When you need to map extracted entities and relations to a target JSON schema, resolving field mappings and inferring missing fields.

## Instructions

### identify-entities

1. Scan the source text for any information that maps to the fields defined in the target schema.
2. Extract the exact text from the source for each entity found.
3. Classify the entity type (person, organization, date, quantity, location, etc.).
4. Map the entity to the corresponding schema field it populates.
5. Assign a confidence score (0.0-1.0) reflecting genuine extraction certainty.
6. Record the location of the entity in the source text using character offsets.
7. Identify any text segments that contain structured information but do not clearly map to a specific schema field as unmapped text.

### extract-relations

1. Open Information Extraction (OpenIE, Banko et al. 2007): extract a binary relation tuple for each pair of entities that have a meaningful relationship in the source text. These are `(arg1, relation, arg2)` tuples with free-text predicates — NOT RDF triples (W3C RDF 1.1), which require typed IRI predicates from a vocabulary.
2. Identify the Subject (arg1) as the entity performing or originating the relationship (after coreference resolution — CoNLL-2012).
3. Identify the Predicate (relation) as a short verb phrase (1-3 words) extracted from the source. For closed-type RE, use ACE2005 (6 types/35 subtypes), TACRED (41 types), or DocRED (96 Wikidata relations) — this skill is open by default.
4. Identify the Object (arg2) as the entity receiving or being the target of the relationship.
5. Record the `trigger_span` (character offsets of the relation signal word(s), ACE2005-style) for every relation.
6. Assign a confidence score (0.0-1.0) for each relation based on textual clarity.
7. Mark any entity that has no detected relations as an isolated entity (graph-theory: isolated vertex; reported in `orphan_entities`).
8. Only extract relations that are explicitly stated or clearly implied in the source text.

### map-to-schema

1. Transform the identified entities into structured JSON that conforms to the target schema.
2. Perform direct mapping from entity text to schema field value, normalizing types.
3. Apply type coercion to convert string values to the type required by the schema (string, number, boolean, array, object).
4. Resolve conflicts if multiple entities map to the same field by selecting the most confident or most recent.
5. Infer missing but required fields from surrounding context if possible.
6. Report fields that cannot be populated from available information as unresolved fields.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `extract-relations.j2` | KnowAct | Open Information Extraction (Banko et al. 2007): extract binary relations between identified entities as `(arg1, relation, arg2)` tuples with free-text predicates, trigger spans, and coreference resolution. NOT RDF triples — predicates are free-text verb phrases, not typed IRIs. |
| `identify-entities.j2` | KnowAct | Identify entities in unstructured text against a target schema with extraction hints. Tracks unmapped text and entity count. |
| `map-to-schema.j2` | KnowAct | Map extracted entities and relations to a target schema. Resolves field mappings, infers missing fields from context, and reports field-level coverage and unresolved fields. |

## Constraints

- `extract-relations.j2`: Public.
- `identify-entities.j2`: Public.
- `map-to-schema.j2`: Public.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
