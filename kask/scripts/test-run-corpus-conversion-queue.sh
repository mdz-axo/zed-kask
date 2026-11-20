#!/usr/bin/env bash
set -euo pipefail

# Regression harness for the conversion queue: manifest build with resume
# classification, the running→terminal strand guard, the --requeue path for
# tool-error deaths, and the source content hash. Uses a fake corpus-tool
# runner with the host-call argument convention.

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
producer="$repo_root/kask/scripts/audit/build-corpus-conversion-queue.sh"
queue_script="$repo_root/kask/scripts/audit/run-corpus-conversion-queue.sh"

tmp=$(mktemp -d)
trap 'chmod -R u+w "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT

for command in jq sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
for script in "$producer" "$queue_script"; do
    if [[ ! -x "$script" ]]; then
        echo "required script is not executable: $script" >&2
        exit 66
    fi
done

sources="$tmp/sources"
outputs="$tmp/extracted"
mkdir -p "$sources"
for name in alpha-source.html beta-source.html; do
    printf '%s\n' "Conversion probe text for $name with several words." > "$sources/$name"
done

# Fake runner: while the fail marker exists for the unit named in the
# arguments, simulate the real transport-timeout death (response file
# created empty, log written, no output). Otherwise write the output text,
# the report companion, and a conforming corpus_convert response.
runner="$tmp/fake-runner"
cat > "$runner" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
tool=$1
arguments=$2
response=$3
log=$4
[[ "$tool" == corpus_convert ]] || { echo "unsupported tool" >&2; exit 64; }
path=$(jq -r '.path' "$arguments")
output=$(jq -r '.output' "$arguments")
name=$(basename "$path")
if [[ -f "${FAIL_MARKER_DIR}/${name}" ]]; then
    rm -f "${FAIL_MARKER_DIR}/${name}"
    : > "$response"
    printf 'simulated transport timeout\n' > "$log"
    exit 0
fi
printf 'converted text of %s with enough words to count\n' "$name" > "$output"
report="${output}.report.json"
jq -cn --arg output "$output" \
    '{format:"html",method:"text_extraction",word_count:9,verification_passed:true,
      output:$output,text_sha256:"deadbeef"}' > "$report"
summary=$(jq -cn --arg output "$output" --arg report "$report" \
    '{format:"html",method:"text_extraction",word_count:9,output:$output,report:$report}')
text=$(jq -cn --argjson content "$summary" '{content:$content}')
jq -cn --arg text "$text" '{jsonrpc:"2.0",id:2,result:{content:[{type:"text",text:$text}],isError:false}}' > "$response"
: > "$log"
BASH
chmod +x "$runner"
export FAIL_MARKER_DIR="$tmp/fail-markers"
mkdir -p "$FAIL_MARKER_DIR"

# ── 1. Manifest build: both pending ───────────────────────────────────────
queue="$tmp/queue.jsonl"
"$producer" "$sources" "$outputs" "$queue" >"$tmp/build.out" 2>"$tmp/build.err"
[[ $(jq -s '[.[] | select(.status == "pending")] | length' "$queue") -eq 2 ]]
echo "manifest build: ok" >&2

# ── 2. Strand guard: transport death inside the running window ────────────
touch "$FAIL_MARKER_DIR/alpha-source.html"
if "$queue_script" "$queue" "$runner" >"$tmp/run1.out" 2>"$tmp/run1.err"; then
    echo "queue unexpectedly survived a missing runner response" >&2
    exit 1
fi
status=$(jq -r 'select(.unit | endswith("alpha-source")) | .status' "$queue")
if [[ "$status" != "failed_runner" ]]; then
    echo "strand guard failed: alpha status=$status (expected failed_runner)" >&2
    exit 1
fi
if [[ -f "$(jq -r 'select(.unit | endswith("alpha-source")) | .output' "$queue")" ]]; then
    echo "a tool-error death must not produce output" >&2
    exit 1
fi
echo "strand guard: ok" >&2

# ── 3. --requeue: stale receipts moved aside, unit pending again ──────────
alpha_unit=$(jq -r 'select(.unit | endswith("alpha-source")) | .unit' "$queue")
"$queue_script" --requeue "$queue" "$alpha_unit" >"$tmp/requeue.out" 2>"$tmp/requeue.err"
status=$(jq -r --arg unit "$alpha_unit" 'select(.unit == $unit) | .status' "$queue")
if [[ "$status" != "pending" ]]; then
    echo "requeue failed: alpha status=$status (expected pending)" >&2
    exit 1
fi
orphans=$(compgen -G "$outputs/.receipts/*.orphan-*" | wc -l)
if [[ "$orphans" -lt 2 ]]; then
    echo "stale receipts were not moved aside: $orphans orphan files" >&2
    exit 1
fi
echo "requeue: ok" >&2

# ── 4. Source mutation refused ────────────────────────────────────────────
cp "$sources/alpha-source.html" "$tmp/alpha.original"
printf '%s\n' "mutated source content" > "$sources/alpha-source.html"
if "$queue_script" "$queue" "$runner" >"$tmp/run2.out" 2>"$tmp/run2.err"; then
    echo "queue accepted a mutated source" >&2
    exit 1
fi
grep -F "source content changed since the queue was built" "$tmp/run2.err" >/dev/null
mv "$tmp/alpha.original" "$sources/alpha-source.html"
echo "source hash: ok" >&2

# ── 5. Requeued unit completes; queue finishes clean ──────────────────────
"$queue_script" "$queue" "$runner" >"$tmp/run3.out" 2>"$tmp/run3.err"
[[ $(jq -s '[.[] | select(.status == "completed")] | length' "$queue") -eq 2 ]]
for unit in $(jq -r '.unit' "$queue"); do
    [[ -n $(jq -r --arg unit "$unit" 'select(.unit == $unit) | .output_sha256 // empty' "$queue") ]]
done
echo "requeue completion: ok" >&2

# ── 6. Manifest rebuild over completed receipts: resume by receipt ─────────
queue2="$tmp/queue2.jsonl"
"$producer" "$sources" "$outputs" "$queue2" >"$tmp/build2.out" 2>"$tmp/build2.err"
[[ $(jq -s '[.[] | select(.status == "completed")] | length' "$queue2") -eq 2 ]]
echo "resume by receipt: ok" >&2

printf 'conversion queue lifecycle test passed\n'
