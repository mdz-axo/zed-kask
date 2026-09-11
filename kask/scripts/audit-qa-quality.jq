# Mechanical subset of .agents/skills/grounding-verify/SKILL.md, not an
# IS/OUGHT extractor or semantic reviewer. No caller-supplied tier is trusted.
def nonblank: type == "string" and test("\\S");
def ratio($n; $d): if $d == 0 then null else $n / $d end;
def expected_types: ["factual","conceptual","analyze","evaluate","create"];
def score($m):
  if $m.claims_checked == 0 or ([$m.sar,$m.cvr,$m.hfr,$m.nlr] | any(. == null))
  then null else 0.30*$m.sar + 0.25*$m.cvr + 0.20*$m.hfr + 0.25*$m.nlr end;
def finding($code; $detail):
  {code:$code, severity:"high", detail:$detail,
   severity_basis:"Conservative release hold: semantic materiality has not been classified by this mechanical subset.",
   disposition:"Re-examine the answer and its citation; do not release on this audit."};
def source_ok: type == "object" and (.entity_ref | nonblank)
  and (.source | nonblank) and (.text | nonblank);
def citation_ok: type == "object" and (keys | sort) == ["chunk_ref","quote","source"]
  and (.chunk_ref | nonblank) and (.source | nonblank) and (.quote | nonblank);
