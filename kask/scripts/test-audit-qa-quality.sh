#!/usr/bin/env bash
# Bounded contract controls for the operator-approved grounding audit repair.
# No inference, corpus mutation, ingestion or production run. All fixtures
# are synthetic except optional caller-supplied read-only control inputs.
set -euo pipefail
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf -- "$WORK"' EXIT
AUDIT="$SCRIPT_DIR/audit-qa-quality.sh"
passed=0
jq -nc '{entity_ref:"chunk:a",source:"book-a",text:"Revenue rose from ten to twelve units. Exact sources preserve meaningful evidence for every reader."},
        {entity_ref:"chunk:b",source:"book-b",text:"Costs fell from nine to seven units."}' > "$WORK/chunks.jsonl"
jq -nc '{chunk_ref:"chunk:a",source:"book-a",qa_type:"factual",
         response:{instruction:"What happened to revenue?",
                   output:(["Revenue rose from ten to twelve units."]|tojson),
                   evidence_quotes:["Revenue rose from ten to twelve units."]}}' > "$WORK/base.jsonl"

check() {
    local name=$1 expected=$2 input=$3 chunks=$4 assertion=$5 status=0
    bash "$AUDIT" "$input" "$chunks" > "$WORK/report.json" || status=$?
    if [[ $status != "$expected" ]]; then
        echo "FAIL $name: exit $status, expected $expected" >&2
        cat "$WORK/report.json" >&2
        exit 1
    fi
    if ! jq -e "$assertion" "$WORK/report.json" >/dev/null; then
        echo "FAIL $name: $assertion" >&2
        cat "$WORK/report.json" >&2
        exit 1
    fi
    # Count reconciliation and canonical provenance apply to EVERY control.
    jq -e '.launch_authorized == false and
      (.structural_counts | .input_rows == (.qa_rows+.parse_error_rows+.generation_error_rows+.invalid_shape_rows)
       and .source_input_rows == (.valid_source_rows+.source_error_rows)) and
      all(.rows[].verified_claims[]; (.why|length)>=40 and
        (.provenance as $p | ["tool_verified","platform_derived","model_inference","unavailable","tool_no_match","pending_check","rejected"]|index($p)!=null))' \
      "$WORK/report.json" >/dev/null
    passed=$((passed+1))
    echo "ok $passed - $name"
}
mutate() { jq -c "$1" "$WORK/base.jsonl" > "$WORK/case.jsonl"; }
check 'fully covered literal citations score one (1e-12 tolerance)' 0 "$WORK/base.jsonl" "$WORK/chunks.jsonl" \
    '(.quality_evidence.fact_score-1 | fabs)<1e-12 and .rows[0].fact_score_breakdown=={sar:1,cvr:1,hfr:1,nlr:1,claims_checked:1} and .rows[0].narrative_check.status=="not_applicable"'
mutate '.response.output="Sales increased by a fifth."'
check 'paraphrase retains model inference despite exact evidence' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].verified_claims[-1].provenance=="model_inference" and .rows[0].verified_claims[0].provenance=="tool_verified" and .rows[0].fact_score_breakdown.nlr==null and .quality_evidence.fact_score==null'
mutate '.response.output=.response.evidence_quotes[0]'
check 'ordinary verbatim prose does not become narrative-free' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].narrative_check.status=="unperformed" and (.data_gaps|index("narrative_check_unperformed")!=null)'
mutate '.response.evidence_quotes=["Revenue doubled to twenty units."] | .response.output=(.response.evidence_quotes|tojson)'
check 'fabricated citation rejected and weighted score is 0.25' 1 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].verified_claims[0].provenance=="rejected" and (.rows[0].fact_score-0.25|fabs)<1e-12 and (.hallucination_findings|length)>0'
mutate '.source="book-b"'
check 'correct quote attributed to wrong source is rejected' 1 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].source_state=="wrong_source" and .rows[0].verified_claims[0].provenance=="rejected" and .quality_evidence.fact_score==null'
mutate '.response.evidence_quotes=["Costs fell from nine to seven units."] | .response.output=(.response.evidence_quotes|tojson)'
check 'quote found only in another chunk cannot ground this citation' 1 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].verified_claims[0].provenance=="rejected"'
mutate '.chunk_ref="missing"'
check 'missing chunk propagates null rather than zero' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].verified_claims[0].provenance=="unavailable" and .rows[0].fact_score_breakdown.sar==null and .rows[0].fact_score_breakdown.cvr==null and .rows[0].fact_score_breakdown.hfr==null'
mutate 'del(.source)'
check 'missing source identity is explicit gap' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '(.data_gaps|index("missing_source_metadata"))!=null'
mutate '.response.evidence_quotes=[]'
check 'zero citations have null CVR and fact score' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].fact_score_breakdown.cvr==null and (.data_gaps|index("zero_citations"))!=null and .quality_evidence.fact_score==null'
mutate '.response.output="" | .response.evidence_quotes=[]'
check 'zero claims cannot produce a clean measurement' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].fact_score_breakdown.claims_checked==0 and (.data_gaps|index("zero_claims"))!=null and .quality_evidence.fact_score==null'
mutate '.response.evidence_quotes=[""]'
check 'empty citation never matches every string' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].verified_claims[0].provenance=="pending_check" and .rows[0].verified_claims[0].cross_check.performed==false'
mutate '.response.evidence_quotes=[42]'
check 'non-string citation is surfaced, not silently omitted' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.rows[0].citation_count==1 and (.data_gaps|index("invalid_citation"))!=null'
mutate '.response | del(.evidence_quotes)'
check 'flat training metadata loss stays visible' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '(.data_gaps|index("flat_training_metadata_unavailable"))!=null and .quality_evidence.qa_type.evenness==null and .quality_evidence.fact_score==null'
mutate '.response.output="The derived growth rate is 20%." | .response.provenance="platform_derived" | .response.cross_check="(+ 10 2)"'
check 'unperformed derivation cannot be platform derived' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    'all(.rows[0].verified_claims[];.provenance!="platform_derived") and .rows[0].verified_claims[-1].provenance=="model_inference"'
mutate '.response.output="Sales increased."'
cat "$WORK/base.jsonl" "$WORK/case.jsonl" > "$WORK/mixed.jsonl"
check 'partial row aggregate cannot hide unperformed checks' 2 "$WORK/mixed.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.rows_measured==1 and .quality_evidence.fact_score==null'
{ cat "$WORK/base.jsonl"; printf '%s\n' '{broken' '{"error":"provider failed"}' '' 'null' '{"response":{"output":42}}'; } > "$WORK/mixed.jsonl"
check 'malformed blank error and schema rows reconcile' 2 "$WORK/mixed.jsonl" "$WORK/chunks.jsonl" \
    '.structural_counts.input_rows==6 and .structural_counts.qa_rows==1 and .structural_counts.parse_error_rows==2 and .structural_counts.generation_error_rows==1 and .structural_counts.invalid_shape_rows==2 and .quality_evidence.fact_score==null'
