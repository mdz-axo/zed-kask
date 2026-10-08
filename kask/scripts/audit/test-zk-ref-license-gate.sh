#!/usr/bin/env bash
# test-zk-ref-license-gate.sh — bounded synthetic controls for the license gate.
#
# Pins the gate's fail-closed contract:
#   1. a clean map over a clean census passes;
#   2. a copyrighted fixture FAILS the gate (the pinned attack);
#   3. a census file with no map entry FAILS the gate (fail-closed on absence);
#   4. a stale map entry (file not in census) FAILS the gate.
# Fixtures are synthetic, built in a temp dir; nothing outside it is touched.

set -euo pipefail

GATE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/zk-ref-license-gate.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

make_census() { # make_census <file> <name:hash> ...
    local f="$1"; shift
    : > "$f"
    for pair in "$@"; do
        printf '%s  %s\n' "${pair%%:*}" "${pair#*:}" >> "$f"
    done
}

pass_count=0
fail_count=0
expect_pass() { # expect_pass <label> <map> <census>
    if bash "$GATE" "$2" "$3" >/dev/null 2>&1; then
        echo "PASS-OK   $1"; pass_count=$((pass_count + 1))
    else
        echo "PASS-FAIL $1 (gate rejected a clean input)" >&2; exit 1
    fi
}
expect_fail() { # expect_fail <label> <map> <census>
    if bash "$GATE" "$2" "$3" >/dev/null 2>&1; then
        echo "FAIL-LEAK $1 (gate passed a violating input)" >&2; exit 1
    else
        echo "FAIL-OK   $1"; fail_count=$((fail_count + 1))
    fi
}

# Control 1: clean map, clean census.
printf 'source\tlicense\n' > "$TMP/map-clean.tsv"
printf 'wikipedia-article.html\tcc_open\n' >> "$TMP/map-clean.tsv"
printf 'arxiv-paper.pdf\tarxiv_nonexclusive\n' >> "$TMP/map-clean.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-clean.tsv"
make_census "$TMP/census-clean.tsv" \
    "aaaa:wikipedia-article.html" \
    "bbbb:arxiv-paper.pdf" \
    "cccc:maia-essay.html"
expect_pass "clean map and census" "$TMP/map-clean.tsv" "$TMP/census-clean.tsv"

# Control 2: a copyrighted fixture must fail (the pinned attack).
printf 'source\tlicense\n' > "$TMP/map-copyrighted.tsv"
printf 'wikipedia-article.html\tcc_open\n' >> "$TMP/map-copyrighted.tsv"
printf 'publisher-book.pdf\tcopyrighted_book\n' >> "$TMP/map-copyrighted.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-copyrighted.tsv"
make_census "$TMP/census-copyrighted.tsv" \
    "aaaa:wikipedia-article.html" \
    "bbbb:publisher-book.pdf" \
    "cccc:maia-essay.html"
expect_fail "copyrighted fixture rejected" "$TMP/map-copyrighted.tsv" "$TMP/census-copyrighted.tsv"

# Control 3: a census file with no map entry must fail (fail-closed on absence).
printf 'source\tlicense\n' > "$TMP/map-missing.tsv"
printf 'wikipedia-article.html\tcc_open\n' >> "$TMP/map-missing.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-missing.tsv"
make_census "$TMP/census-missing.tsv" \
    "aaaa:wikipedia-article.html" \
    "bbbb:arxiv-paper.pdf" \
    "cccc:maia-essay.html"
expect_fail "missing map entry rejected" "$TMP/map-missing.tsv" "$TMP/census-missing.tsv"

# Control 4: a stale map entry (file not in census) must fail.
printf 'source\tlicense\n' > "$TMP/map-stale.tsv"
printf 'wikipedia-article.html\tcc_open\n' >> "$TMP/map-stale.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-stale.tsv"
printf 'removed-file.html\tcc_open\n' >> "$TMP/map-stale.tsv"
make_census "$TMP/census-stale.tsv" \
    "aaaa:wikipedia-article.html" \
    "cccc:maia-essay.html"
expect_fail "stale map entry rejected" "$TMP/map-stale.tsv" "$TMP/census-stale.tsv"

# Control 5: an NC variant is not in the allowlist and must fail.
printf 'source\tlicense\n' > "$TMP/map-nc.tsv"
printf 'wikipedia-article.html\tcc_open\n' >> "$TMP/map-nc.tsv"
printf 'nc-article.html\tcc_by_nc\n' >> "$TMP/map-nc.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-nc.tsv"
make_census "$TMP/census-nc.tsv" \
    "aaaa:wikipedia-article.html" \
    "bbbb:nc-article.html" \
    "cccc:maia-essay.html"
expect_fail "NC variant rejected" "$TMP/map-nc.tsv" "$TMP/census-nc.tsv"

# Control 6 (D10-a amendment, 2026-10-07): the Vatican free-use class is
# allowlisted — an encyclical-quoting charter under it passes.
printf 'source\tlicense\n' > "$TMP/map-vatican.tsv"
printf 'magnifica-humanitas.md\tvatican_free_use\n' >> "$TMP/map-vatican.tsv"
printf 'maia-essay.html\toperator_owned\n' >> "$TMP/map-vatican.tsv"
make_census "$TMP/census-vatican.tsv" \
    "aaaa:magnifica-humanitas.md" \
    "cccc:maia-essay.html"
expect_pass "Vatican free-use class admitted" "$TMP/map-vatican.tsv" "$TMP/census-vatican.tsv"

echo "test-zk-ref-license-gate: $pass_count expected passes, $fail_count expected failures — all controls pinned"
