#!/usr/bin/env bash
# Real Cargo fixture: bad implementation -> corrective receipt -> repair -> fresh receipt.
set -euo pipefail
[[ $# -eq 1 ]] || { echo 'usage: evaluate-test-evidence-selftest.sh CHECKER' >&2; exit 2; }
checker=$(realpath -e "$1")
runner=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/evaluate-test-evidence.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/src"
cat > "$tmp/Cargo.toml" <<'EOF'
[package]
name = "expectation-fixture"
version = "0.1.0"
edition = "2024"
[workspace]
EOF
printf '%s\n' 'A refused request must leave the user balance unchanged.' > "$tmp/contract.txt"
cat > "$tmp/src/lib.rs" <<'EOF'
pub fn balance_after_refusal(balance: u32) -> u32 { balance - 1 }
#[cfg(test)]
mod tests;
EOF
cat > "$tmp/src/tests.rs" <<'EOF'
#[test]
fn refusal_preserves_user_balance() {
    assert_eq!(super::balance_after_refusal(20), 20);
}
EOF
cargo generate-lockfile --offline --manifest-path "$tmp/Cargo.toml"
for budget in 0 invalid 3601; do
    status=0
    bash "$runner" "$checker" "$tmp/Cargo.toml" expectation-fixture "$tmp/contract.txt" "$tmp/src/tests.rs" "$tmp/invalid-budget" "$budget" "$tmp/src/lib.rs" || status=$?
    [[ $status -eq 2 && ! -e "$tmp/invalid-budget" ]]
done
evaluate() {
    bash "$runner" "$checker" "$tmp/Cargo.toml" expectation-fixture "$tmp/contract.txt" "$tmp/src/tests.rs" "$tmp/$1" 30 "$tmp/src/lib.rs"
}
status=0
evaluate before || status=$?
[[ $status -eq 1 ]]
jq -e '.disposition == "corrective_work_required" and .failed == 1' "$tmp/before/receipt.json" >/dev/null
# Same test and expectation, change only the faulty decision.
sed -i 's/{ balance - 1 }/{ balance }/' "$tmp/src/lib.rs"
evaluate after
jq -e '.disposition == "evaluation_passed" and .passed == 1' "$tmp/after/receipt.json" >/dev/null
jq -e --slurpfile before "$tmp/before/expected.json" \
    '.identity.artifact_sha256 != $before[0].identity.artifact_sha256 and .identity.contract_sha256 == $before[0].identity.contract_sha256 and .identity.evaluator_sha256 == $before[0].identity.evaluator_sha256' \
    "$tmp/after/expected.json" >/dev/null

reject() {
    local expected=$1 report=$2 log=$3 status=0
    "$checker" "$expected" "$report" "$log" > "$tmp/rejection.json" || status=$?
    [[ $status -eq 2 ]]
    jq -e '.disposition == "evidence_rejected"' "$tmp/rejection.json" >/dev/null
}
reject "$tmp/after/expected.json" "$tmp/before/report.json" "$tmp/before/tests.log"
reject "$tmp/after/expected.json" "$tmp/missing.json" "$tmp/after/tests.log"
printf '{' > "$tmp/broken.json"
reject "$tmp/after/expected.json" "$tmp/broken.json" "$tmp/after/tests.log"
for change in '.inputs_unchanged = false' '.exit_code = 124' '.finished_at = "2000-01-01T00:00:00Z"' '.identity.evaluator_sha256 = "wrong"'; do
    jq "$change" "$tmp/after/report.json" > "$tmp/broken.json"
    reject "$tmp/after/expected.json" "$tmp/broken.json" "$tmp/after/tests.log"
done
printf 'fabricated success\n' > "$tmp/altered.log"
reject "$tmp/after/expected.json" "$tmp/after/report.json" "$tmp/altered.log"
# A zero-test run is not a passing expectation evaluation.
printf 'pub fn balance_after_refusal(balance: u32) -> u32 { balance }\n' > "$tmp/src/lib.rs"
status=0
evaluate empty || status=$?
[[ $status -eq 2 ]]
# Same expectation survives an implementation refactor (allowed-change control).
cat > "$tmp/src/lib.rs" <<'EOF'
pub fn balance_after_refusal(balance: u32) -> u32 { std::convert::identity(balance) }
#[cfg(test)]
mod tests;
EOF
evaluate equivalent
if evaluate equivalent; then
    echo 'evidence directory was overwritten' >&2
    exit 1
fi
# Exercise a real execution deadline, not only a fabricated timeout report.
sed -i 's/assert_eq!(super::balance_after_refusal(20), 20);/std::thread::sleep(std::time::Duration::from_secs(30));/' "$tmp/src/tests.rs"
status=0
bash "$runner" "$checker" "$tmp/Cargo.toml" expectation-fixture "$tmp/contract.txt" "$tmp/src/tests.rs" "$tmp/timeout" 1 "$tmp/src/lib.rs" || status=$?
[[ $status -eq 2 ]]
jq -e '.exit_code == 124' "$tmp/timeout/report.json" >/dev/null
# A test run that edits a scoped input cannot certify the starting artifact.
cat > "$tmp/src/tests.rs" <<'EOF'
#[test]
fn changes_scoped_input() {
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"), "// changed during evaluation\n").expect("fixture edit");
}
EOF
status=0
evaluate changed || status=$?
[[ $status -eq 2 ]]
jq -e '.inputs_unchanged == false' "$tmp/changed/report.json" >/dev/null
printf 'this is not Rust\n' > "$tmp/src/lib.rs"
status=0
evaluate compile-error || status=$?
[[ $status -eq 2 ]]
echo 'PASS: real failure -> correction -> remeasurement; equivalent implementation accepted; invalid evidence rejected'