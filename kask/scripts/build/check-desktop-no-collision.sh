#!/bin/bash
# check-desktop-no-collision.sh
#
# REGRESSION TEST: zed-kask .desktop templates must NOT declare any MIME type
# or keyword that overlaps with upstream Zed.
#
# History: commit 853542beab (Jul 26 2026) changed the URL scheme in the
# upstream .desktop template from x-scheme-handler/zed to x-scheme-handler/
# zed-kask but left text/plain, application/x-zerosize, and Keywords=zed in
# place. When the installer rendered that template and called
# update-desktop-database, zed-kask appeared as a competitor for opening text
# files and "zed" launcher searches. The more recently installed app
# (zed-kask) hijacked the user's real Zed install — clicking to open Zed
# opened zed-kask instead.
#
# This test asserts that neither .desktop template contains the forbidden
# declarations. It is a hard gate: if any forbidden pattern appears, the test
# fails with exit code 1 and names the file and the offending pattern.
#
# Forbidden patterns (checked against MimeType= and Keywords= lines only,
# not comments):
#   MimeType=...text/plain...          — upstream Zed's MIME type
#   MimeType=...application/x-zerosize — upstream Zed's MIME type
#   MimeType=...x-scheme-handler/zed;  — upstream Zed's URL scheme (exact,
#                                        not a prefix of zed-kask)
#   Keywords=zed;                      — upstream Zed's keyword
#
# Run: bash kask/scripts/build/check-desktop-no-collision.sh
# CI:  wired into kask-ci.yml as a required check.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"

# Both .desktop templates that get rendered into installed .desktop files.
# If a new bundling path is added, its template MUST be added to this list.
templates=(
    "$script_dir/zed-kask.desktop.in"
    "$repo_root/crates/zed/resources/zed.desktop.in"
)

errors=0

for template in "${templates[@]}"; do
    if [ ! -f "$template" ]; then
        echo "FAIL: template not found: $template" >&2
        errors=$((errors + 1))
        continue
    fi

    # Check MimeType line for forbidden MIME types.
    # We grep the MimeType= line specifically, so comments mentioning these
    # strings for explanatory purposes don't trigger false positives.
    mime_line=$(grep -i '^MimeType=' "$template" || true)

    if echo "$mime_line" | grep -qi 'text/plain'; then
        echo "FAIL: $template — MimeType declares text/plain" >&2
        echo "  text/plain belongs to upstream Zed. Declaring it makes zed-kask" >&2
        echo "  compete for opening text files, hijacking the user's real Zed." >&2
        errors=$((errors + 1))
    fi

    if echo "$mime_line" | grep -qi 'application/x-zerosize'; then
        echo "FAIL: $template — MimeType declares application/x-zerosize" >&2
        echo "  application/x-zerosize belongs to upstream Zed — same collision." >&2
        errors=$((errors + 1))
    fi

    # Check for upstream's URL scheme (exact: x-scheme-handler/zed followed
    # by ; or end-of-line, NOT x-scheme-handler/zed-kask).
    if echo "$mime_line" | grep -qE 'x-scheme-handler/zed;'; then
        echo "FAIL: $template — MimeType declares x-scheme-handler/zed;" >&2
        echo "  This is upstream Zed's URL scheme. zed-kask must use" >&2
        echo "  x-scheme-handler/zed-kask instead." >&2
        errors=$((errors + 1))
    fi

    # Check Keywords line.
    kw_line=$(grep -i '^Keywords=' "$template" || true)
    if echo "$kw_line" | grep -qiE '^Keywords=zed;'; then
        echo "FAIL: $template — Keywords=zed; (must be Keywords=zed-kask;)" >&2
        echo "  Keyword 'zed' belongs to upstream Zed. App launcher searches for" >&2
        echo "  'zed' must not surface zed-kask alongside or instead of real Zed." >&2
        errors=$((errors + 1))
    fi
done

if [ "$errors" -gt 0 ]; then
    echo "" >&2
    echo "REGRESSION: $errors collision(s) detected." >&2
    echo "zed-kask .desktop files must NOT declare any MIME type or keyword" >&2
    echo "that overlaps with upstream Zed. This collision causes desktop" >&2
    echo "environments to hijack the user's real Zed install." >&2
    echo "See kask/scripts/build/zed-kask.desktop.in for the full rationale." >&2
    exit 1
fi

echo "PASS: No upstream-Zed collision in zed-kask .desktop templates."
exit 0