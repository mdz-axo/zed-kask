#!/usr/bin/env bash
# Self-test for the pre-push hook (kask/githooks/pre-push): the net-lines
# reporting and the seven invariant gates.
#
# The reporting job's RED proof is that the reported numbers are the TRUE
# numbers for a known range. The gates' RED proof (parity 2026-10-23,
# EUREKA-1; the five docs/skills gates 2026-10-24, EUREKA-2) is that a red
# gate tree BLOCKS the push and a green or restored tree does not:
#   1. A range with known content reports the true net line count and the
#      true DIVERGENCE.md seam delta (+1 added / -1 retired → net 0), and
#      exits 0 (all seven gates green on the fixture).
#   2. A new-branch push (zero remote sha) with no upstream remote
#      degrades VISIBLY (names the skip) instead of guessing a range.
#   3. A ref deletion (zero local sha) measures nothing; the range-based
#      gate's skip is named visibly.
#   4. A malformed range still exits 0 and reports "no diff in range".
#   5. A red parity tree (an unregistered mermaid fence) BLOCKS: exit 1
#      with the parity gate named in the failure list.
#   6. Balance restored → the hook allows again (exit 0).
#   7. A red docs-count tree (the README's recorded count drifted) BLOCKS:
#      exit 1 with the docs-count-gate named in the failure list.
#   8. Containment: the hook names all seven gate scripts explicitly —
#      the blocking steps are greppable, never implicit.
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
mkdir -p "$WORK/githooks"
cp "$HOOK_SRC" "$WORK/githooks/pre-push"

# Base: three seam rows, five code lines.
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

# ── The gate fixtures ────────────────────────────────────────────────────────
# The real gate scripts against a minimal green tree: 5 files under
# kask/docs (all frontmatter-complete), the count-gate's README/STANDARDS
# anchor lines carrying the canonical cap (85) and target (75), the
# retired-skill record, the parity fixture's registered diagram, and
# the mcp-readme-counts gate's demo server plus agreeing README.
mkdir -p "$WORK/kask/scripts" "$WORK/kask/docs/architecture" "$WORK/crates/agent_skills"
for s in check-diagram-registry-parity check-docs-count-gate check-skill-crossrefs check-docs-frontmatter check-retired-record check-docs-citations check-mcp-readme-counts; do
  cp "$KASK_ROOT/scripts/$s.sh" "$WORK/kask/scripts/"
done

cat > "$WORK/kask/docs/registered.md" <<'EOF'
---
title: "Registered"
audience: [developers]
last_updated: 2026-10-24
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

```mermaid
graph TD
    A --> B
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TEST-001
verified_date: 2026-10-24
verified_against: selftest
status: VERIFIED
-->
EOF

cat > "$WORK/kask/docs/DIAGRAMS_INDEX.md" <<'EOF'
---
title: "Index"
audience: [developers]
last_updated: 2026-10-24
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Index

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-TEST-001` | registered.md | 2026-10-24 | VERIFIED |
EOF

cat > "$WORK/kask/docs/README.md" <<'EOF'
---
title: "README"
audience: [developers]
last_updated: 2026-10-24
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# README

**Corpus size (measured 2026-10-24):** 5 files total — under the fewer-than-85 cap (working target 75)

- [x] Document count is 5, under the fewer-than-85 cap (working target 75)
EOF

cat > "$WORK/kask/docs/architecture/DOCUMENTATION_STANDARDS.md" <<'EOF'
---
title: "Documentation Standards (fixture)"
audience: [developers]
last_updated: 2026-10-24
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Documentation Standards (fixture)

The docs tree is capped at **fewer than 85 files** with a working **target of 75 files**.

- Document-count gate: the tree must stay under fewer than 85 files (working target 75).

```bash
# the docs count must stay < 85 files (working target 75)
```
EOF

printf 'const RETIRED_SKILL_NAMES: &[&str] = &[\n    "kata-coaching",\n];\n' > "$WORK/crates/agent_skills/agent_skills.rs"

# The mcp-readme-counts gate's minimal green tree: one demo server whose
# pin is 7, and the README's three count surfaces agreeing with it.
mkdir -p "$WORK/kask/scripts/build" "$WORK/kask/mcp-servers/hkask-mcp-demo/src" \
  "$WORK/kask/docs/reference/mcp-servers"
printf 'hkask-mcp-demo\n' > "$WORK/kask/scripts/build/mcp-servers.txt"
cat > "$WORK/kask/mcp-servers/hkask-mcp-demo/src/hkask_mcp_demo.rs" <<'RS'
//! Demo fixture server for the pre-push selftest's counts gate.

