#!/usr/bin/env bash
set -euo pipefail

# Regression harness for the classification queue lifecycle fixes: the
# running→terminal strand guard, the failed_runner discard-and-requeue path,
# and the input content hash. Uses a fake corpus-tool runner (the host-call
# argument convention: <tool> <arguments> <response> <log>).

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
queue_script="$repo_root/kask/scripts/audit/run-corpus-classification-queue.sh"
reconcile_script="$repo_root/kask/scripts/audit/reconcile-corpus-classification-unit.sh"

tmp=$(mktemp -d)
trap 'chmod -R u+w "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT

for command in jq sha256sum cmp; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
for script in "$queue_script" "$reconcile_script"; do
    if [[ ! -x "$script" ]]; then
        echo "required script is not executable: $script" >&2
        exit 66
    fi
done

# Two tagged-chunk inputs, two units per file.
for unit in unit-001 unit-002; do
    input="$tmp/$unit-input.jsonl"
    : > "$input"
    for i in 1 2; do
        jq -cn --arg unit "$unit" --arg i "$i" \
            '{entity_ref:("corpus:researcher:"+$unit+":"+$i),source:($unit+".jsonl"),
              text:("chunk text "+$unit+" "+$i),word_count:4}' >> "$input"
    done
done
queue="$tmp/queue.jsonl"
for unit in unit-001 unit-002; do
    jq -cn --arg unit "$unit" \
        --arg input "$tmp/$unit-input.jsonl" \
        --arg output "$tmp/$unit-output.jsonl" \
        --arg args "$tmp/$unit-args.json" \
        --arg response "$tmp/$unit-response.json" \
        --arg log "$tmp/$unit-server.log" \
        '{unit:$unit,ordinal:0,rows:2,input:$input,output:$output,
          args:$args,response:$response,log:$log,status:"pending"}'
done > "$queue"

# Fake runner: while the fail marker exists for the unit named in the
# arguments, exit 0 without writing the response (an unguarded queue exit
# inside the running→terminal window). Otherwise write a conforming
# corpus_tag_chunks response and classified output.
runner="$tmp/fake-runner"
cat > "$runner" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
tool=$1
arguments=$2
response=$3
log=$4
[[ "$tool" == corpus_tag_chunks ]] || { echo "unsupported tool" >&2; exit 64; }
input=$(jq -r '.chunks_jsonl' "$arguments")
output=$(jq -r '.output' "$arguments")
unit=$(basename "$input" -input.jsonl)
if [[ -f "${FAIL_MARKER_DIR}/${unit}" ]]; then
    rm -f "${FAIL_MARKER_DIR}/${unit}"
    # Simulate the real transport-timeout death: the wrapper creates the
    # response (empty on timeout) and the log, but never the output.
    : > "$response"
    printf 'simulated transport timeout\n' > "$log"
    exit 0
fi
jq -c '. + {classification:{status:"classified",ontology_protocol:"published-term-resolution-v1"},
          candidate_terms:["one","two","three"],ontology_tags:{core:["5w1h_core"]},
          concepts:["5w1h_core"]}' "$input" > "$output"
summary=$(jq -cn --argjson total "$(wc -l < "$input" | tr -d ' ')" \
    '{total_chunks:$total,tagged:$total,failed:0,reported_cost_usd:0.001,cost_reporting_complete:true}')
# The queue unwraps: text -> fromjson -> .content (the summary object).
text=$(jq -cn --argjson content "$summary" '{content:$content}')
jq -cn --arg text "$text" '{jsonrpc:"2.0",id:2,result:{content:[{type:"text",text:$text}],isError:false}}' > "$response"
: > "$log"
BASH
chmod +x "$runner"

export FAIL_MARKER_DIR="$tmp/fail-markers"
mkdir -p "$FAIL_MARKER_DIR"

# ── 1. The strand guard: an unguarded exit inside the running window ──────
touch "$FAIL_MARKER_DIR/unit-001"
if "$queue_script" "$queue" "$runner" 10 2 10 0.01 >"$tmp/run1.out" 2>"$tmp/run1.err"; then
    echo "queue unexpectedly survived a missing runner response" >&2
    exit 1
fi
status=$(jq -r 'select(.unit == "unit-001") | .status' "$queue")
if [[ "$status" != "failed_runner" ]]; then
    echo "strand guard failed: unit-001 status=$status (expected failed_runner)" >&2
    exit 1
fi
if [[ -f "$tmp/unit-001-output.jsonl" ]]; then
    echo "the fake never wrote output; a tool-error death must not produce one" >&2
    exit 1
fi
echo "strand guard: ok" >&2

# ── 2. Discard-and-requeue: failed_runner without output ──────────────────
"$reconcile_script" "$queue" unit-001 0.01 >"$tmp/reconcile.out" 2>"$tmp/reconcile.err"
status=$(jq -r 'select(.unit == "unit-001") | .status' "$queue")
if [[ "$status" != "pending" ]]; then
    echo "requeue failed: unit-001 status=$status (expected pending)" >&2
    exit 1
fi
reserved=$(jq -r 'select(.unit == "unit-001") | .reserved_cost_usd' "$queue")
if [[ "$reserved" != "0.02" ]]; then
    echo "requeue reserve wrong: $reserved (expected 0.02 = 2 rows * 0.01)" >&2
    exit 1
fi
orphans=$(compgen -G "$tmp/unit-001-*.orphan-*" | wc -l)
if [[ "$orphans" -lt 2 ]]; then
    echo "stale receipts were not moved aside: $orphans orphan files" >&2
    exit 1
fi
echo "discard-and-requeue: ok" >&2

# ── 3. Input content hash: a mutated pending input is refused ─────────────
cp "$tmp/unit-001-input.jsonl" "$tmp/unit-001-input.original"
jq -c '.text = "mutated"' "$tmp/unit-001-input.jsonl" > "$tmp/mutated.jsonl"
mv "$tmp/mutated.jsonl" "$tmp/unit-001-input.jsonl"
if "$queue_script" "$queue" "$runner" 10 2 10 0.01 >"$tmp/run2.out" 2>"$tmp/run2.err"; then
    echo "queue accepted a mutated input with an unchanged row count" >&2
    exit 1
fi
grep -F 'input content changed since the unit was first processed: unit-001' "$tmp/run2.err" >/dev/null
mv "$tmp/unit-001-input.original" "$tmp/unit-001-input.jsonl"
echo "input content hash: ok" >&2

# ── 4. The requeued unit completes and the queue finishes clean ────────────
"$queue_script" "$queue" "$runner" 10 2 10 0.01 >"$tmp/run3.out" 2>"$tmp/run3.err"
for unit in unit-001 unit-002; do
    status=$(jq -r --arg unit "$unit" 'select(.unit == $unit) | .status' "$queue")
    if [[ "$status" != "completed" ]]; then
        echo "$unit did not complete: status=$status" >&2
        exit 1
    fi
    if [[ -z $(jq -r --arg unit "$unit" 'select(.unit == $unit) | .input_sha256 // empty' "$queue") ]]; then
        echo "$unit completed without a recorded input hash" >&2
        exit 1
    fi
done
echo "requeue completion: ok" >&2

printf 'classification queue lifecycle test passed\n'
