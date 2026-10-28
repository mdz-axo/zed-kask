#!/usr/bin/env bash
# Selftest for check-skill-lisp-forms.sh.
#
# The zero-defect production reading is trusted only after the same binary:
#   1. accepts a clean corpus with multiline fenced/inline forms,
#      quote-prefixed forms, and parentheses in strings;
#   2. rejects a surplus closing parenthesis with file:line;
#   3. rejects the inert is_null(member ...) idiom;
#   4. rejects numeric = on a string literal;
#   5. rejects string=/numeric comparison of one union-typed symbol without guards;
#   6. accepts the guarded stringp/numberp union comparison;
#   7. fails visibly on a missing corpus directory;
#   8. fails non-vacuously when a lisp_eval skill exposes no candidate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CHECK="$SCRIPT_DIR/check-skill-lisp-forms.sh"
BIN="$ROOT/target/debug/check-skill-lisp-forms"

cargo build -q -p hkask-lisp --bin check-skill-lisp-forms --manifest-path "$ROOT/Cargo.toml"

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/skills/demo" "$fixture/templates/demo"

run_check() {
    SKILL_CORPUS_DIR="$fixture/skills" \
    TEMPLATE_CORPUS_DIR="$fixture/templates" \
    SKILL_LISP_CHECKER_BIN="$BIN" \
        bash "$CHECK" 2>&1
}

reset_fixture() {
    cat > "$fixture/skills/demo/SKILL.md" <<'EOF'
# Demo

Use `lisp_eval` with `(and ok (= count 1))` and the quoted form `'(literal data)`.

The multiline inline form is `(and
  ok
  (= count 1))` and remains one code span.

```lisp
(begin
  (list "(")
  (list ")"))
```
EOF
    cat > "$fixture/templates/demo/gates.md" <<'EOF'
# Gates

```
form: "(or (and (stringp x) (string= x \"none\")) (and (numberp x) (> x 0.05)))"
env: { "x": "none or a number" }
```
EOF
    printf '{# goal: clean fixture #}\nNo executable form here.\n' \
        > "$fixture/templates/demo/demo.j2"
}

expect_fail() {
    local name=$1 pattern=$2 output=$3
    if run_check > "$output"; then
        echo "SELFTEST FAIL: $name was accepted"; cat "$output"; exit 1
    fi
    grep -q -- "$pattern" "$output" || {
        echo "SELFTEST FAIL: $name output missed '$pattern'"; cat "$output"; exit 1; }
    echo "OK ($name)"
}

# 1. Clean, multiline, string-parenthesis and guarded-union fixture.
reset_fixture
if ! run_check > "$fixture/clean.txt"; then
    echo "SELFTEST FAIL: clean fixture was rejected"; cat "$fixture/clean.txt"; exit 1
fi
grep -q 'parsed 5 Lisp candidates; 0 defects' "$fixture/clean.txt" || {
    echo "SELFTEST FAIL: clean fixture count drifted"; cat "$fixture/clean.txt"; exit 1; }
echo "OK (clean multiline/string/guarded fixture passes)"

# 2. Parse failure — the original KSA-1/SWM-2 defect class.
reset_fixture
printf '\nBroken: `(begin (list 1))))`\n' >> "$fixture/skills/demo/SKILL.md"
expect_fail "surplus parenthesis fails" 'SKILL.md:14: parse error:' "$fixture/parse.txt"

# 3. Inert member/null idiom — member returns boolean, never nil.
reset_fixture
printf '\nBad: `(not (is_null (member x xs)))`\n' >> "$fixture/skills/demo/SKILL.md"
expect_fail "is_null(member) fails" 'is_null(member ...)' "$fixture/member.txt"

# 4. Literal type error — numeric equality on a string.
reset_fixture
printf '\nBad: `(= verdict "pass")`\n' >> "$fixture/skills/demo/SKILL.md"
expect_fail "numeric equality on string fails" 'numeric = receives a string literal' "$fixture/eq.txt"

