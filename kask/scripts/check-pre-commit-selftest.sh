#!/usr/bin/env bash
# Self-test for the pre-commit hook (kask/githooks/pre-commit): the
# scoped tree-shape gate runner. The hook's RED proof is that the commit
# which stages a watched-surface file on a red tree BLOCKS with the
# gate named, and its scoping proof is that the SAME red tree does not
# block a commit staging only an unwatched path:
#   1. A green fixture tree commits cleanly (the gates run and pass).
#   2. A red docs tree (unregistered fence + stale count anchor) does
#      NOT block a commit staging only crates/ code — scoping pins
#      that the hook gates the staged set, never the tree's existence.
#   3. Staging the drifting doc BLOCKS: exit 1 naming docs-count-gate
#      and diagram-registry-parity.
#   4. Reconciled docs (fence removed, count anchors updated) commit
#      cleanly — the reconcile-in-the-same-change flow works.
#   5. A frontmatter-less doc BLOCKS naming docs-frontmatter (count
#      anchors updated, isolating the frontmatter class).
#   6. A drifted mcp registry catalog row BLOCKS naming mcp-readme-counts.
#   7. Containment: the hook names all five gate scripts explicitly —
#      the blocking steps are greppable, never implicit.
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
# Exit codes: 0 = every case behaved as expected, 1 = the hook is wrong.

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOK_SRC="$KASK_ROOT/githooks/pre-commit"

