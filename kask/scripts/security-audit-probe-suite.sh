#!/usr/bin/env bash
# security-audit-probe-suite.sh — the deterministic evidence floor for the
# security-audit skill (SKILL.md Step 2).
#
# The skill's judgment layers (interpretation, severity, falsifiers) stay with
# the agent. This harness pins what the agent may NOT choose: the sweep
# battery and its known-positive validation. Every probe below validates
# itself against a fixture tree planted with violations BEFORE it may report
# on a target (the check-*-selftest.sh pattern, made intrinsic to the run) —
# a probe whose fixture check fails is reported as broken and the run exits 1:
# an unvalidated instrument never produces evidence (.rules known-positive rule).
#
# Rows are JSONL on stdout: one `run` header, one `probe` row per probe, one
# `summary` footer. Every target match carries file:line, a preview, and a
# test_scoped stamp (lines after the first #[cfg(test)] marker in their file
# are counted separately — the .rules file-metrics trap, made mechanical).
#
# Usage:
#   bash kask/scripts/security-audit-probe-suite.sh --selftest
#   bash kask/scripts/security-audit-probe-suite.sh --target <path> [--surfaces S2,S3,S4,S5,S7,S8]
#
# Exit codes: 0 = all probes validated (and, in target mode, the battery ran);
#             1 = a probe's fixture validation failed, or the scan errored —
#                 never a silent pass.
#
# Version: 1.0  (pinned by .agents/skills/security-audit/SKILL.md — the SA-2
# gate checks the run row's version against the pin)

set -uo pipefail

VERSION="1.0"
ALL_SURFACES="S2,S3,S4,S5,S7,S8"
TARGET=""
MODE=""
SURFACES="$ALL_SURFACES"

while [ $# -gt 0 ]; do
    case "$1" in
        --selftest) MODE="selftest" ;;
        --target) MODE="target"; TARGET="${2:?--target requires a path}"; shift ;;
        --surfaces) SURFACES="${2:?--surfaces requires a list}"; shift ;;
        *) echo "unknown argument: $1" >&2; exit 1 ;;
    esac
    shift
done

if [ -z "$MODE" ]; then
    echo "usage: $0 --selftest | --target <path> [--surfaces S2,...]" >&2
    exit 1
fi
if [ "$MODE" = "target" ] && [ ! -e "$TARGET" ]; then
    echo "FAIL: target does not exist: $TARGET" >&2
    exit 1
fi

# ── JSONL escaping ──────────────────────────────────────────────────────────
json_escape() {
    # Escape a single-line preview for a JSON string: backslash, double quote,
    # control chars, and any stray newlines/tabs.
    printf '%s' "$1" \
        | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/\t/ /g' -e 's/[\x01-\x1f]//g' \
        | tr -d '\n\r' \
        | cut -c1-160
}

# ── Fixture tree (the known-positive for every probe) ────────────────────────
FIXTURE_DIR=""
make_fixtures() {
    FIXTURE_DIR="$(mktemp -d)"
    mkdir -p "$FIXTURE_DIR"
    cat > "$FIXTURE_DIR/probe_fixtures.rs" <<'FIXEOF'
use std::env;
// S2-01: direct env read of the DB passphrase outside the canonical resolver
let p = std::env::var("HKASK_DB_PASSPHRASE");
// S2-02: the source-shipped default passphrase used outside passphrase.rs
let fallback = DEFAULT_PASSPHRASE.to_string();
// S2-03: credential resolution with a silent fallback
let memory_passphrase = non_empty_or_env(resolve_db_passphrase(&map).ok(), fallback);
pub struct ConfigRequest {
    // S3-01: serde_json::Value in a request struct field (the bare-true schemars defect)
    pub payload: serde_json::Value,
    // S3-02: the AnyJsonValue contract
    pub proposals: AnyJsonValue,
    // S4-01: caller-supplied path parameter
    pub media_path: String,
}
fn canonical_root() { let c = base.canonicalize(); if c.starts_with(&root) {} }
// S5-01: a spawn site
let out = tokio::process::Command::new("ffmpeg").arg("-i").arg(path).output();
// S5-02: an interpolated program path — command construction from strings
let out = tokio::process::Command::new(&format!("ffmpeg-{}", tool)).output();
// S7-01: a destructive function definition
fn purge_expired_files() {}
#[tool(description = "Delete a file from the gallery and its index entry")]
// S8-01 (production scope): silent discard of a fallible operation
let _ = fs::remove_file(path);
// S8-02 (production scope): .ok()? on a fallible operation
let value = maybe_result().ok()?;
#[cfg(test)]
mod tests {
    // S8-01/S8-02 (test scope — must be counted separately, not as production)
    let _ = fs::remove_file(other);
    let v2 = maybe_result().ok()?;
}
FIXEOF
}