# 5. Union comparison without short-circuit type guards — the MCDA class.
reset_fixture
printf '\nBad: `(or (string= min_flip "none") (> min_flip 0.05))`\n' >> "$fixture/skills/demo/SKILL.md"
expect_fail "unguarded union comparison fails" 'both string and number without stringp/numberp guards' "$fixture/union.txt"

# 6. The legitimate guarded case must not regress.
reset_fixture
printf '\nGood: `(or (and (stringp min_flip) (string= min_flip "none")) (and (numberp min_flip) (> min_flip 0.05)))`\n' >> "$fixture/skills/demo/SKILL.md"
if ! run_check > "$fixture/guarded.txt"; then
    echo "SELFTEST FAIL: guarded union comparison was rejected"; cat "$fixture/guarded.txt"; exit 1
fi
echo "OK (guarded union comparison passes)"

# 7. Missing corpus directories fail as configuration errors, never clean scans.
reset_fixture
mv "$fixture/skills" "$fixture/skills.missing"
expect_fail "missing corpus directory fails" 'skill corpus directory not found' "$fixture/missing.txt"
mv "$fixture/skills.missing" "$fixture/skills"

# 8. A skill that names lisp_eval but exposes no candidate is a coverage failure.
reset_fixture
printf '# Demo\n\nCall lisp_eval with a form supplied elsewhere.\n' \
    > "$fixture/skills/demo/SKILL.md"
expect_fail "lisp_eval without candidate fails non-vacuously" 'exposes no parseable Lisp candidate' "$fixture/vacuous.txt"

# Pin the production wrapper too: parsing must precede behavioral tests, either
# failure must reach CI, and a stale filter must not turn zero tests into green.
mkdir -p "$fixture/bin"
cat > "$fixture/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
    run)
        echo parse >> "$FAKE_CARGO_LOG"
        if [[ "$FAKE_CARGO_MODE" == parse_failure ]]; then exit 41; fi
        ;;
    test)
        [[ " $* " == *" --bin check-skill-lisp-forms scientific_gate_ "* ]] || exit 43
        echo behavior >> "$FAKE_CARGO_LOG"
        if [[ "$FAKE_CARGO_MODE" == behavior_failure ]]; then exit 42; fi
        if [[ "$FAKE_CARGO_MODE" == zero_tests ]]; then
            echo 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'
        else
            echo 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'
        fi
        ;;
    *) exit 44 ;;
esac
EOF
chmod +x "$fixture/bin/cargo"
for mode in pass parse_failure behavior_failure zero_tests; do
    : > "$fixture/cargo-calls"
    status=0
    PATH="$fixture/bin:$PATH" FAKE_CARGO_LOG="$fixture/cargo-calls" \
        FAKE_CARGO_MODE="$mode" SKILL_LISP_CHECKER_BIN= \
        bash "$CHECK" > "$fixture/pipeline-$mode.txt" 2>&1 || status=$?
    case "$mode" in
        pass) expected=0 ;;
        parse_failure) expected=41 ;;
        behavior_failure) expected=42 ;;
        zero_tests) expected=4 ;;
    esac
    if [[ "$status" -ne "$expected" ]]; then
        echo "SELFTEST FAIL: pipeline $mode exited $status, expected $expected"
        cat "$fixture/pipeline-$mode.txt"
        exit 1
    fi
    if [[ "$mode" == parse_failure ]]; then
        printf 'parse\n' > "$fixture/expected-calls"
    else
        printf 'parse\nbehavior\n' > "$fixture/expected-calls"
    fi
    cmp "$fixture/expected-calls" "$fixture/cargo-calls" || {
        echo "SELFTEST FAIL: pipeline $mode skipped or reordered stages"; exit 1; }
    echo "OK (pipeline $mode)"
done

echo "SELFTEST OK: skill Lisp detector and behavioral gate are alive."
