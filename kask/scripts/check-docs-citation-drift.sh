#!/usr/bin/env bash
# Citation-drift ratchet — the standing detector for the semantic-drift rot
# class (card 68518cb7's census, wired per the operator's 2026-10-24
# approval). Both existing gates are blind to it: check-docs-citations.sh
# verifies a cited line is IN RANGE (a valid line pointing at the wrong
# content passes), and check-docs-citation-anchors.sh checks only the 12
# scoped basenames in full-path form. The 2026-10-24 deep audit found 186
# rotted citations that passed both.
#
# Two phases:
#   GATE (exit 1): every @symbol-annotated citation, in EVERY form (full
#     path in any file, and continuation forms `:LINE@sym`/`,LINE@sym`
#     inheriting their path in textual order) resolves to an enclosing
#     definition matching the symbol — name, `::`-tail, or an impl block
#     (annotating the block itself). This is the only intent-carrying
#     check that is fully mechanical; it extends the anchors gate beyond
#     its scoped set and its full-path-only extraction.
#   WARN (exit 0): plain citations whose cited line falls in a region
#     shifted or edited since the audit hash (kask/scripts/
#     citation-drift-audit-hash.txt) — the "re-anchor at consultation"
#     class, listed but not failed, matching the existing gates' warning
#     tier (line-drift, ambiguous-name). Also warns on ambiguous bare
#     basenames and orphaned continuation forms.
#
# --refresh writes the current HEAD hash into the baseline after a deep
# audit (or any re-anchoring pass) — the ratchet: each run checks drift
# since the last pinned audit point, never since the beginning of time.
#
# Self-test: kask/scripts/check-docs-citation-drift-selftest.sh
# Run locally: bash kask/scripts/check-docs-citation-drift.sh
# Exit codes: 0 = no gate failure (warnings allowed), 1 = @sym mismatches,
#             2 = usage/tooling error.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: not inside a git repository (run from the repo or a fixture repo)." >&2
  exit 2
}
cd "$REPO_ROOT" || exit 2
DOCS_ROOT="${DOCS_ROOT:-kask/docs}"
BASELINE="$SCRIPT_DIR/citation-drift-audit-hash.txt"
SG="$SCRIPT_DIR/ast-grep.sh"
DEFS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-definitions.yml"
IMPLS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-impls.yml"

command -v jq >/dev/null 2>&1 || { echo "FAIL: jq not found." >&2; exit 2; }
[ -x "$SG" ] || { echo "FAIL: ast-grep wrapper missing." >&2; exit 2; }
[ -d "$DOCS_ROOT" ] || { echo "FAIL: docs root $DOCS_ROOT not found." >&2; exit 2; }

REFRESH=0
[ "${1:-}" = "--refresh" ] && REFRESH=1

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git ls-files '*.rs' | sort > "$TMP/rsfiles.txt"