def identify($index; $ref):
  (if $ref.chunk_ref | nonblank then ($index[$ref.chunk_ref] // []) else [] end) as $matches
  | if ($ref.chunk_ref | nonblank | not) or ($ref.source | nonblank | not) then "missing_source_metadata"
    elif ($matches|length) == 0 then "missing_source_chunk"
    elif ($matches|length) > 1 then "ambiguous_source_chunk"
    elif $matches[0].source != $ref.source then "wrong_source"
    else "identified" end;
def registry($id; $text; $role; $ref; $tier; $why; $check):
  {claim_id:$id, text:$text, role:$role, source_reference:$ref,
   provenance:$tier,
   strength:(if $tier == "tool_verified" or $tier == "platform_derived" then 2
             elif $tier == "model_inference" then 1 else 0 end),
   why:$why, cross_check:$check};
def audit_row($index):
  . as $raw | (if .value | type == "object" then .value else {} end) as $r
  | ($r | has("response")) as $generated
  | (if $generated then $r.response else $r end) as $body
  | (if $body | type == "object" then $body else {} end) as $b
  | ($b.output // null) as $out
  | (if $out | type == "string" then $out else "" end) as $answer
  | (if $b.evidence_quotes | type == "array" then $b.evidence_quotes else [] end) as $quotes
  | {chunk_ref:($r.chunk_ref // null), source:($r.source // null)} as $ref
  | identify($index; $ref) as $source_state
  | ($raw.parse_error != null) as $parse_error
  | (($r.error? != null) or ($b.error? != null)) as $generation_error
  | (($b.instruction | nonblank) and ($out | nonblank)) as $shape_ok
  | ("row-\($raw.line)") as $id
  | [$quotes | to_entries[] | . as $entry
      | (if .value | type == "object" then .value else {} end) as $q
      | {chunk_ref:($q.chunk_ref // null),source:($q.source // null)} as $citation_ref
      | identify($index; $citation_ref) as $citation_source_state
      | (if ($entry.value | citation_ok | not) then "pending_check"
         elif $citation_source_state == "wrong_source" then "rejected"
         elif $citation_source_state != "identified" then "unavailable"
         elif ($index[$q.chunk_ref][0].text | contains($q.quote)) then "tool_verified"
         else "rejected" end) as $tier
      | registry("\($id):citation-\($entry.key+1)"; ($q.quote // null); "cited_substring"; $citation_ref; $tier;
          (if $tier == "pending_check" then "The citation must contain only nonblank chunk_ref, source and quote strings; no valid citation check can run."
           elif $citation_source_state == "wrong_source" then "The identified chunk belongs to a different source; the supplied citation attribution is rejected."
           elif $tier == "unavailable" then "The named source was not consulted: its metadata or unique chunk text is unavailable in this audit."
           elif $tier == "tool_verified" then "This nonempty cited substring occurs exactly in the uniquely identified chunk with matching source metadata."
           else "The identified source was consulted and this exact cited substring was absent; the citation is rejected." end);
          {method:"exact_nonempty_substring_and_source_identity", source_state:$citation_source_state,
           performed:($tier == "tool_verified" or $tier == "rejected"),
           matched:(if $tier == "tool_verified" then true elif $tier == "rejected" then false else null end)})
      + {provisional_provenance:"tool_verified"}] as $citations
  # Only a literal JSON array of canonical citation objects is citation-only.
  # Matching ordinary prose does NOT erase its narrative field. This is a
  # deliberately narrow format, not a heuristic about a sentence's meaning.
  | (try ($answer | fromjson) catch null) as $literal
  | (if ($literal | type) == "array" and ($literal|length) > 0
     then all($literal[]; citation_ok and (. as $cell | $quotes | index($cell) != null))
     else false end) as $citation_only
  | ($citations + (if ($answer | nonblank) and ($citation_only | not) then
      [registry("\($id):answer"; $answer; "unclassified_answer_candidate"; $ref;
        (if $source_state == "identified" then "model_inference" else "unavailable" end);
        (if $source_state == "identified" then "Answer synthesis is not verified by a matching evidence quote; declarative claim extraction and semantic support remain unperformed."
         else "The named source was not consulted under the supplied identity; answer grounding cannot be established from these inputs." end);
        {method:"semantic_support",performed:false,matched:null})]
      else [] end)) as $claims
  | [if $parse_error then "invalid_json" else empty end,
     if $generation_error then "generation_error" else empty end,
     if ($shape_ok|not) then "invalid_qa_shape" else empty end,
     if ($b.evidence_quotes | type) != "array" then "missing_or_invalid_evidence_metadata" else empty end,
     if $source_state != "identified" then $source_state else empty end,
     if ($quotes|length) == 0 then "zero_citations" else empty end,
     if any($quotes[]; citation_ok|not) then "invalid_citation" else empty end,
     ($citations[] | select(.cross_check.source_state != "identified") | .cross_check.source_state),
     if ($claims|length) == 0 then "zero_claims" else empty end,
     if ($r.qa_type | nonblank | not) then "missing_qa_type"
     elif ($r.qa_type as $type | expected_types | index($type)) == null then "unknown_qa_type" else empty end,
     if ($citation_only|not) then "declarative_claim_extraction_unperformed", "narrative_check_unperformed" else empty end] as $gaps
  | [(if $source_state == "wrong_source" then finding("wrong_source"; $ref) else empty end),
     ($citations[] | select(.provenance == "rejected")
       | finding("citation_rejected"; {claim_id,text,source_reference,cross_check}))] as $findings
  | ($citation_only and ($claims|length) > 0 and $source_state == "identified"
     and all($citations[]; .cross_check.performed)) as $measurable
  | {sar:(if $measurable then ratio(([$claims[]|select(.strength>=1)]|length); ($claims|length)) else null end),
     cvr:(if all($citations[]; .cross_check.performed) then ratio(([$citations[]|select(.provenance=="tool_verified")]|length); ($citations|length)) else null end),
     hfr:(if $measurable then ratio(([$claims[]|select(.provenance!="rejected")]|length); ($claims|length)) else null end),
     nlr:(if $citation_only then 1 else null end),
     claims_checked:([$citations[]|select(.cross_check.performed)]|length)} as $metrics
  | (if ($gaps|length)>0 then null else score($metrics) end) as $fact
  | {line:$raw.line, row_kind:(if $parse_error then "parse_error" elif $generation_error then "generation_error"
                             elif ($shape_ok|not) then "invalid_shape" else "qa" end),
     parse_error:$raw.parse_error, generation_error:($r.error // $b.error // null),
     instruction:($b.instruction // null), answer:$out, chunk_ref:$ref.chunk_ref, source:$ref.source,
     qa_type:($r.qa_type // null), source_state:$source_state,
     extraction_scope:(if $citation_only then "literal_citation_array" else "whole_answer_candidate_only" end),
     verified_claims:$claims, citation_count:($citations|length),
     fact_score_breakdown:$metrics, fact_score:$fact,
     data_gaps:($gaps + if $fact == null then ["fact_score_measurement_failed"] else [] end | unique),
     hallucination_findings:$findings,
     narrative_check:{status:(if $citation_only then "not_applicable" else "unperformed" end),
                      fields:(if $citation_only then 0 else 1 end), leaks:null},
     verification_scope_limitations:[
       "No semantic IS/OUGHT classification, entailment, completeness, reasoning quality, plausible fabrication or subject-lens check is performed.",
       "No numeric derivations are verified; platform_derived is never assigned. No cross-source congruence rules were supplied.",
       (if $citation_only then "no narrative fields — NLR vacuous; only an explicit literal array of structured citations is covered, not a semantic claim inventory."
        else "Ordinary output prose remains narrative even when it equals a source quote; extraction and narrative leak checks require semantic review." end)]};

([$chunks[] | select(.parse_error == null and (.value | source_ok)) | .value]) as $valid_sources
| ($valid_sources | group_by(.entity_ref) | map({key:.[0].entity_ref,value:.}) | from_entries) as $index
| [$qa[] | audit_row($index)] as $rows
| [$chunks[] | select(.parse_error != null or (.value | source_ok | not))
   | {line,parse_error,gap:"invalid_source_row"}] as $source_errors
| [$index | to_entries[] | select((.value|length)>1) | .key] as $duplicate_refs
| [$rows[] | .data_gaps[]] as $row_gaps
| ([if ($rows|length)==0 then "zero_qa_rows" else empty end,
    if ($valid_sources|length)==0 then "missing_sources" else empty end,
    if ($source_errors|length)>0 then "invalid_source_rows" else empty end,
    if ($duplicate_refs|length)>0 then "duplicate_source_refs" else empty end] + $row_gaps | unique) as $gaps
| ([$rows[].hallucination_findings[]]) as $findings
| ([$rows[] | select(.row_kind=="qa")]) as $qa_rows
| ([$qa_rows[] | select(.qa_type | nonblank) | .qa_type]) as $types
| expected_types as $expected_types
| ([$expected_types[] | . as $type | {qa_type:$type,count:([$types[]|select(.==$type)]|length)}]) as $type_counts
| ([$types[] | select(. as $t | $expected_types | index($t) == null)] | unique) as $unknown_types
| ([$qa_rows[] | select(.source_state=="identified") | .source] | unique) as $covered_sources
| ($valid_sources | map(.source) | unique) as $all_sources
| {
  audit_kind:"mechanical_grounding_verify_subset", decoupling:"in_thread",
  inputs:{qa:$qa_path,chunks:$chunks_path},
  target_fields:["output", "evidence_quotes", "response.output", "response.evidence_quotes"],
  instruction_scope:"Instruction text is checked structurally only; factual premises in questions are not semantically verified.",
  status:(if ($findings|length)>0 then "findings" elif ($gaps|length)>0 then "incomplete" else "mechanical_checks_completed" end),
  launch_authorized:false,
  launch_gate:"Not evaluated by this subset: every applicable grounding-verify report must reach >=0.80, no high/critical findings, all missing checks resolved, structural targets reconciled, and operator semantic judgment recorded. A partial aggregate never authorizes pilot/full runs.",
  structural_counts:{input_rows:($rows|length),qa_rows:($qa_rows|length),
    parse_error_rows:([$rows[]|select(.row_kind=="parse_error")]|length),
    generation_error_rows:([$rows[]|select(.row_kind=="generation_error")]|length),
    invalid_shape_rows:([$rows[]|select(.row_kind=="invalid_shape")]|length),
    source_input_rows:($chunks|length),valid_source_rows:($valid_sources|length),source_error_rows:($source_errors|length),
    duplicate_instruction_rows:($qa_rows|map(.instruction|ascii_downcase)|length - (unique|length))},
  quality_evidence:{
    fact_score:(if ($rows|length)==0 or ($gaps|length)>0 or any($rows[];.fact_score==null) then null
                else ([$rows[].fact_score]|add/length) end),
    aggregation:"Mean of fully measurable row scores only when every input row is measurable and no data gaps remain; never a mean of the passing subset.",
    rows_measured:([$rows[]|select(.fact_score!=null)]|length),
    weights:{sar:0.30,cvr:0.25,hfr:0.20,nlr:0.25},
    boilerplate:$boilerplate[0],
    qa_type:{expected:$expected_types,counts:$type_counts,unknown:$unknown_types,
      missing:([$qa_rows[]|select(.qa_type|nonblank|not)]|length),
      evenness:(if ($types|length)==0 or ($unknown_types|length)>0 or ($types|length)!=($qa_rows|length) then null
                else ($type_counts|map(.count)) as $counts | ($counts|min)/($counts|max) end),
      limitation:"Requested qa_type metadata, not verified Bloom difficulty; min/max count evenness includes absent expected types."},
    source_diversity:{identified_sources:($covered_sources|length),available_sources:($all_sources|length),
      coverage:(if ($qa_rows|length)==0 or any($qa_rows[];.source_state!="identified") then null
                else ratio(($covered_sources|length);($all_sources|length)) end),
      missing_sources:($all_sources-$covered_sources),
      limitation:"Source identity coverage is not semantic subject diversity; imposed-frame review remains unperformed."}},
  data_gaps:$gaps, source_errors:$source_errors, duplicate_source_refs:$duplicate_refs,
  hallucination_findings:$findings, rows:$rows,
  verification_scope_limitations:["Read-only Bash/jq audit; no model inference or external source retrieval.",
    "Only explicit citation bytes and named chunk/source identity are mechanically verified. Answer prose cannot inherit citation provenance.",
    "No production-run coverage or pilot/full authorization is asserted; operator review must resolve semantic checks."]
}
