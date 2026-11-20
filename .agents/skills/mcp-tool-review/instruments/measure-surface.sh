#!/usr/bin/env bash
# Description and schema sizes of a tools/list capture, and the characters the editor forwards per description
# (crates/agent/src/thread.rs build_request_tools: whole if <= 200 bytes, else first ". " within 200 bytes, else 200 + "...").
# Usage: measure-surface.sh <tools.json>
set -uo pipefail
jq -r '
  def fwd: if (utf8bytelength <= 200) then . else (.[0:200] | index(". ")) as $p | if $p then .[0:$p+1] else .[0:200] + "..." end end;
  (map({name, d:(.description|length), f:(.description|fwd|length), cut:((.description|fwd) | (endswith("...") or (endswith(".")|not))), s:(.inputSchema|tostring|length), closed:(.inputSchema.additionalProperties == false)})) as $r
  | ($r[] | [.name, "desc=\(.d)", "forwarded=\(.f)", "cut_mid_sentence=\(.cut)", "schema_bytes=\(.s)", "closed=\(.closed)"] | @tsv),
    "TOTAL\tdesc=\($r|map(.d)|add)\tforwarded=\($r|map(.f)|add)\tcut_mid_sentence=\($r|map(select(.cut))|length)\tschema_bytes=\($r|map(.s)|add)\tclosed=\($r|map(select(.closed))|length)"' "$1"