# ── Probes ───────────────────────────────────────────────────────────────────
# Each probe is a function taking a root directory; it emits `file<TAB>line<TAB>preview`
# rows on stdout for the given root. run_probe validates against fixtures first.

probe_S2_01() { grep -rnE 'env(_)?::var(_os)?\("HKASK_DB_PASSPHRASE"\)|env_var\("HKASK_DB_PASSPHRASE"' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S2_02() { grep -rn 'DEFAULT_PASSPHRASE' "$1" 2>/dev/null | grep -v 'hkask-keystore/src/passphrase.rs' | grep -v 'provision' | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S2_03() { grep -rnE '(resolve_db_passphrase|resolve_credential|passphrase)[^;]*\.ok\(\)|\.ok\(\)[^;]*passphrase' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S3_01() { grep -rnE 'pub [a-z_]+: *serde_json::Value[ ,]' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S3_02() { grep -rn 'AnyJsonValue' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S4_01() { grep -rnE 'pub [a-z_]*(path|dir)[a-z_]*: *String' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S4_02() { grep -rn 'canonicalize' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S5_01() { grep -rnE '(tokio::process|std::process)?::?Command::new\(' "$1" 2>/dev/null | grep -v 'std::process::Command::new(std::env::current_exe' | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S5_02() { grep -rnE 'Command::new\(&?format!|Command::new\(&[a-z_]+[,)]' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S7_01() { grep -rnE 'fn [a-z_]*(delete|remove|purge|destroy)[a-z_]*\(' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S7_02() { grep -rnE 'description = "[^"]*[Dd]elete' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S8_01() { grep -rnE 'let _ = [a-z_][0-9a-z_:.]*\(' "$1" 2>/dev/null | grep -v 'let _ = (' | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }
probe_S8_02() { grep -rnF '.ok()?' "$1" 2>/dev/null | cut -d: -f1,2 --output-delimiter=$'\t' | awk -F'\t' '{print $1"\t"$2"\t"substr($0, length($1)+length($2)+3)}'; }

# probe_id|surface|function|adjudication_note
PROBES=(
    "S2-01|S2|probe_S2_01|direct HKASK_DB_PASSPHRASE env reads outside the canonical resolver (each site adjudicated canonical-or-inline)"
    "S2-02|S2|probe_S2_02|source-shipped DEFAULT_PASSPHRASE usage outside passphrase.rs + provisioning (the R-01 fail-open class)"
    "S2-03|S2|probe_S2_03|credential-resolution-adjacent silent fallbacks (.ok() on the resolution call itself)"
    "S3-01|S3|probe_S3_01|serde_json::Value in pub struct fields (the bare-true schemars defect when it is a tool request field)"
    "S3-02|S3|probe_S3_02|AnyJsonValue usage sites (the compliant arbitrary-JSON contract)"
    "S4-01|S4|probe_S4_01|caller-supplied path/dir String fields in pub structs (each adjudicated for containment)"
    "S4-02|S4|probe_S4_02|canonicalize sites (containment controls census)"
    "S5-01|S5|probe_S5_01|process Command spawn sites (fixed binary vs interpolated; env handling) — self-spawn test harness form excluded"
    "S5-02|S5|probe_S5_02|interpolated Command::new programs (format!/variables — injection-adjacent construction)"
    "S7-01|S7|probe_S7_01|destructive function definitions (delete/remove/purge/destroy — each adjudicated for authorization gates)"
    "S7-02|S7|probe_S7_02|tool-schema descriptions naming deletion (destructive tool surface)"
    "S8-01|S8|probe_S8_01|silent fallible-operation discards: let _ = f(...) (the .rules prohibition class)"
    "S8-02|S8|probe_S8_02|.ok()? collapse sites (the .rules silent-discard class)"
)

first_cfg_test_line() {
    grep -n -m1 '#\[cfg(test)\]' "$1" 2>/dev/null | cut -d: -f1
}

# The real work: run one probe against a root, emit its JSONL row.
run_probe() {
    local probe_id="$1" surface="$2" fn="$3" note="$4" root="$5" validated="$6"
    local prod=0 testscoped=0 total=0
    local matches=""
    while IFS=$'\t' read -r file line preview; do
        [ -z "${file:-}" ] && continue
        [ -z "${line:-}" ] && continue
        total=$((total + 1))
        local cfg_line scoped="false"
        cfg_line="$(first_cfg_test_line "$file")"
        if [ -n "$cfg_line" ] && [ "$line" -gt "$cfg_line" ] 2>/dev/null; then
            scoped="true"
            testscoped=$((testscoped + 1))
        else
            prod=$((prod + 1))
        fi
        matches+="{\"file\":\"$(json_escape "$file")\",\"line\":${line},\"preview\":\"$(json_escape "$preview")\",\"test_scoped\":${scoped}},"
    done < <("$fn" "$root")
    # trailing comma -> array
    if [ -n "$matches" ]; then
        matches="${matches%,}"
        matches="[$matches]"
    else
        matches="[]"
    fi
    printf '{"row":"probe","probe_id":"%s","surface":"%s","instrument_validated":%s,"note":"%s","production_matches":%d,"test_scoped_matches":%d,"matches":%s}\n' \
        "$probe_id" "$surface" "$validated" "$(json_escape "$note")" "$prod" "$testscoped" "$matches"
}

# ── Fixture validation: every probe must catch its planted specimen ─────────
make_fixtures
BROKEN=0
declare -A VALIDATED
for spec in "${PROBES[@]}"; do
    IFS='|' read -r probe_id surface fn note <<< "$spec"
    fixture_hits="$("$fn" "$FIXTURE_DIR" 2>/dev/null | wc -l | tr -d ' ')"
    if [ "$fixture_hits" -ge 1 ]; then
        VALIDATED[$probe_id]="true"
    else
        VALIDATED[$probe_id]="false"
        echo "FAIL: probe $probe_id did not detect its planted fixture specimen — instrument broken, refusing to run (never a silent pass)." >&2
        BROKEN=1
    fi
done
rm -rf "$FIXTURE_DIR"

if [ "$MODE" = "selftest" ]; then
    if [ "$BROKEN" -ne 0 ]; then
        exit 1
    fi
    echo "OK: all ${#PROBES[@]} probes detected their planted fixtures (instrument validation green)."
    exit 0
fi
if [ "$BROKEN" -ne 0 ]; then
    # In target mode a broken instrument is still a hard failure.
    exit 1
fi

# ── Target run ───────────────────────────────────────────────────────────────
printf '{"row":"run","script":"security-audit-probe-suite","version":"%s","target":"%s","surfaces":"%s","started":"%s"}\n' \
    "$VERSION" "$(json_escape "$TARGET")" "$SURFACES" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

prod_total=0
test_total=0
for spec in "${PROBES[@]}"; do
    IFS='|' read -r probe_id surface fn note <<< "$spec"
    case ",$SURFACES," in
        *",$surface,"*) ;;
        *) continue ;;
    esac
    row="$(run_probe "$probe_id" "$surface" "$fn" "$note" "$TARGET" "${VALIDATED[$probe_id]}")"
    printf '%s\n' "$row"
    p="$(printf '%s' "$row" | grep -o '"production_matches":[0-9]*' | cut -d: -f2)"
    t="$(printf '%s' "$row" | grep -o '"test_scoped_matches":[0-9]*' | cut -d: -f2)"
    prod_total=$((prod_total + ${p:-0}))
    test_total=$((test_total + ${t:-0}))
done

printf '{"row":"summary","probes_run":"%s","version":"%s","production_match_total":%d,"test_scoped_match_total":%d}\n' \
    "$SURFACES" "$VERSION" "$prod_total" "$test_total"