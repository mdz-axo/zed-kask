#!/usr/bin/env bash
# Classification output contract (published-term-resolution-v2) — the shared
# gate of the classification lifecycle. Sourced, never executed, by
# run-corpus-classification-queue.sh (terminal validation of every processed
# unit) and reconcile-corpus-classification-unit.sh (validation of a failed
# unit's partial output before recovery). One owner of the contract: when the
# protocol moves v2 -> v3, this file changes, not two drifting copies — a
# recovered unit that passed one tool's check and failed the other's was a
# real incident class.
#
# classification_shape_ok <rows> <output-jsonl>
#   Exits 0 when the output satisfies the contract: exactly <rows> rows,
#   unique entity_refs, and per-row classification states/shapes (classified
#   rows carry the v2 protocol, 3-5 candidate terms, object tags, array
#   concepts; failed rows carry a nonempty reason). Exits nonzero otherwise.
#   A pure predicate — the failure message and terminal handling stay with
#   each caller.
classification_shape_ok() {
    jq -s -e --argjson rows "$1" '
        length == $rows and
        ([.[].entity_ref] | length == (unique | length)) and
        all(.[];
          if .classification.status == "classified" then
            .classification.ontology_protocol == "published-term-resolution-v2" and
            (.candidate_terms | type == "array" and length >= 3 and length <= 5) and
            (.ontology_tags | type == "object") and
            (.concepts | type == "array")
          elif .classification.status == "failed" then
            (.classification.reason | type == "string" and length > 0)
          else false end)
    ' "$2" >/dev/null
}