: > "$WORK/empty.jsonl"
check 'empty QA input is a gap' 2 "$WORK/empty.jsonl" "$WORK/chunks.jsonl" \
    '(.data_gaps|index("zero_qa_rows"))!=null and .quality_evidence.fact_score==null'
check 'empty source input is a gap' 2 "$WORK/base.jsonl" "$WORK/empty.jsonl" \
    '(.data_gaps|index("missing_sources"))!=null and .quality_evidence.fact_score==null'
{ cat "$WORK/chunks.jsonl"; printf '%s\n' '{broken'; } > "$WORK/bad-chunks.jsonl"
check 'malformed source rows cannot silently disappear' 2 "$WORK/base.jsonl" "$WORK/bad-chunks.jsonl" \
    '.structural_counts.source_error_rows==1 and .quality_evidence.fact_score==null'
cat "$WORK/chunks.jsonl" "$WORK/chunks.jsonl" > "$WORK/bad-chunks.jsonl"
check 'duplicate source identifiers are ambiguous' 2 "$WORK/base.jsonl" "$WORK/bad-chunks.jsonl" \
    '.rows[0].source_state=="ambiguous_source_chunk" and (.data_gaps|index("duplicate_source_refs"))!=null'
jq -c '. as $r | ["factual","conceptual","analyze","evaluate","create"][] | . as $t | $r | .qa_type=$t' \
    "$WORK/base.jsonl" > "$WORK/types.jsonl"
check 'qa_type evenness counts all five canonical levels' 0 "$WORK/types.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.qa_type.evenness==1 and .quality_evidence.source_diversity.identified_sources==1'
mutate 'del(.qa_type)'
cat "$WORK/base.jsonl" "$WORK/case.jsonl" > "$WORK/mixed.jsonl"
check 'missing final-row metadata cannot hide earlier metadata' 2 "$WORK/mixed.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.qa_type.missing==1 and .quality_evidence.qa_type.counts[0].count==1 and .quality_evidence.qa_type.evenness==null'
mutate '.qa_type="invented"'
check 'unknown Bloom label is disclosed' 2 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.qa_type.unknown==["invented"] and .quality_evidence.qa_type.evenness==null and (.data_gaps|index("unknown_qa_type"))!=null'
mutate '.response.output="Exact sources preserve meaningful evidence for every reader. Exact sources preserve meaningful evidence for every reader."'
cat "$WORK/case.jsonl" "$WORK/case.jsonl" > "$WORK/mixed.jsonl"
check 'six-gram repetition within an answer counts once per document' 2 "$WORK/mixed.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.boilerplate.answer_documents==2 and all(.quality_evidence.boilerplate.top[];.document_frequency==2)'
jq -nc '{instruction:"A?",output:"one two three"},{instruction:"B?",output:"four five six"}' > "$WORK/mixed.jsonl"
check 'six-grams never span answer boundaries' 2 "$WORK/mixed.jsonl" "$WORK/chunks.jsonl" \
    '.quality_evidence.boilerplate.repeated_sixgrams==0'
mutate '.response.evidence_quotes=["Revenue rose from ten to twelve units.","Invented claim."] | .response.output=(.response.evidence_quotes|tojson)'
check 'mixed verified and rejected citations obey all four weights' 1 "$WORK/case.jsonl" "$WORK/chunks.jsonl" \
    '(.rows[0].fact_score-0.625|fabs)<1e-12 and .rows[0].fact_score_breakdown=={sar:0.5,cvr:0.5,hfr:0.5,nlr:1,claims_checked:2}'
echo "PASS: $passed bounded audit controls"
