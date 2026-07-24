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

# Collect all lines containing 'Result<' from library code (exclude tests and main.rs)
grep -rn -- 'Result<' crates/ mcp-servers/ \
    --include='*.rs' \
    --exclude-dir=target \
    2>/dev/null \
    | grep -vE '/(tests|examples)/|main\.rs' \
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
    echo "See: crates/hkask-acp/src/cloud.rs (CloudError example)"
    echo "     crates/hkask-agents/src/curator/semantic_sync.rs (SyncError example)"
    exit 1
fi
