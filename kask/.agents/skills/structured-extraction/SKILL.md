---
name: structured-extraction
visibility: public
description: "Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.
"
---

# Structured Extraction

Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.


## When to Use

- When you need to identify entities in unstructured text and map them to a target schema using extraction hints.
- When you need to extract semantic relations between identified entities as subject-predicate-object triples.
- When you need to map extracted entities and relations to a target JSON schema, resolving field mappings and inferring missing fields.
- When you need to compute a normalized convergence metric for structured-extraction PDCA cycles to evaluate schema coverage and unresolved fields.

## Instructions

### extract-relations

1. Extract a relation triple for each pair of entities that have a meaningful relationship in the source text.
2. Identify the Subject as the entity performing or originating the relationship.
3. Identify the Predicate as the relationship type (a short verb phrase of 1-3 words).
4. Identify the Object as the entity receiving or being the target of the relationship.
5. Assign a confidence score (0.0-1.0) for each relation based on textual clarity.
6. Mark any entity that has no detected relations as an orphan.
7. Only extract relations that are explicitly stated or clearly implied in the source text.

### identify-entities

1. Scan the source text for any information that maps to the fields defined in the target schema.
2. Extract the exact text from the source for each entity found.
3. Classify the entity type (person, organization, date, quantity, location, etc.).
4. Map the entity to the corresponding schema field it populates.
5. Assign a confidence score (0.0-1.0) reflecting genuine extraction certainty.
6. Record the location of the entity in the source text using character offsets.
7. Identify any text segments that contain structured information but do not clearly map to a specific schema field as unmapped text.

### map-to-schema

1. Transform the identified entities into structured JSON that conforms to the target schema.
2. Perform direct mapping from entity text to schema field value, normalizing types.
3. Apply type coercion to convert string values to the type required by the schema (string, number, boolean, array, object).
4. Resolve conflicts if multiple entities map to the same field by selecting the most confident or most recent.
5. Infer missing but required fields from surrounding context if possible.
6. Report fields that cannot be populated from available information as unresolved fields.

### structured-extraction-convergence-check

1. Measure convergence on a scale of [0,1] where 0 means schema coverage is acceptable and unresolved fields are low-risk/non-critical.
2. Score how much work remains based on the provided mapping result.
3. Return the convergence metric, method, rationale, and any blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `extract-relations.j2` | KnowAct | Extract semantic relations between identified entities. Links entities via subject-predicate-object triples within context.  |
| `identify-entities.j2` | KnowAct | Identify entities in unstructured text against a target schema with extraction hints. Tracks unmapped text and entity count.  |
| `map-to-schema.j2` | KnowAct | Map extracted entities and relations to a target schema. Resolves field mappings, infers missing fields from context, and reports field-level coverage and unresolved fields.  |
| `structured-extraction-convergence-check.j2` | KnowAct | Compute normalized convergence metric for structured-extraction PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `extract-relations.j2`: Public.
- `identify-entities.j2`: Public.
- `map-to-schema.j2`: Public.
- `structured-extraction-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
