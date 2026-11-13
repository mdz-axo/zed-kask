# dme-measure.jq — measures gate inputs from files the dme-harness wrote.
# It decides nothing: every threshold and verdict lives in the lisp gate forms.
# Use: jq -n -c -L <this directory> --slurpfile ... 'include "dme-measure"; gate_dme3(...)'
#   rows     a probe rows file (JSONL), slurped
#   summary  the object `dme-harness summary --rows <file>` prints for that file
#   receipts the edit receipts leading from the base file to an arm's file, in order
def per_row: ["ts","problem_id","gold","prompt_tokens","new_tokens","wall_secs","tok_per_sec","hit_limit","extracted","extraction_via","correct","label","tail_distinct_ratio","seed","model","gguf_sha256"];
def need: ["cap","n_ctx","dtype","param_count","engine_version","harness","penalty_last_n","config_file","server_generation_settings"];
def cls: if .hit_limit == true and .correct == false then "h" elif .hit_limit == false and .correct == false then "w" elif .hit_limit == false and .correct == true then "c" else "x" end;
def good: type == "object" and (.problem_id | type) == "string" and (.seed | type) == "number" and cls != "x" and .label == ({h: "hit_limit", w: "wrong_complete", c: "correct_complete"}[cls]);
def rowlist: if type == "array" then . else [] end;
def fact: delpaths([per_row[] | [.]]) | if (.server_generation_settings | type) == "object" then del(.server_generation_settings.seed) else . end;
# distinct fact sets; the string pass removes exact repeats cheaply, the structural pass ignores key order
def distinct: unique_by(tojson) | unique;
def facts: rowlist | map(select(good) | fact) | distinct;
# Leaf paths as arrays, so a key that contains "." cannot collide with a nested path.
def leaves: [paths(type != "object" or length == 0) | select(all(.[]; type == "string"))];
def leaf($p): try (getpath($p[:-1]) | if type == "object" and has($p[-1]) then [.[$p[-1]]] else null end) catch null;
def differ($a; $b): ($a | if type == "object" then . else {} end) as $x | ($b | if type == "object" then . else {} end) as $y
  | (($x | leaves) + ($y | leaves) | unique) | map(select(. as $p | ($x | leaf($p)) != ($y | leaf($p))) | join("."));
