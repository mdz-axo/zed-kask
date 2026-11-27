---
name: structured-extraction
description: "Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.
"
audience: user
---

# Structured Extraction

Structured data extraction from unstructured text. Identifies entities, extracts inter-entity relations, and maps extracted data to target schemas with field-level coverage and inferred field population.

## Initial and target condition

- **Initial condition:** the exact source text, target schema and extraction hints, with no entity, relation or inferred field accepted before substring and schema checks.
- **Target condition:** every retained entity/relation trigger passes `SE-1`, `SE-2` reconciles counts/orphans, `SE-3` computes schema-only coverage, and `unresolved_fields` is empty. A nonempty unresolved set re-enters entity identification once with those field names as hints; fields still absent after the second pass remain explicit and are never fabricated.

## When to Use

- When you need to identify entities in unstructured text and map them to a target schema using extraction hints.
- When you need to extract binary relations between identified entities as OpenIE `(arg1, relation, arg2)` tuples (Banko et al. 2007) — free-text predicates, not RDF triples. For closed-type RE (ACE2005/TACRED/DocRED inventories), pre-populate the predicate vocabulary in `extraction_hints`.
- When you need to map extracted entities and relations to a target JSON schema, resolving field mappings and inferring missing fields.

## When NOT to Use

- Verifying claims against sources — use `grounding-verify`; extraction produces candidates, not verdicts.
- Semantic document search — use the corpus tools (`corpus_query`); retrieval is not extraction.
- Validating already-structured data against a schema — just validate; there is nothing to extract.

## Instructions

### identify-entities

1. Render `structured-extraction/identify-entities` with `source_text`, `target_schema`, and `extraction_hints`. Scan the source text for any information that maps to the fields defined in the target schema.
2. Extract the exact text from the source for each entity found.
3. Classify the entity type (person, organization, date, quantity, location, etc.).
4. Map the entity to the corresponding schema field it populates.
5. Assign a confidence score (0.0-1.0) reflecting genuine extraction certainty.
6. Record the approximate location of the entity in the source text using character offsets (a locating hint; the exact text is what is verified).
7. Identify any text segments that contain structured information but do not clearly map to a specific schema field as `unmapped_text`.

### extract-relations

1. Render `structured-extraction/extract-relations` with `source_text` and `entities`. Open Information Extraction (OpenIE, Banko et al. 2007): extract a binary relation tuple for each pair of entities that have a meaningful relationship in the source text. These are `(arg1, relation, arg2)` tuples with free-text predicates — NOT RDF triples (W3C RDF 1.1), which require typed IRI predicates from a vocabulary.
2. Identify the Subject (arg1) as the entity performing or originating the relationship (after coreference resolution — CoNLL-2012).
3. Identify the Predicate (relation) as a short verb phrase (1-3 words) extracted from the source. For closed-type RE, use ACE2005 (6 types/17 subtypes, LDC English Relations Guidelines v6.2), TACRED (41 types), or DocRED (96 Wikidata relations) — this skill is open by default.
4. Identify the Object (arg2) as the entity receiving or being the target of the relationship.
5. Record the `trigger_span` (character offsets of the relation signal word(s), standoff-annotation style) for every relation.
6. Assign a confidence score (0.0-1.0) for each relation based on textual clarity.
7. Only extract relations that are explicitly stated or clearly implied in the source text.

### Verify and Graph Counts (`SE-1`, `SE-2` — D in `lisp_eval`)