hkask_mcp_server::tool_surface_pin!(
    DemoServer::combined_router(),
    "combined_router",
    7,
    tool_surface_is_exactly_7_registered_tools,
);
RS
cat > "$WORK/kask/docs/reference/mcp-servers/README.md" <<'MD'
---
title: "Demo Registry"
audience: [developers]
last_updated: 2026-10-30
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Demo registry

| Server | Crate | Purpose | Tools |
|--------|-------|---------|------:|
| Demo | `kask/mcp-servers/hkask-mcp-demo` | Demo | 7 |

Live pin sites: demo 7 (`hkask_mcp_demo.rs:7`).

Arithmetic: `7 = 7`.
MD

# The citations gate resolves file:line citations against tracked files —
# stage the fixture tree so the demo citation resolves (the base..tip
# commit range the net-lines reporting reads is unchanged by staging).
git -C "$WORK" add -A

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

# Case 1: known range → true numbers, all gates green, exit 0.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook exits 0 on a normal range with a green gate tree" 0 "$rc"
expect_contains "known range reports true net lines" "net -2 lines" "$out"
expect_contains "known range reports true seam delta" "seams: +1 added / -1 retired" "$out"
expect_contains "known range reports zero seam net" "— net 0" "$out"

# Case 2: new branch, no upstream remote → visible skip.
out="$(run_hook "refs/heads/topic $TIP refs/heads/topic $ZERO")"
expect_contains "new branch without upstream names the skip" \
  "no upstream/main merge base — net-lines skipped" "$out"

# Case 3: ref deletion → no measurement; the range-based gate's skip is visible.
out="$(run_hook "refs/heads/gone $ZERO refs/heads/gone $BASE")"; rc=$?
expect_exit "hook exits 0 on a ref deletion" 0 "$rc"
expect_contains "ref deletion names the range-based gate's skip" \
  "no resolvable pushed range — retired-record unchecked" "$out"
expect_not_contains "ref deletion measures no net-lines" "[pre-push] refs/heads/gone:" "$out"

# Case 4: malformed range → still exits 0, reports no diff.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")"; rc=$?
expect_exit "hook exits 0 on a malformed range" 0 "$rc"
expect_contains "malformed range reports no diff" "no diff in range" "$out"

# Case 5: a red parity tree (unregistered mermaid fence) BLOCKS the push.
printf '```mermaid\ngraph TD\n    B --> C\n```\n' > "$WORK/kask/docs/unregistered.md"
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a red parity tree" 1 "$rc"
expect_contains "red rejection names the parity gate in the failure list" \
  "invariant gate(s) red: diagram-registry-parity" "$out"
rm "$WORK/kask/docs/unregistered.md"

# Case 6: balance restored → the hook allows again.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook allows once parity is restored" 0 "$rc"

# Case 7: a red docs-count tree (the README's recorded count drifted) BLOCKS.
sed -i 's/5 files total/6 files total/' "$WORK/kask/docs/README.md"
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a red docs-count tree" 1 "$rc"
expect_contains "red rejection names the docs-count gate in the failure list" \
  "invariant gate(s) red: docs-count-gate" "$out"
sed -i 's/6 files total/5 files total/' "$WORK/kask/docs/README.md"

# Case 7b: a red mcp-readme-counts tree (the README's server count drifted
# from the pin) BLOCKS.
sed -i 's/| Demo | 7 |/| Demo | 8 |/' "$WORK/kask/docs/reference/mcp-servers/README.md"
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a red mcp-readme-counts tree" 1 "$rc"
expect_contains "red rejection names the mcp-readme-counts gate in the failure list" \
  "invariant gate(s) red: mcp-readme-counts" "$out"
sed -i 's/| Demo | 8 |/| Demo | 7 |/' "$WORK/kask/docs/reference/mcp-servers/README.md"

# Case 8: containment — the hook names all seven gate scripts explicitly.
hook_src="$(cat "$HOOK_SRC")"
expect_contains "hook names the parity gate script"        "check-diagram-registry-parity.sh" "$hook_src"
expect_contains "hook names the docs-count gate script"    "check-docs-count-gate.sh" "$hook_src"
expect_contains "hook names the crossrefs gate script"      "check-skill-crossrefs.sh" "$hook_src"
expect_contains "hook names the frontmatter gate script"    "check-docs-frontmatter.sh" "$hook_src"
expect_contains "hook names the retired-record gate script" "check-retired-record.sh" "$hook_src"
expect_contains "hook names the citations gate script"      "check-docs-citations.sh" "$hook_src"
expect_contains "hook names the readme-counts gate script" "check-mcp-readme-counts.sh" "$hook_src"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: pre-push hook reports true numbers, and the seven invariant gates block red and allow green (report RED + skip + deletion + malformed + parity/count/readme-counts red + containment all pinned)"
  exit 0
fi
echo "" >&2
echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected." >&2
exit 1