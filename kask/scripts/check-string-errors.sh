#!/usr/bin/env bash
# Check for `Result<_, String>` anti-pattern in library code.
#
# String error types discard structured error information and prevent
# callers from matching on specific error variants. Use `thiserror` enums
# for library code, `anyhow` for application binaries.
#
# Enabled in CI via `.github/workflows/kask-invariants.yml` check job.
# Run locally: `scripts/check-string-errors.sh`
#
# COVERAGE (known, accepted gaps — an advertised invariant must state what
# it does not check):
# - Line-based: a `Result<..., String>` split across lines so that no
#   single line carries the full shape is not matched. No such signature
#   has been observed; widen with a multiline tool when one is.
# - An Ok type ending in a nested `String>` (e.g. `Result<(String, String),
#   TypedError>`) false-positives. Zero instances today; a loud false
#   positive is recoverable, a silent miss is not.
# - Scope: every hkask-* crate — the kask subtree (crates/hkask-*,
#   mcp-servers/hkask-*) and the zed-path widget/adapter crates
#   (../crates/hkask-*). Only `kask_bridge` is out of scope: the D8 seam
#   crosses the GPUI/tokio boundary where String errors are accepted.

set -euo pipefail
cd "$(dirname "$0")/.."

# Overridable scan roots so the self-test (check-string-errors-selftest.sh)
# can point the gate at a temp tree with a synthetic violation. Defaults to
# the production source roots.
SCAN_DIRS=( ${SCAN_DIRS:-crates/hkask-* mcp-servers/hkask-* ../crates/hkask-*} )

FAIL=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# Collect all lines containing 'Result<' from hkask library code (exclude tests
# and main.rs). Scan roots cover every hkask-* crate — the kask subtree's
# crates/hkask-* + mcp-servers/hkask-* and the zed-path widget crates
# ../crates/hkask-* (widened 2026-09-30: the hkask-* hygiene standard is
# uniform across both trees, matching check-unsafe-forbid.sh's coverage).
# Only the kask_bridge seam (D8) stays out — String errors are accepted at
# the GPUI/tokio boundary. See zed-host-architecture-plan.md:640.
grep -rn -- 'Result<' "${SCAN_DIRS[@]}" \
    --include='*.rs' \
    --exclude-dir=target \
    2>/dev/null \
    | grep -vE '/(tests|examples)/|main\.rs' \
    > "$TMPFILE" || true

while IFS=: read -r file line text; do
    [ -z "$file" ] && continue
    # Skip comment lines — doc text mentioning the rule (e.g. "Replaces
    # `Result<_, String>` with MediaError") is not a violation.
    [[ "$text" =~ ^[[:space:]]*// ]] && continue
    # Skip justified sites: a `string-error-ok` marker suppresses the
    # finding; the justification lives in the surrounding doc comment
    # (same reviewable-suppression pattern as the dead-code baseline).
    [[ "$text" == *string-error-ok* ]] && continue
    # Match: Result<*, String> in ANY position — return, parameter, field,
    # or bound. The error slot is the last type argument before `>`.
    # The negative lookahead (?!\s*,) prevents false positives where `String>` is a
    # type parameter inside the Ok type (e.g. Result<HashMap<String, String>, ServiceError>).
    if echo "$text" | grep -qP -- 'Result<.+,\s*String\s*>(?!\s*,)'; then
        echo "  ${file}:${line}:${text}"
        FAIL=1
    fi
done < "$TMPFILE"

if [ $FAIL -eq 0 ]; then
    echo "OK: No Result<_, String> patterns found in library code."
    exit 0
else
    echo ""
    echo "FAIL: Result<_, String> patterns found. Replace String error types with thiserror enums."
    echo "See: crates/hkask-keystore/src/error.rs (KeystoreError example)"
    echo "     crates/hkask-inference/src/passage_tagging.rs (PassageTaggingError example)"
    exit 1
fi
