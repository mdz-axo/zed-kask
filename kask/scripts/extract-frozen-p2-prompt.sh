#!/usr/bin/env bash
# extract-frozen-p2-prompt.sh — emit the byte-frozen P2 prompt class from the
# p0-bench-llama.sh suite. The frozen instrument is never patched mid-arc (the
# MTP record's D-E discipline), so the long-leg runner reads the prompt from
# the ONE frozen home instead of carrying a second copy that could drift.
# Output: the prompt bytes verbatim (the heredoc's inner lines, no wrapper).
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/p0-bench-llama.sh"

begin=$(grep -nF "P2=\$(cat <<'PROMPT_EOF'" "$SRC" | head -1 | cut -d: -f1)
if [ -z "${begin:-}" ]; then
    echo "FATAL: the P2 heredoc opener not found in $SRC — the frozen suite changed; re-freeze explicitly, never substitute" >&2
    exit 3
fi

end=$(awk -v b="$begin" 'NR>b && /^PROMPT_EOF$/ {print NR; exit}' "$SRC")
if [ -z "${end:-}" ]; then
    echo "FATAL: the P2 heredoc close not found in $SRC" >&2
    exit 3
fi

sed -n "$((begin+1)),$((end-1))p" "$SRC"
