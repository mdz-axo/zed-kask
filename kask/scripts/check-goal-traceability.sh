#!/usr/bin/env bash
# Advisory only. Goal trailers are syntax, not authenticated goal/approval evidence.
# --message FILE: staged changes (local hook). --range BASE HEAD: every commit
# in BASE..HEAD (CI, including squash commits). All skill-path edits are reported;
# materiality and exception approval remain reviewer decisions.
set -euo pipefail

check_message() {
    local label="$1" message="$2" paths="$3" affected=0 path trailers
    trailers=$(git -c trailer.separators=: interpret-trailers --parse <<< "$message")
    while IFS= read -r -d '' path; do
        case "$path" in .agents/skills/*) affected=1 ;; esac
    done < "$paths"
    if [ "$affected" -eq 0 ]; then
        echo "$label: no skill paths changed"
    elif grep -Eq '^Goal: [[:xdigit:]]{8}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{12}[[:space:]]*$' <<< "$trailers"; then
        echo "$label: Goal trailer present (syntax only; goal existence and approval not verified)"
    elif grep -Eq '^Goal-Exception: [^[:space:]].*' <<< "$trailers"; then
        echo "ADVISORY: $label: exception claim requires operator review; not approval evidence"
    else
        echo "ADVISORY: missing or malformed Goal: <full UUID> for $label skill changes; commit remains allowed"
    fi
}

main() (
    set -euo pipefail
    local_paths=$(mktemp)
    commits=$(mktemp)
    trap 'rm -f "$local_paths" "$commits"' EXIT
    case "${1:-}" in
        --message)
            [ "$#" -eq 2 ]
            git diff --cached --name-only --no-renames -z > "$local_paths"
            message=$(cat "$2")
            check_message staged "$message" "$local_paths"
            ;;
        --range)
            [ "$#" -eq 3 ]
            git rev-parse --verify "${2}^{commit}" >/dev/null
            git rev-parse --verify "${3}^{commit}" >/dev/null
            git rev-list --reverse --end-of-options "$2..$3" > "$commits"
            while IFS= read -r commit; do
                git diff-tree --root -m --no-commit-id --name-only --no-renames -r -z "$commit" > "$local_paths"
                message=$(git show -s --format=%B "$commit")
                check_message "$commit" "$message" "$local_paths"
            done < "$commits"
            ;;
        *) echo 'usage: check-goal-traceability.sh --message FILE | --range BASE HEAD' >&2; exit 2 ;;
    esac
)

# Do not place main in an `if`/`||` test: that disables errexit inside functions.
set +e
main "$@"
status=$?
if [ "$status" -ne 0 ]; then
    echo "ADVISORY: traceability could not be evaluated (exit $status); no approval inferred" >&2
fi
exit 0