1. **Gate `SE-1` (Substring verification):** Before mapping, check every entity's `entity_text` and every relation's `source_evidence` / trigger against `source_text` with `lisp_eval`:
   - form: `(and (stringp entity_text) (> (length entity_text) 0) (stringp source_text) (<= (length entity_text) (length source_text)) (string-contains entity_text source_text))`
   - env: `{"entity_text": "<extracted entity or trigger text>", "source_text": "<verbatim source text>"}`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"SE-1","case":"present","env":{"entity_text":"Acme Corp","source_text":"Acme Corp reported revenue of $12M in 2024."},"expected":true}
     {"gate":"SE-1","case":"fabricated","env":{"entity_text":"Globex Industries","source_text":"Acme Corp reported revenue of $12M in 2024."},"expected":false}
     {"gate":"SE-1","case":"empty-entity","env":{"entity_text":"","source_text":"Acme Corp reported revenue of $12M in 2024."},"expected":false}
     {"gate":"SE-1","case":"short-ascii-source","env":{"entity_text":"Acme Corp","source_text":"Acme"},"expected":false}
     ```
   Known `hkask-lisp` boundary: `length` counts Unicode characters while `string-contains` compares UTF-8 byte lengths, so the length pre-check prevents short-source crashes for ASCII strings while a multi-byte UTF-8 needle longer in bytes than `source_text` still surfaces a runtime error (meaning rejection — never swap the arguments: the swapped call can return true and pass a fabrication through the gate). Drop anything that fails and list it as `rejected_extractions`; a model-produced string that is not in the source is a fabrication, not an entity. Offsets are not checked (the interpreter has no substring-at-offset builtin), so they stay approximate — a consumer that needs an exact offset (e.g. `grounding-verify`) must locate the verified text itself.
2. **Gate `SE-2` (Entity/relation counts and `orphan_entities`):** Over the verified `entities` and `relations`, deterministically compute `[entity_count, relation_count, orphan_entities]` (isolated vertices — entities appearing as neither `subject` nor `object` in any relation) via `lisp_eval`:
   - form: `(begin (define is-array (lambda (x) (or (eq x (list)) (and (listp x) (not (is_null x)))))) (define in-rels (lambda (name rels) (if (is_null rels) false (let ((r (car rels))) (or (and (listp r) (stringp (assoc "subject" r)) (string= name (assoc "subject" r))) (and (listp r) (stringp (assoc "object" r)) (string= name (assoc "object" r))) (in-rels name (cdr rels))))))) (define orphans (lambda (es rels) (if (is_null es) (list) (let ((name (if (listp (car es)) (assoc "entity_text" (car es)) nil))) (if (and (stringp name) (in-rels name rels)) (orphans (cdr es) rels) (cons (if (stringp name) name "invalid_entity") (orphans (cdr es) rels))))))) (if (or (not (is-array entities)) (not (is-array relations))) (list 0 0 (list "invalid_inputs")) (list (length entities) (length relations) (orphans entities relations))))`
   - env: `{ "entities": <verified entities array>, "relations": <verified relations array> }`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"SE-2","case":"one-orphan-missing-object","env":{"entities":[{"entity_text":"Alice"},{"entity_text":"Bob"}],"relations":[{"subject":"Alice"}]},"expected":[2,1,["Bob"]]}
     {"gate":"SE-2","case":"null-entities-fails-closed","env":{"entities":null,"relations":[]},"expected":[0,0,["invalid_inputs"]]}
     ```

### map-to-schema

