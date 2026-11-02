#!/usr/bin/env bash
# Self-test for the pre-push hook (kask/githooks/pre-push): the
# net-lines reporting and the four blocking gates (rustfmt + the three
# citation gates), run against the PUSHED COMMIT via a git archive export.
#
# The reporting job's RED proof is that the reported numbers are the
# TRUE numbers for a known range. The gate's RED proof is that a push
# of a commit with rustfmt drift BLOCKS, naming the gate, and a green
# push does not — whether the working tree is busy or clean, because
# the gate reads the pushed sha's export, never the tree:
#   1. A green range reports the true net line count and the true
#      DIVERGENCE.md seam delta, and exits 0.
#   2. A push with uncommitted working-tree changes still ALLOWS (the
#      2026-10-31 unwiring of the busy-tree block).
#   3. A pushed commit with fmt drift BLOCKS, naming rustfmt.
#   4. Restored formatting ALLOWS again.
#   5. Pushing a RED sha with a clean tree BLOCKS — the export gates
#      the pushed sha, so a green tree can never greenwash a red push.
#   6. Pushing a non-HEAD green sha ALLOWS.
#   7. A ref deletion measures nothing; exits 0.
#   8. Containment: the hook names the rustfmt gate script and the
#      three wired citation gates, and names NONE of the still-unwired
#      bookkeeping gates (the 2026-11-02 T3 re-wiring).
#   9. A pushed commit with a green citation set ALLOWS — all four
#      gates pass against the pushed export.
#  10. A pushed commit with a deliberately rotted @sym citation
#      BLOCKS, naming citation-drift — the known-positive discipline:
#      a gate that cannot fail is not a gate.
#  11. A re-anchored citation ALLOWS again.
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
# Exit codes: 0 = every case behaved as expected, 1 = the hook is wrong.

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOK_SRC="$KASK_ROOT/githooks/pre-push"

