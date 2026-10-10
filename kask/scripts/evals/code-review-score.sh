#!/usr/bin/env bash
# Score code-review eval runs against the fixture manifest.
#
# Usage: code-review-score.sh <runs.jsonl> <manifest.json>
#
# Run record (one JSON object per line in runs.jsonl):
#   {"fixture_id": "...", "run": "...", "executor": "...", "verdict": "...",
#    "findings": [{"id": "...", "file": "...", "line_approx": "...",
#                  "severity": "FYI|Nit|Should-fix|Blocker",
#                  "mapped_class": "<class or null>"}],
#    "behavior_pass": true|false|null}
#
# mapped_class is assigned by the run recorder: a finding maps to an expected
# entry iff it names the same file and its evidence overlaps the planted line
# range. Unmapped findings are false positives unless the fixture tolerates
# them (tolerate_below severity).
#
# Metrics per fixture: recall (distinct mapped classes / expected count),
# precision (mapped / total findings), severity_correct (mapped findings at
# the expected severity / mapped), false_positives (unmapped, untolerated),
# ceiling_violations (findings above the fixture's max_severity).
# Recall assumes distinct expected classes per fixture (true of this
# manifest — see PROTOCOL.md limitations).
set -euo pipefail

runs_file="${1:?usage: code-review-score.sh <runs.jsonl> <manifest.json>}"
manifest_file="${2:?usage: code-review-score.sh <runs.jsonl> <manifest.json>}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

# Join each run with its manifest fixture (array-collect + first: a bare
# generator-select binding runs its body zero times on no match — the known
# jq trap), then score.
jq -c --slurpfile m "$manifest_file" '
  . as $r |
  ([ $m[0].fixtures[] | select(.id == $r.fixture_id) ] | first // null) as $f |
  if $f == null then error("unknown fixture: \($r.fixture_id)") else
  ($r.findings // []) as $fs |
  ($f.expected // []) as $exp |
  ($exp | length) as $en |
  ($fs | map(select(.mapped_class != null)) | length) as $mapped |
  {
    fixture: $r.fixture_id,
    run: $r.run,
    executor: ($r.executor // null),
    behavior_pass: ($r.behavior_pass // null),
    expected: $en,
    findings: ($fs | length),
    mapped: $mapped,
    recall: (if $en > 0
      then (($fs | map(.mapped_class) | map(select(. != null)) | unique | length) / $en)
      else null end),
    precision: (if ($fs | length) > 0
      then ($mapped / ($fs | length))
      else (if $en == 0 then 1 else 0 end) end),
    severity_correct: (if $mapped > 0 then
      ([ $fs[] | select(.mapped_class != null) as $x |
         ($exp[] | select(.class == $x.mapped_class) | .severity) as $s |
         select($x.severity == $s) ] | length) / $mapped
      else null end),
    false_positives: ([ $fs[] | select(.mapped_class == null) as $x |
      ({"FYI":0,"Nit":1,"Should-fix":2,"Blocker":3}[$f.tolerate_below // "FYI"]) as $tb |
      ({"FYI":0,"Nit":1,"Should-fix":2,"Blocker":3}[$x.severity] // 4) as $sr |
      select($tb < $sr) ] | length),
    ceiling_violations: (if $f.max_severity then
      ([ $fs[] |
        ({"FYI":0,"Nit":1,"Should-fix":2,"Blocker":3}[.severity] // 4) as $sr |
        ({"FYI":0,"Nit":1,"Should-fix":2,"Blocker":3}[$f.max_severity]) as $cap |
        select($sr > $cap) ] | length)
      else null end)
  }
  end
' "$runs_file" > "$workdir/per-fixture.jsonl"

echo "=== per-fixture ==="
cat "$workdir/per-fixture.jsonl"

echo "=== aggregate ==="
jq -s '
  {
    fixtures: length,
    mean_recall: (([.[] | select(.recall != null) | .recall] | add // 0) /
                  ([.[] | select(.recall != null)] | length // 1)),
    mean_precision: (([.[] | .precision] | add // 0) / (length // 1)),
    mean_severity_correct: (([.[] | select(.severity_correct != null) | .severity_correct] | add // 0) /
                             ([.[] | select(.severity_correct != null)] | length // 1)),
    total_false_positives: ([.[] | .false_positives] | add // 0),
    total_ceiling_violations: ([.[] | .ceiling_violations // 0] | add // 0),
    behavior_failures: ([.[] | select(.behavior_pass == false)] | length)
  }
' "$workdir/per-fixture.jsonl"