[ -f "$HOOK_SRC" ] || { echo "FAIL: hook not found at $HOOK_SRC"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILURES=0

git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest
mkdir -p "$WORK/.git/hooks"
cp "$HOOK_SRC" "$WORK/.git/hooks/pre-commit"
chmod +x "$WORK/.git/hooks/pre-commit"

# The real gate scripts against a minimal green tree (the same fixture
# shape as the pre-push selftest): 5 frontmatter-complete files under
# kask/docs, the count-gate anchors recording 5, the parity fixture's
# registered diagram, the retired-skill record, and the mcp-readme-counts
# demo server plus its agreeing registry README (symbol-form pin sites).
mkdir -p "$WORK/kask/scripts/build" "$WORK/kask/docs/architecture" \
  "$WORK/kask/docs/reference/mcp-servers" "$WORK/kask/mcp-servers/hkask-mcp-demo/src" \
  "$WORK/crates/agent_skills" "$WORK/crates/unrelated"
for s in check-diagram-registry-parity check-docs-count-gate check-skill-crossrefs check-docs-frontmatter check-mcp-readme-counts; do
  cp "$KASK_ROOT/scripts/$s.sh" "$WORK/kask/scripts/"
done

cat > "$WORK/kask/docs/registered.md" <<'EOF'
---
title: "Registered"
audience: [developers]
last_updated: 2026-10-30
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
verified_date: 2026-10-30
verified_against: selftest
status: VERIFIED
-->
EOF

cat > "$WORK/kask/docs/DIAGRAMS_INDEX.md" <<'EOF'
---
title: "Index"
audience: [developers]
last_updated: 2026-10-30
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Index

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-TEST-001` | registered.md | 2026-10-30 | VERIFIED |
EOF

cat > "$WORK/kask/docs/README.md" <<'EOF'
---
title: "README"
audience: [developers]
last_updated: 2026-10-30
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# README

**Corpus size (measured 2026-10-30):** 5 files total — under the fewer-than-85 cap (working target 75)

- [x] Document count is 5, under the fewer-than-85 cap (working target 75)
EOF

cat > "$WORK/kask/docs/architecture/DOCUMENTATION_STANDARDS.md" <<'EOF'
---
title: "Documentation Standards (fixture)"
audience: [developers]
last_updated: 2026-10-30
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

printf 'hkask-mcp-demo\n' > "$WORK/kask/scripts/build/mcp-servers.txt"
cat > "$WORK/kask/mcp-servers/hkask-mcp-demo/src/hkask_mcp_demo.rs" <<'RS'
//! Demo fixture server for the pre-commit selftest's counts gate.

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

Live pin sites: demo 7 (`hkask_mcp_demo.rs` → `tool_surface_is_exactly_7_registered_tools`).

Arithmetic: `7 = 7`.
MD

commit() { # commit <msg> — capture rc and combined output
  COMMIT_OUT="$(git -C "$WORK" commit -q -m "$1" 2>&1)"; COMMIT_RC=$?
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

expect_exit() { # expect_exit <label> <expected> <actual>
  if [ "$3" -eq "$2" ]; then
    echo "ok: $1"
  else
    echo "FAIL: $1 — expected exit $2, got $3" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

# Case 1: the green fixture tree commits cleanly (all five gates run and pass).
git -C "$WORK" add -A
commit "baseline"
expect_exit "green fixture tree commits" 0 "$COMMIT_RC"

# Case 2: a red docs tree (unregistered fence, stale count anchor) does
# NOT block a commit staging only an unwatched crates/ path.
cat > "$WORK/kask/docs/unregistered.md" <<'EOF'
---
title: "Unregistered"
audience: [developers]
last_updated: 2026-10-30
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

```mermaid
graph TD
    B --> C
```
EOF
printf 'pub fn unrelated() {}\n' > "$WORK/crates/unrelated/lib.rs"
git -C "$WORK" add crates/unrelated/lib.rs
commit "unrelated code only"
expect_exit "unwatched-surface commit is not blocked by the red docs tree" 0 "$COMMIT_RC"

# Case 3: staging the drifting doc BLOCKS, naming both red gates.
git -C "$WORK" add kask/docs/unregistered.md
commit "the drifting doc"
expect_exit "commit staging the drifting doc is blocked" 1 "$COMMIT_RC"
expect_contains "rejection names the docs-count gate" "docs-count-gate" "$COMMIT_OUT"
expect_contains "rejection names the parity gate" "diagram-registry-parity" "$COMMIT_OUT"

# Case 4: reconciled docs (fence removed, count anchors updated) commit cleanly.
sed -i '/```mermaid/,/```/d' "$WORK/kask/docs/unregistered.md"
sed -i 's/5 files total/6 files total/; s/Document count is 5/Document count is 6/' "$WORK/kask/docs/README.md"
git -C "$WORK" add kask/docs/unregistered.md kask/docs/README.md
commit "reconciled docs"
expect_exit "reconciled docs commit cleanly" 0 "$COMMIT_RC"

# Case 5: a frontmatter-less doc BLOCKS naming docs-frontmatter (with the
# count anchors updated, isolating the frontmatter class).
cat > "$WORK/kask/docs/bad.md" <<'EOF'
# No frontmatter here
EOF
sed -i 's/6 files total/7 files total/; s/Document count is 6/Document count is 7/' "$WORK/kask/docs/README.md"
git -C "$WORK" add kask/docs/bad.md kask/docs/README.md
commit "frontmatter-less doc"
expect_exit "frontmatter-less doc commit is blocked" 1 "$COMMIT_RC"
expect_contains "rejection names the frontmatter gate" "docs-frontmatter" "$COMMIT_OUT"
git -C "$WORK" reset -q
rm "$WORK/kask/docs/bad.md"
sed -i 's/7 files total/6 files total/; s/Document count is 7/Document count is 6/' "$WORK/kask/docs/README.md"

# Case 6: a drifted mcp registry catalog row BLOCKS naming mcp-readme-counts.
sed -i 's/| Demo | `kask\/mcp-servers\/hkask-mcp-demo` | Demo | 7 |/| Demo | `kask\/mcp-servers\/hkask-mcp-demo` | Demo | 8 |/' \
  "$WORK/kask/docs/reference/mcp-servers/README.md"
git -C "$WORK" add kask/docs/reference/mcp-servers/README.md
commit "drifted registry row"
expect_exit "drifted registry row commit is blocked" 1 "$COMMIT_RC"
expect_contains "rejection names the readme-counts gate" "mcp-readme-counts" "$COMMIT_OUT"
git -C "$WORK" reset -q
git -C "$WORK" checkout -q -- kask/docs/reference/mcp-servers/README.md

# Case 7: containment — the hook names all five gate scripts explicitly.
hook_src="$(cat "$HOOK_SRC")"
expect_contains "hook names the parity gate script"       "check-diagram-registry-parity.sh" "$hook_src"
expect_contains "hook names the docs-count gate script"   "check-docs-count-gate.sh" "$hook_src"
expect_contains "hook names the crossrefs gate script"    "check-skill-crossrefs.sh" "$hook_src"
expect_contains "hook names the frontmatter gate script"  "check-docs-frontmatter.sh" "$hook_src"
expect_contains "hook names the readme-counts gate script" "check-mcp-readme-counts.sh" "$hook_src"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: pre-commit hook runs the scoped tree-shape gates at the introducing commit (green allowed, unwatched-surface scoping, red blocked with attribution, containment all pinned)"
  exit 0
fi
echo "" >&2
echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected." >&2
exit 1