1. Render `structured-extraction/map-to-schema` with `source_text`, `target_schema`, `entities`, `relations`, and `unmapped_text`. Transform the identified entities into structured JSON (`extracted_data`) that conforms to the target schema.
2. Perform direct mapping from entity text to schema field value, normalizing types.
3. Apply type coercion to convert string values to the type required by the schema (string, number, boolean, array, object).
4. Resolve conflicts if multiple entities map to the same field by selecting the most confident or most recent.
5. Infer missing but required fields from surrounding context if possible (`inferred_fields`).
6. Report fields that cannot be populated from available information as `unresolved_fields`.
7. **Gate `SE-3` (Field coverage — D in `lisp_eval`):** Deterministically compute `field_coverage` (`total_fields`, `populated_fields`, `coverage_ratio`) from `extracted_data` and `schema_fields` (the property names of `target_schema`):
   - form: `(begin (define is-non-null-list (lambda (x) (or (eq x (list)) (and (listp x) (not (is_null x)))))) (define all-strings (lambda (xs) (if (is_null xs) true (and (stringp (car xs)) (all-strings (cdr xs)))))) (define pop-count (lambda (fs) (if (is_null fs) 0 (+ (if (is_null (assoc (car fs) extracted_data)) 0 1) (pop-count (cdr fs)))))) (if (or (not (is-non-null-list schema_fields)) (not (all-strings schema_fields)) (not (is-non-null-list extracted_data))) "invalid_inputs" (let ((total (length schema_fields)) (pop (pop-count schema_fields))) (list total pop (if (= total 0) 0.0 (/ pop total))))))`
   - env: `{ "schema_fields": <list of target_schema field names>, "extracted_data": <extracted_data object> }`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate":"SE-3","case":"one-of-two","env":{"schema_fields":["company","revenue"],"extracted_data":{"company":"Acme","revenue":null}},"expected":[2,1,0.5]}
     {"gate":"SE-3","case":"null-extracted-data","env":{"schema_fields":["company","revenue"],"extracted_data":null},"expected":"invalid_inputs"}
     {"gate":"SE-3","case":"null-schema-fields","env":{"schema_fields":null,"extracted_data":{}},"expected":"invalid_inputs"}
     {"gate":"SE-3","case":"non-string-schema-field","env":{"schema_fields":["company",123],"extracted_data":{"company":"Acme"}},"expected":"invalid_inputs"}
     ```
   Counts only schema-declared fields that are present and non-null in `extracted_data`; keys outside the schema and null values are not coverage, so `coverage_ratio` can never exceed 1.0.

**D/P labelling.** Entity identification, typing, relation extraction, confidence, field inference, and map-to-schema's type coercion and conflict resolution are P, critiqued by `SE-1` and by the operator; `SE-1` (substring check), `SE-2` (entity/relation counts and `orphan_entities`), `SE-3` (`field_coverage`), and the Convergence gate are D. Reference models: OpenIE (Banko et al. 2007, IJCAI) — anchored: `onto_anchor` → derived `open_information_extraction` (operator ruling 2026-10-14; verified 2026-11-02); ACE2005 standoff annotation and CoNLL-2012 coreference — unanchored, house rules: `onto_anchor` falls through to the coarse 5W1H core for ACE2005, CoNLL-2012, TACRED and DocRED (checked 2026-11-02; ruling path open), so their type inventories are named external conventions, not ontology-resolved terms.

### Convergence

Gate — call `lisp_eval` with:
- form: `(eq unresolved_fields (list))`
- env: `{ "unresolved_fields": <required schema fields still unpopulated> }`
- receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
  ```lisp-receipts
  {"gate":"convergence","case":"resolved","env":{"unresolved_fields":[]},"expected":true}
  {"gate":"convergence","case":"unresolved","env":{"unresolved_fields":["founded_date"]},"expected":false}
  {"gate":"convergence","case":"null-fails-closed","env":{"unresolved_fields":null},"expected":false}
  ```
- Bound: one re-entry — re-run identify-entities with the unresolved field
  names as extraction hints; fields that remain unresolved after the second
  pass are reported as unresolved (map-to-schema step 6), never fabricated.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `extract-relations.j2` | Open Information Extraction (Banko et al. 2007): extract binary relations between identified entities as `(arg1, relation, arg2)` tuples with free-text predicates, trigger spans, and coreference resolution. NOT RDF triples — predicates are free-text verb phrases, not typed IRIs. `relation_count` and `orphan_entities` are computed deterministically by `SE-2` (`lisp_eval`). |
| `identify-entities.j2` | Identify entities in unstructured text against a target schema with extraction hints and track `unmapped_text`. `entity_count` is computed deterministically by `SE-2` (`lisp_eval`). |
| `map-to-schema.j2` | Map extracted entities and relations to a target schema. Resolves field mappings, infers missing fields from context (`inferred_fields`), and reports `unresolved_fields`. `field_coverage` is computed deterministically by `SE-3` (`lisp_eval`). |

To render a template, call the `render_template` tool with the template ref (e.g., `structured-extraction/extract-relations`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `identify-entities.j2`: `source_text`, `target_schema`, `extraction_hints`
- `extract-relations.j2`: `source_text`, `entities`
- `map-to-schema.j2`: `source_text`, `target_schema`, `entities`, `relations`, `unmapped_text`

Copyable `render_template` contexts:

- `structured-extraction/identify-entities`: `{"source_text": "Acme Corp reported revenue of $12M in 2024.", "target_schema": {"company": "string", "revenue": "string", "year": "number"}, "extraction_hints": ["organization", "money", "date"]}`
- `structured-extraction/extract-relations`: `{"source_text": "Acme Corp reported revenue of $12M in 2024.", "entities": [{"entity_text": "Acme Corp", "type": "organization"}, {"entity_text": "$12M", "type": "money"}]}`
- `structured-extraction/map-to-schema`: `{"source_text": "Acme Corp reported revenue of $12M in 2024.", "target_schema": {"company": "string", "revenue": "string", "year": "number"}, "entities": [{"entity_text": "Acme Corp", "type": "organization"}], "relations": [{"subject": "Acme Corp", "relation": "reported", "object": "$12M"}], "unmapped_text": []}`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-11 audit):

- Substring verify, present: `{entity_text: "Acme Corp", source_text: "Acme
  Corp reported revenue of $12M in 2024."}` → `true` (needle-first: the
  entity text is the needle, the source the haystack).
- Substring verify, fabricated: `{entity_text: "Globex Industries", ...}` →
  `false` — a model-produced string not in the source is a fabrication, not
  an entity; it lands in `rejected_extractions`.
- Convergence gate, resolved: `{unresolved_fields: []}` → `true`.
- Convergence gate, unresolved: `{unresolved_fields: ["founded_date"]}` →
  `false` — one re-entry with the field names as hints, then reported, never
  fabricated.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Extraction produces candidates, not verdicts — verification belongs to
  `grounding-verify`.
- Predicates are free-text verb phrases (OpenIE), never RDF typed IRIs;
  closed-type RE pre-populates the predicate vocabulary in
  `extraction_hints`.
- Every entity text and relation trigger is substring-verified against the
  source before mapping; failures are rejected, not repaired.
- Character offsets are locating hints only (the interpreter has no
  substring-at-offset builtin) — a consumer needing exact offsets locates
  the verified text itself.
- Unresolved fields are reported after one re-entry, never fabricated.

