#!/usr/bin/env bash
# Check for `Result<_, String>` anti-pattern in library code.
#
# String error types discard structured error information and prevent
# callers from matching on specific error variants. Use `thiserror` enums
# for library code, `anyhow` for application binaries.
#
# Enabled in CI via `.github/workflows/ci.yml` invariants job.
# Run locally: `scripts/check-string-errors.sh`

set -euo pipefail
cd "$(dirname "$0")/.."

FAIL=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# Collect all lines containing 'Result<' from hKask library code (exclude tests
# and main.rs). Only `hkask-*` crates are scanned — the bridge crate
# `kask_bridge` and panel `kask_panel` are zed-kask-side adapters (D8/D10)
# that legitimately use `String` errors to cross the GPUI/tokio boundary,
# not hKask library code. See zed-host-architecture-plan.md:640.
#
# `hkask-test-harness`'s oracle API (`hkask_test_harness.rs`) is also excluded:
# `oracle_invariant` returns `Result<(), String>` where the `String` is a
# human-readable verdict message fed to `OracleVerdict::Fail(String)` — it is
# never matched on variants, so `String` is the correct type, not an
# anti-pattern. The discarded-error `oracle_inconclusive` was converted to
# `Option<JsonValue>`; only the verdict-message case remains `String`.
grep -rn -- 'Result<' crates/hkask-* mcp-servers/hkask-* \
    --include='*.rs' \
    --exclude-dir=target \
    2>/dev/null \
    | grep -vE '/(tests|examples)/|main\.rs|crates/hkask-test-harness/src/hkask_test_harness\.rs' \
    > "$TMPFILE" || true

while IFS=: read -r file line text; do
    [ -z "$file" ] && continue
    # Match: -> Result<*, String> (where * is any non-> content)
    # Match: -> Result<*, String> (handles nested generics like Result<Vec<u8>, String>)
    # The negative lookahead (?!\s*,) prevents false positives where `String>` is a
    # type parameter inside the Ok type (e.g. Result<HashMap<String, String>, ServiceError>).
    if echo "$text" | grep -qP -- '->\s*Result<.+,\s*String\s*>(?!\s*,)'; then
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
    echo "See: crates/hkask-keystore/src/error.rs (KeychainError example)"
    echo "     crates/hkask-memory/src/episodic.rs (EpisodicError example)"
    exit 1
fi
