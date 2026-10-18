#!/usr/bin/env bash
#
# recover-keychain.sh — Recover API keys stranded in the old rust-keyring
# namespace (service=stack.user.mdz-axolotl.key, username=<ENV_VAR>) into
# the canonical credential URL that zed-kask actually reads.
#
# Background: commits 84fc7f12e2 + 511e591748 (Aug 24) moved data-service
# keys to kask://credentials/*; later inference-provider consolidation moved
# those keys to their provider api_url slots. Keys left only in the old
# namespace became invisible even though they remained in the keyring.
#
# Usage:
#   bash script/recover-keychain.sh           # dry-run (default)
#   bash script/recover-keychain.sh --apply   # actually write
#
# Requires: libsecret-tools (secret-tool), gnome-keyring-daemon running.

set -eu

MODE="dry-run"
if [ "${1:-}" = "--apply" ]; then
  MODE="apply"
fi

OLD_SERVICE="stack.user.mdz-axolotl.key"
LABEL="zed-github-account"

# Read a secret from the old namespace.
# Args: <old_username_env_var>
read_old() {
  secret-tool lookup username "$1" service "$OLD_SERVICE" 2>/dev/null || true
}

# Resolve the canonical target URL for a credential key. Inference-provider
# keys live at the provider API URL; pure data-service keys use the kask
# namespace. RunPod remains live for OCR, so recovering it to the retired
# kask://credentials/runpod slot would report success while leaving OCR broken.
credential_url() {
  case "$1" in
    openrouter) printf '%s\n' 'https://openrouter.ai/api/v1' ;;
    runpod) printf '%s\n' 'https://api.runpod.io' ;;
    *) printf 'kask://credentials/%s\n' "$1" ;;
  esac
}

# Read a secret from its canonical current URL.
# Args: <credential_key>
read_new() {
  url=$(credential_url "$1")
  secret-tool lookup url "$url" 2>/dev/null || true
}

# Write a secret to its canonical current URL with the exact attribute shape
# zed's LinuxPlatform::write_credentials produces (label=zed-github-account,
# username=kask).
# Args: <credential_key> <value>
write_new() {
  local key="$1" value="$2" url
  url=$(credential_url "$key")
  printf '%s' "$value" | secret-tool store \
    --label="$LABEL" \
    url "$url" \
    username "kask" \
    2>/dev/null
}

# ─── Recovery map: old_username → new_credential_key ───
# Only keys that (a) have a current credential consumer and (b) are
# recoverable from the old namespace. Conflicted keys (already re-entered with a different value) are
# listed separately for reporting but NOT overwritten.
declare -a RECOVER=(
  "RUNPOD_API_KEY:runpod"
  "FMP_API_KEY:fmp"
)

# Keys present in both namespaces with DIFFERENT values — the new value was
# re-entered manually after the namespace split. Don't overwrite; just report.
declare -a CONFLICTS=(
  "OPENROUTER_API_KEY:openrouter"
)

# Keys that were never in either namespace — genuinely need regeneration.
# (The `eodhd:HKASK_EODHD_API_KEY` entry retired with the companies server and
# the FMP/EODHD providers, 2026-10-18 — nothing consumes it.)
declare -a LOST=(
  "fred:HKASK_FRED_API_KEY"
  "hkask_abw_api_key:HKASK_ABW_API_KEY"
  "hkask_smtp_password:HKASK_SMTP_PASSWORD"
  "serpapi:HKASK_SERPAPI_API_KEY"
)

# Old-namespace keys with no current credential consumer. Listed for
# awareness; not recovered. (Browserbase was dropped in fb2acf99c8; Qdrant
# and GitHub are handled outside this helper; training-only HuggingFace keys
# retired with the training capability.)
declare -a NO_CONSUMER=(
  "BROWSERBASE_API_KEY"
  "BROWSERBASE_PROJECT_ID"
  "QDRANT_API_KEY"
  "QDRANT_CLUSTER_ENDPOINT"
  "GITHUB_TOKEN"
  "GITHUB_CLASSIC_PAT"
  "RUNPOD_REGISTRY_AUTH_ID"
  "FAL_KEY"
  "TELNYX_API_KEY"
  "HF_TOKEN"
  "HUGGINGFACE_API_KEY"
)