def run($rows; $summary):
  ($rows | rowlist) as $r | ($r | map(select(good))) as $ok | ($ok | map(fact) | distinct) as $f | ($ok | map(cls)) as $c
  | ($ok | map(.gguf_sha256) | unique) as $h
  | { hit_limit: ($c | map(select(. == "h")) | length), wrong_complete: ($c | map(select(. == "w")) | length), correct_complete: ($c | map(select(. == "c")) | length) } as $n
  | { answers: ($r | length),
      malformed: (($r | length) - ($ok | length)),
      repeated_ids: (($ok | length) - ($ok | map(.problem_id) | unique | length)),
      counts: $n,
      # the harness's own second count of the same file: class counts, row count and model-file hash
      summary_agrees: (($summary | type) == "object" and ($summary.counts | type) == "object" and ($summary.counts | {hit_limit, wrong_complete, correct_complete}) == $n
                       and $summary.n == ($r | length) and ($h | length) == 1 and $summary.gguf_sha256 == $h[0]),
      fact_sets: ($f | length),
      differs_within: (if ($f | length) > 1 then differ($f[0]; $f[1]) else [] end),
      facts_missing: (need | map(select(. as $k | ($f[0] // {})[$k] == null))),
      model_files: $h }
  | .facts_missing_count = (.facts_missing | length);
def idx: rowlist | map(select(good)) | map({key: .problem_id, value: {s: .seed, c: cls, g: .gold, p: .prompt_tokens}}) | from_entries;
def pair($ra; $rb): ($ra | idx) as $a | ($rb | idx) as $b | [$a | keys_unsorted[] | select($b[.] != null)] as $both
  | { paired: ($both | length),
      seed_mismatch: ([$both[] | select($a[.].s != $b[.].s)] | length),
      gold_mismatch: ([$both[] | select($a[.].g != $b[.].g)] | length),
      prompt_mismatch: ([$both[] | select($a[.].p != $b[.].p)] | length),
      table: (reduce $both[] as $k ({hh:0,hw:0,hc:0,wh:0,ww:0,wc:0,ch:0,cw:0,cc:0}; .[$a[$k].c + $b[$k].c] += 1)) };
def receipt: if type != "object" then {} else { input_sha256, output_sha256, alpha, kind: (try .trigger.kind catch null), weight_min: (try .trigger.weight_min catch null), weight_max: (try .trigger.weight_max catch null), direction: (.direction | if type == "object" then {source, dim, sha256, mean} else null end), site: {layers, tensor, trigger},
    delta_l2: [.per_layer[]? | .edit_delta_l2?], collateral: ([.per_layer[]? | .unedited_elements_changed?] | if length == 0 or any(.[]; type != "number") then null else add end) } end;
def chain($rs; $from): reduce ($rs | rowlist)[] as $r ($from; if type == "string" and ($r | type) == "object" and . == $r.input_sha256 then $r.output_sha256 else null end);
def arm($base; $rows; $summary; $receipts):
  ($receipts | rowlist | map(receipt)) as $rc
  | run($rows; $summary) + {
      differs_from_base: differ(($base | facts)[0]; ($rows | facts)[0]),
      vs_base: pair($base; $rows),
      receipt_count: ($rc | length),
      chain_end: chain($receipts; ($base | rowlist | map(select(good) | .gguf_sha256) | unique | if length == 1 then .[0] else null end)),
      collateral: ($rc | map(.collateral) | if length == 0 or any(.[]; . == null) then null else add end),
      alphas: ($rc | map(.alpha) | unique),
      edits: ($rc | map({alpha, kind, weight_min, weight_max})) }
  | .differs_count = (.differs_from_base | length);
def nums: map(select(type == "number"));
# The direction record a receipt must carry (see the skill's Constraints): source, dim, sha256, mean.
def like($x; $y):
  ($x | rowlist | map(receipt)) as $a | ($y | rowlist | map(receipt)) as $b | ([($a | length), ($b | length)] | min) as $m
  | [range(0; $m) as $i | ($a[$i].alpha) as $p | ($b[$i].alpha) as $q | if ($p | type) == "number" and ($q | type) == "number" and $p != 0 then $q / $p else null end] as $ar
  | [range(0; $m) as $i | range(0; [($a[$i].delta_l2 | length), ($b[$i].delta_l2 | length)] | max) as $j | ($a[$i].delta_l2[$j]) as $p | ($b[$i].delta_l2[$j]) as $q | if ($p | type) == "number" and ($q | type) == "number" and $p != 0 then $q / $p else null end] as $sr
  | [$b[] | (.direction | if type == "object" then . else {} end) as $d | if ($d.mean | type) == "number" and ($d.dim | type) == "number" then (($d.mean | if . < 0 then -. else . end) * $d.dim) else null end] as $oc
  | { count_equal: (($a | length) == ($b | length) and $m > 0),
      directions_unrecorded: ([$a[], $b[] | select((.direction | type) != "object" or (.direction.sha256 | type) != "string")] | length),
      first_source: ($a | map(.direction.source? // null) | unique | if length == 1 then .[0] else null end),
      second_source: ($b | map(.direction.source? // null) | unique | if length == 1 then .[0] else null end),
      same_direction: (($a | map(.direction)) == ($b | map(.direction))),
      site_mismatches: ([range(0; $m) | select($a[.].site != $b[.].site)] | length),
      ratios_unmeasured: ([$ar[], $sr[] | select(. == null)] | length),
      alpha_ratio_min: ($ar | nums | min), alpha_ratio_max: ($ar | nums | max),
      size_ratio_min: ($sr | nums | min), size_ratio_max: ($sr | nums | max),
      second_offcentre_max: (if ($oc | length) == 0 or any($oc[]; . == null) then null else ($oc | max) end) };
def overlap($p; $t): ($t | rowlist | map(select(good) | {key: .problem_id, value: true}) | from_entries) as $b | [$p | rowlist[] | select(good) | select($b[.problem_id] == true)] | length;
def typed($t): if . == null then null elif type == $t then . else "mistyped" end;
# What the server applied (its echoed generation settings), the mode the config asked for, and the row's cap and penalty window.
def settings($rows): ($rows | facts) as $f | if ($f | length) != 1 then {} else $f[0]
  | (.config_file | if type == "object" then . else {} end) as $c | (.server_generation_settings | if type == "object" then . else {} end) as $e
  | { settings: { enable_thinking: $c.enable_thinking, temperature: $e.temperature, top_p: $e.top_p, top_k: $e.top_k, min_p: $e.min_p, presence_penalty: $e.presence_penalty, repetition_penalty: $e.repeat_penalty, cap: .cap, penalty_last_n: .penalty_last_n },
      card_row: ($c.card_row | if type == "object" then { row: (.row | typed("string")), source_quote: (.source_quote | typed("string")), enable_thinking: (.enable_thinking | typed("boolean")), default_enable_thinking: (.default_enable_thinking | typed("boolean")), cap: (.cap | typed("number")), penalty_last_n: (.penalty_last_n | typed("number")) } else null end) } end;
# The value the quoted card text gives a sampling setting: `name = number` or `name: number`.
# null when the quote does not state it; "ambiguous" when it states more than one value.
def quoted_value($text; $key): [$text | match("(?<![A-Za-z_])" + $key + "\\s*[=:]\\s*(-?[0-9]+(?:\\.[0-9]+)?)"; "gi") | .captures[0].string | tonumber] | unique
  | if length == 0 then null elif length == 1 then .[0] else "ambiguous" end;
def quoted($q): reduce ("temperature", "top_p", "top_k", "min_p", "presence_penalty", "repetition_penalty") as $k ({}; .[$k] = quoted_value($q; $k));
# One builder per gate. Each returns the complete lisp_eval env.
# The card row is read from the run config the harness embedded in every row (config_file.card_row).
# $card: the vendor card as fetched, nothing added. The row's source_quote must occur in it verbatim,
# and the six sampling values are read from that quote, never from a transcription.
def gate_dme0($rows; $summary; $card): settings($rows) as $s | ($s.card_row // null) as $c
  | (($card | type) == "string" and ($c | type) == "object" and ($c.source_quote | type) == "string" and ($c.source_quote | length) > 0 and $c.source_quote != "mistyped" and ($card | contains($c.source_quote))) as $found
  | {in: {gate: "dme-0", run: (run($rows; $summary) + $s), quote_in_card: $found, quoted: (if $found then quoted($c.source_quote) else {} end)}};
def gate_dme2b($prows; $psummary; $trows; $tsummary):
  {in: {gate: "dme-2b",
        preservation: (run($prows; $psummary) + {overlap_with_target: overlap($prows; $trows), differs_from_target: differ(($prows | facts)[0]; ($trows | facts)[0])}),
        target: run($trows; $tsummary)}}
  | .in.preservation.differs_count = (.in.preservation.differs_from_target | length);
def gate_dme3($b; $bs; $e; $es; $rcs): {in: {gate: "dme-3", base: run($b; $bs), edited: arm($b; $e; $es; $rcs)}};
def gate_dme2c($b; $bs; $t; $ts; $trc; $c; $cs; $crc): {in: {gate: "dme-2c", base: run($b; $bs), targeted: arm($b; $t; $ts; $trc), control: (arm($b; $c; $cs; $crc) + {like_targeted: like($trc; $crc), to_targeted: pair($c; $t)})}};
def ncorrect: rowlist | map(select(good) | select(cls == "c")) | length;
# $arms: [{rows, summary, receipts}] in order of increasing strength
def gate_dme2d($b; $bs; $arms): ($arms | if type == "array" then map(if type == "object" then . else {} end) else [] end) as $a
  | {in: {gate: "dme-2d", base: run($b; $bs), arms: [range(0; $a | length) as $i | $a[$i] as $x
      | ([{rows: $b}] + $a[0:$i] | max_by(.rows | ncorrect)) as $best
      | arm($b; $x.rows; $x.summary; $x.receipts) + {like_first: like($a[0].receipts; $x.receipts), vs_previous: pair((if $i == 0 then $b else $a[$i - 1].rows end); $x.rows), vs_best_earlier: pair($best.rows; $x.rows)}]}};