# ── Phase A+B: census with order-preserving continuation inheritance ──────
resolve() {
  local p=$1 d b hits n bhits bn
  [ -f "$p" ] && { printf '%s\n' "$p"; return; }
  d=$(dirname "$doc"); [ -f "$d/$p" ] && { printf '%s\n' "$d/$p"; return; }
  hits=$(grep -x ".*/$p" "$TMP/rsfiles.txt" || true)
  n=$(printf '%s\n' "$hits" | grep -c . || true)
  [ "$n" -eq 1 ] && { printf '%s\n' "$hits"; return; }
  b=${p##*/}
  bhits=$(grep -x ".*/$b" "$TMP/rsfiles.txt" || true)
  bn=$(printf '%s\n' "$bhits" | grep -c . || true)
  [ "$bn" -eq 1 ] && { printf '%s\n' "$bhits"; return; }
  return 1
}

: > "$TMP/citations.tsv"   # doc, docline, file, startline, sym, form
: > "$TMP/unresolved.log"
mapfile -t DOCS < <(find "$DOCS_ROOT" -name '*.md' | sort)
for doc in "${DOCS[@]}"; do
  lineno=0; last_path=""
  while IFS= read -r line; do
    lineno=$((lineno + 1))
    toks=$(grep -oE '[A-Za-z0-9_./-]+\.rs:[0-9]+(-[0-9]+)?(@[A-Za-z_][A-Za-z0-9_:]*)?|[:,][0-9]+(-[0-9]+)?@[A-Za-z_][A-Za-z0-9_:]*' <<<"$line" || true)
    if [ -n "$toks" ]; then
      while IFS= read -r t; do
        case "$t" in
          *\.rs:*)
            p=${t%%:*}; rest=${t#*:}; linespec=${rest%%@*}; sym=""
            case "$rest" in *@*) sym=${rest#*@};; esac
            r=$(resolve "$p") || { echo "$doc:$lineno UNRESOLVED $t" >> "$TMP/unresolved.log"; continue; }
            printf '%s\t%s\t%s\t%s\t%s\tfull\n' "$doc" "$lineno" "$r" "${linespec%%-*}" "$sym" >> "$TMP/citations.tsv"
            last_path="$r"
            ;;
          :*|,*)
            [ -n "$last_path" ] || { echo "$doc:$lineno CONT-NO-PATH $t" >> "$TMP/unresolved.log"; continue; }
            rest=${t#[:,]}; linespec=${rest%%@*}; sym=${rest#*@}
            printf '%s\t%s\t%s\t%s\t%s\tcont\n' "$doc" "$lineno" "$last_path" "${linespec%%-*}" "$sym" >> "$TMP/citations.tsv"
            ;;
        esac
      done <<<"$toks"
    else
      last_path=""
    fi
  done < "$doc"
done

# ── Phase C: bulk definition map (one ast-grep dump per rule) ─────────────
cut -f3 "$TMP/citations.tsv" | sort -u > "$TMP/files.txt"
: > "$TMP/spans.tsv"
if [ -s "$TMP/files.txt" ]; then
  "$SG" scan --rule "$DEFS_RULE" --json=stream $(cat "$TMP/files.txt") 2>/dev/null \
    | jq -r 'select(.ruleId == "rust-definitions")
        | [.file, .metaVariables.single.NAME.text, (.range.start.line + 1), (.range.end.line + 1)]
        | @tsv' >> "$TMP/spans.tsv" || true
  "$SG" scan --rule "$IMPLS_RULE" --json=stream $(cat "$TMP/files.txt") 2>/dev/null \
    | jq -r 'select(.ruleId == "rust-impls")
        | [.file, ("impl " + .metaVariables.single.TYPE.text), (.range.start.line + 1), (.range.end.line + 1)]
        | @tsv' >> "$TMP/spans.tsv" || true
fi

# Enclosing definition per unique (file,line), qualified "Type::name" like
# the standalone resolver (the innermost def wins; a def inside an impl is
# qualified by the impl's type).
cut -f3,4 "$TMP/citations.tsv" | sort -u \
| while IFS=$'\t' read -r f l; do
    e=$(awk -F'\t' -v f="$f" -v l="$l" '
      $1 == f && $3 <= l && l <= $4 && $3 > best { best = $3; n = $2 }
      END { if (n != "") print n }' "$TMP/spans.tsv")
    if [ -z "$e" ]; then
      printf '%s\t%s\t\n' "$f" "$l"
      continue
    fi
    case "$e" in "impl "*) printf '%s\t%s\t%s\n' "$f" "$l" "$e"; continue;; esac
    implq=$(awk -F'\t' -v f="$f" -v b="$e" '
      $1 == f && $2 ~ /^impl / && $3 <= b && b <= $4 { print substr($2, 6); exit }' "$TMP/spans.tsv")
    [ -n "$implq" ] && e="$implq::$e"
    printf '%s\t%s\t%s\n' "$f" "$l" "$e"
  done > "$TMP/enclosing.tsv"

# ── Phase D: the @symbol gate (every form, every file) ────────────────────
# Tab is IFS whitespace: empty sym fields collapse under plain `read`,
# shifting the form column into sym — read through the unit separator.
mismatch=0
while IFS=$'\037' read -r doc docline file line sym form; do
  [ -n "$sym" ] || continue
  e=$(awk -F'\t' -v f="$file" -v l="$line" '$1==f && $2==l {print $3; exit}' "$TMP/enclosing.tsv")
  if [ -z "$e" ]; then
    mismatch=$((mismatch + 1))
    echo "::error::citation-drift: $doc cites $file:$line@$sym — no enclosing definition at that line (drifted past its symbol; re-anchor with kask/scripts/resolve-citation-anchor.sh)"
    continue
  fi
  te=${e##*::}; ts=${sym##*::}
  ei=$e; [[ "$ei" == "impl "* ]] && ei=${ei#impl }
  if [ "$e" != "$sym" ] && [ "$te" != "$sym" ] && [ "$e" != "$ts" ] && [ "$te" != "$ts" ] && [ "$ei" != "$sym" ]; then
    mismatch=$((mismatch + 1))
    echo "::error::citation-drift: $doc cites $file:$line@$sym — the enclosing definition there is '$e' now (re-anchor with kask/scripts/resolve-citation-anchor.sh)"
  fi
done < <(tr '\t' '\037' < "$TMP/citations.tsv")

# ── Phase E: drift-window warnings against the audit hash ─────────────────
drift_warn=0
if [ -f "$BASELINE" ]; then
  BASE=$(head -1 "$BASELINE")
  : > "$TMP/hunks.tsv"
  while IFS= read -r f; do
    git diff "$BASE..HEAD" --unified=0 -- "$f" 2>/dev/null | awk -v f="$f" '
      /^@@/ {
        oc=1; nc=1; os=""; ns=""
        for (i = 2; i <= NF; i++) {
          if (substr($i, 1, 1) == "-") { os = substr($i, 2) }
          if (substr($i, 1, 1) == "+") { ns = substr($i, 2) }
        }
        if (os != "") { split(os, a, ","); oc = (a[2] == "" ? 1 : a[2]) }
        if (ns != "") { split(ns, b, ","); nc = (b[2] == "" ? 1 : b[2]) }
        print f "\t" b[1] "\t" nc "\t" (nc - oc)
      }' >> "$TMP/hunks.tsv"
  done < "$TMP/files.txt"
  while IFS=$'\037' read -r doc docline file line sym form; do
    [ -z "$sym" ] || continue
    shift_n=0; inhunk=0
    while IFS=$'\t' read -r hf c d delta; do
      [ "$hf" = "$file" ] || continue
      if [ "$d" -eq 0 ]; then
        [ "$line" -ge "$c" ] && shift_n=$((shift_n + delta))
      else
        end=$((c + d - 1))
        [ "$line" -gt "$end" ] && shift_n=$((shift_n + delta))
        [ "$line" -ge "$c" ] && [ "$line" -le "$end" ] && inhunk=1
      fi
    done < "$TMP/hunks.tsv"
    if [ "$shift_n" != "0" ] || [ "$inhunk" = "1" ]; then
      drift_warn=$((drift_warn + 1))
      echo "warn: citation-drift: $doc cites $file:$line — in a region shifted/edited since the audit hash (re-anchor at consultation)"
    fi
  done < <(tr '\t' '\037' < "$TMP/citations.tsv")
else
  echo "warn: citation-drift: no audit hash at $BASELINE — run '$0 --refresh' after a deep audit to pin the audit point; the drift-warning phase is skipped until then."
fi

ambig=$(grep -c 'UNRESOLVED' "$TMP/unresolved.log" 2>/dev/null || true)
orphan=$(grep -c 'CONT-NO-PATH' "$TMP/unresolved.log" 2>/dev/null || true)

if [ "$REFRESH" -eq 1 ]; then
  git rev-parse HEAD > "$BASELINE"
  echo "OK: audit hash refreshed to $(git rev-parse HEAD). Commit it with the audit that earned it."
  exit 0
fi

echo "----"
echo "citation drift: $mismatch @sym mismatches (failures), $drift_warn shifted-region warnings, $ambig ambiguous-name warnings, $orphan orphaned continuation forms, over ${#DOCS[@]} docs"
[ "$mismatch" -eq 0 ]