echo "=== keychain recovery ($MODE) ==="
echo ""

# ─── 1. Recover safe keys ───
echo "--- safe recovery (missing in new, present in old) ---"
recovered=0
skipped=0
for pair in "${RECOVER[@]}"; do
  old_user="${pair%%:*}"
  new_key="${pair##*:}"
  old_val=$(read_old "$old_user")
  new_val=$(read_new "$new_key")

  if [ -n "$new_val" ]; then
    echo "  $new_key: SKIP (already present in new namespace, ${#new_val} chars)"
    skipped=$((skipped + 1))
    continue
  fi

  if [ -z "$old_val" ]; then
    echo "  $new_key: SKIP (not found in old namespace either — genuinely lost)"
    skipped=$((skipped + 1))
    continue
  fi

  if [ "$MODE" = "apply" ]; then
    write_new "$new_key" "$old_val"
    target_url=$(credential_url "$new_key")
    echo "  $new_key: RECOVERED (${#old_val} chars) from old $old_user → $target_url"
  else
    target_url=$(credential_url "$new_key")
    echo "  $new_key: WOULD RECOVER (${#old_val} chars) from old $old_user → $target_url"
  fi
  recovered=$((recovered + 1))
done
echo "  → $recovered to recover, $skipped skipped"
echo ""

# ─── 2. Report conflicts (do NOT overwrite) ───
echo "--- conflicts (present in both, values differ — keeping new) ---"
for pair in "${CONFLICTS[@]}"; do
  old_user="${pair%%:*}"
  new_key="${pair##*:}"
  old_val=$(read_old "$old_user")
  new_val=$(read_new "$new_key")

  if [ -z "$old_val" ]; then
    echo "  $new_key: old entry gone (nothing to conflict)"
    continue
  fi
  if [ -z "$new_val" ]; then
    # New is missing — this is actually a safe recovery candidate, not a conflict.
    if [ "$MODE" = "apply" ]; then
      write_new "$new_key" "$old_val"
      echo "  $new_key: RECOVERED (${#old_val} chars) — new was missing, recovered from old $old_user"
    else
      echo "  $new_key: WOULD RECOVER (${#old_val} chars) — new was missing, recover from old $old_user"
    fi
    continue
  fi
  if [ "$old_val" = "$new_val" ]; then
    echo "  $new_key: values match (already consistent)"
  else
    echo "  $new_key: CONFLICT — old=${#old_val}c new=${#new_val}c (keeping new, not overwriting)"
  fi
done
echo ""

# ─── 3. Report genuinely lost keys ───
echo "--- genuinely lost (never in either namespace — regenerate these) ---"
for pair in "${LOST[@]}"; do
  new_key="${pair%%:*}"
  env_var="${pair##*:}"
  new_val=$(read_new "$new_key")
  if [ -z "$new_val" ]; then
    echo "  $new_key ($env_var): MISSING — regenerate at the provider dashboard"
  else
    echo "  $new_key ($env_var): present (${#new_val} chars)"
  fi
done
echo ""

# ─── 4. Report no-consumer keys ───
echo "--- old-namespace keys with no current consumer (not recovered) ---"
for old_user in "${NO_CONSUMER[@]}"; do
  old_val=$(read_old "$old_user")
  if [ -n "$old_val" ]; then
    echo "  $old_user: present in old (${#old_val} chars) — no current credential mapping"
  else
    echo "  $old_user: not found"
  fi
done
echo ""

if [ "$MODE" = "dry-run" ] && [ "$recovered" -gt 0 ]; then
  echo "Dry run complete. $recovered key(s) would be recovered."
  echo "Run with --apply to write them:"
  echo "  bash script/recover-keychain.sh --apply"
elif [ "$MODE" = "apply" ] && [ "$recovered" -gt 0 ]; then
  echo "Applied. $recovered key(s) recovered to their canonical credential URLs."
  echo "Restart zed-kask (or any running MCP servers) to pick them up."
else
  echo "Nothing to recover."
fi