[ -f "$HOOK_SRC" ] || { echo "FAIL: hook not found at $HOOK_SRC"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILURES=0

git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest
mkdir -p "$WORK/githooks" "$WORK/kask/scripts"
cp "$HOOK_SRC" "$WORK/githooks/pre-push"
cp "$KASK_ROOT/scripts/check-rustfmt.sh" "$WORK/kask/scripts/"
# The citation gates + their tree-sitter instrument: export-gated pushes
# must carry the gated object's own gate scripts (the hook warns and
# skips a missing gate in the export; the red-proof cases below need
# them live).
cp "$KASK_ROOT/scripts/check-docs-citations.sh" "$WORK/kask/scripts/"
cp "$KASK_ROOT/scripts/check-docs-citation-drift.sh" "$WORK/kask/scripts/"
cp "$KASK_ROOT/scripts/check-code-docs-citations.sh" "$WORK/kask/scripts/"
cp "$KASK_ROOT/scripts/ast-grep.sh" "$WORK/kask/scripts/"
mkdir -p "$WORK/kask/ast-grep/rules"
cp "$KASK_ROOT/ast-grep/rules/rust-definitions.yml" "$WORK/kask/ast-grep/rules/"
cp "$KASK_ROOT/ast-grep/rules/rust-impls.yml" "$WORK/kask/ast-grep/rules/"
mkdir -p "$WORK/kask/docs/fx"
# Stub every kask/docs/**.md path the copied gate banners cite — the
# code-docs gate sweeps non-docs surfaces for docs paths, so the banners'
# citations must exist in the fixture as they do in the real tree.
for p in $(cat "$KASK_ROOT/scripts/check-docs-citations.sh" \
              "$KASK_ROOT/scripts/check-docs-citation-drift.sh" \
              "$KASK_ROOT/scripts/check-code-docs-citations.sh" \
              "$KASK_ROOT/scripts/ast-grep.sh" \
    | grep -hoE 'kask/docs/[A-Za-z0-9_./-]+\.md' | sort -u); do
  mkdir -p "$WORK/$(dirname "$p")"
  : > "$WORK/$p"
done

# The rustfmt gate's minimal green tree: a single-crate workspace whose
# one lib file is formatted.
cat > "$WORK/Cargo.toml" <<'EOF'
[package]
name = "fixture"
version = "0.0.0"
edition = "2021"

[lib]
path = "fixture.rs"
EOF
printf 'pub fn fixture() {}\n' > "$WORK/fixture.rs"

# Base: three seam rows, five code lines — committed so pushes attest
# exactly the fixture content.
printf '| D1 | base seam |\n| D2 | base seam |\n| D3 | base seam |\n' > "$WORK/DIVERGENCE.md"
printf 'a\nb\nc\nd\ne\n' > "$WORK/code.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm base
BASE="$(git -C "$WORK" rev-parse HEAD)"

# Tip: one seam row added (D4), one retired (D2), two code lines removed.
# True diff: 1 insertion, 3 deletions → net -2; seams +1/-1 → net 0.
printf '| D1 | base seam |\n| D3 | base seam |\n| D4 | new seam |\n' > "$WORK/DIVERGENCE.md"
printf 'a\nb\nc\n' > "$WORK/code.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm tip
TIP="$(git -C "$WORK" rev-parse HEAD)"

# fcommit <msg> — land every working-tree change as a fixture commit, so
# each red-gate case's drift is attributable to the pushed sha.
fcommit() {
  git -C "$WORK" add -A
  git -C "$WORK" commit -qm "$1"
}

ZERO=0000000000000000000000000000000000000000

run_hook() { # run_hook <stdin-line...> — mirrors git's invocation (cwd = repo root)
  (cd "$WORK" && printf '%s\n' "$@" | bash githooks/pre-push 2>&1)
}

expect_contains() { # expect_contains <label> <needle> <haystack>
  if grep -qF -- "$2" <<<"$3"; then
    echo "ok: $1"
  else
    echo "FAIL: $1 — output did not contain: $2" >&2
    echo "--- got ---" >&2
    printf '%s\n' "$3" >&2
    echo "------------" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

expect_not_contains() { # expect_not_contains <label> <needle> <haystack>
  if grep -qF -- "$2" <<<"$3"; then
    echo "FAIL: $1 — output contained: $2" >&2
    echo "--- got ---" >&2
    printf '%s\n' "$3" >&2
    echo "------------" >&2
    FAILURES=$((FAILURES + 1))
  else
    echo "ok: $1"
  fi
}

expect_exit() { # expect_exit <label> <expected> <actual>
  if [ "$3" -eq "$2" ]; then
    echo "ok: $1"
  else
    echo "FAIL: $1 — expected exit $2, got $3" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

# Case 1: known range → true numbers, green gate, exit 0.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook exits 0 on a normal green range" 0 "$rc"
expect_contains "known range reports true net lines" "net -2 lines" "$out"
expect_contains "known range reports true seam delta" "seams: +1 added / -1 retired" "$out"
expect_contains "known range reports zero seam net" "— net 0" "$out"

# Case 2: uncommitted working-tree changes do NOT block a green push —
# the gate reads the pushed commit's export, never the tree.
printf 'uncommitted\n' >> "$WORK/code.txt"
out="$(run_hook "refs/heads/main $(git -C "$WORK" rev-parse HEAD) refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook allows a green push with uncommitted tree changes" 0 "$rc"
expect_not_contains "no refusal blaming uncommitted changes" "uncommitted changes" "$out"
git -C "$WORK" checkout -q -- code.txt

# Case 3: a pushed commit with fmt drift BLOCKS, naming rustfmt.
printf 'pub fn fixture(){}\n' > "$WORK/fixture.rs"
fcommit "red rustfmt"
RED_FMT_SHA="$(git -C "$WORK" rev-parse HEAD)"
out="$(run_hook "refs/heads/main $(git -C "$WORK" rev-parse HEAD) refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a push with fmt drift in the pushed commit" 1 "$rc"
expect_contains "red rejection names the rustfmt gate" \
  "invariant gate(s) red: rustfmt" "$out"

# Case 4: restored formatting ALLOWS again.
printf 'pub fn fixture() {}\n' > "$WORK/fixture.rs"
fcommit "restore rustfmt"

# Case 5: pushing a RED sha with a clean tree BLOCKS — the export gates
# the pushed sha's content, so a green tree can never greenwash a red
# push (the false-green class, closed 2026-10-31).
out="$(run_hook "refs/heads/main $RED_FMT_SHA refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a push of a red sha despite the clean green tree" 1 "$rc"
expect_contains "red-sha rejection names the rustfmt gate" \
  "invariant gate(s) red: rustfmt" "$out"

# Case 6: pushing a non-HEAD green sha ALLOWS — the gate attests that
# sha's own content via the export.
out="$(run_hook "refs/heads/old $BASE refs/heads/old $ZERO")"; rc=$?
expect_exit "hook allows a push of a non-HEAD green sha" 0 "$rc"

# Case 7: a ref deletion measures nothing; exits 0.
out="$(run_hook "refs/heads/gone $ZERO refs/heads/gone $BASE")"; rc=$?
expect_exit "hook exits 0 on a ref deletion" 0 "$rc"
expect_not_contains "ref deletion measures no net-lines" "[pre-push] refs/heads/gone:" "$out"

# Case 8: containment — the hook names the rustfmt gate script, the
# export mechanism, and the three wired citation gates; it names none
# of the still-unwired bookkeeping gates.
hook_src="$(cat "$HOOK_SRC")"
expect_contains "hook names the rustfmt gate script" "check-rustfmt.sh" "$hook_src"
expect_contains "hook gates via the export mechanism" "git archive" "$hook_src"
expect_contains "hook names the docs-citations gate" "check-docs-citations.sh" "$hook_src"
expect_contains "hook names the citation-drift gate" "check-docs-citation-drift.sh" "$hook_src"
expect_contains "hook names the code-docs-citations gate" "check-code-docs-citations.sh" "$hook_src"
expect_not_contains "hook carries no docs-count gate" "check-docs-count-gate.sh" "$hook_src"
expect_not_contains "hook carries no frontmatter gate" "check-docs-frontmatter.sh" "$hook_src"
expect_not_contains "hook carries no diagram gate" "check-diagram-registry-parity.sh" "$hook_src"
expect_not_contains "hook carries no readme-counts gate" "check-mcp-readme-counts.sh" "$hook_src"
expect_not_contains "hook carries no skill-crossrefs gate" "check-skill-crossrefs.sh" "$hook_src"
expect_not_contains "hook carries no retired-record gate" "check-retired-record.sh" "$hook_src"

# Case 9: a green citation in a pushed commit allows — all four gates
# pass against the pushed export.
printf 'See fixture.rs:1@fixture for the entry point.\n' \
  > "$WORK/kask/docs/fx/doc.md"
fcommit "green citations"
out="$(run_hook "refs/heads/main $(git -C "$WORK" rev-parse HEAD) refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook allows a push with green citations" 0 "$rc"

# Case 10: a deliberately rotted @sym citation in a pushed commit
# BLOCKS, naming citation-drift — the known-positive discipline
# (.rules): a gate that cannot fail is not a gate.
printf 'See fixture.rs:1@alpha for the entry point.\n' \
  > "$WORK/kask/docs/fx/doc.md"
fcommit "rotted citation"
out="$(run_hook "refs/heads/main $(git -C "$WORK" rev-parse HEAD) refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a push with a rotted citation" 1 "$rc"
expect_contains "rotted-citation rejection names the drift gate" \
  "invariant gate(s) red: citation-drift" "$out"

# Case 11: a re-anchored citation allows again.
printf 'See fixture.rs:1@fixture for the entry point.\n' \
  > "$WORK/kask/docs/fx/doc.md"
fcommit "restore citation"
out="$(run_hook "refs/heads/main $(git -C "$WORK" rev-parse HEAD) refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook allows again after re-anchoring the citation" 0 "$rc"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: pre-push hook reports true numbers, gates rustfmt and the three citation gates over the pushed commit via export (busy tree allows; red sha blocked despite clean tree; non-HEAD green allowed; rotted citation blocked, re-anchored allowed), and carries none of the unwired bookkeeping gates (report + fmt red/green + citation red/green + export gating + containment all pinned)"
  exit 0
fi
echo "" >&2
echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected." >&2
exit 1