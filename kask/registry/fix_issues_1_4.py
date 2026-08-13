#!/usr/bin/env python3
"""Fix Issues 1 and 4 in hKask skill manifests.

Issue 1: Raise timeout_seconds on action:select steps whose template uses
         thinking_budget = full/on/medium/unset to >= 120s (150s for
         max_tokens >= 3000, 180s for max_tokens >= 6000).
Issue 4: Replace dead `on_timeout: retry` + `max_retries: N` config with
         `on_timeout: abort` + `max_retries: 0` (the executor does not
         implement retry; see ErrorHandlingConfig docs). Keeps the schema
         valid under deny_unknown_fields and is honest about behavior.

Uses line-based text replacement to preserve comments and structure.
Idempotent: re-running on already-fixed files is a no-op.
"""
import re
import sys
from pathlib import Path

REGISTRY = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry")
MANIFESTS_DIR = REGISTRY / "manifests"
TEMPLATES_DIR = REGISTRY / "templates"

ALREADY_FIXED_TIMEOUTS = {"metacognition"}


def load_template(template_ref):
    path = TEMPLATES_DIR / f"{template_ref}.j2"
    if not path.exists():
        return None
    with open(path) as f:
        return f.read()


def extract_thinking_budget(template_text):
    if not template_text:
        return None
    m = re.search(r'thinking_budget\s*=\s*["\']?(\w+)["\']?', template_text)
    if m:
        return m.group(1).lower()
    return None


def extract_max_tokens(template_text):
    if not template_text:
        return None
    m = re.search(r'max_tokens\s*=\s*(\d+)', template_text)
    if m:
        return int(m.group(1))
    return None


def required_timeout(thinking_budget, max_tokens):
    """Determine the minimum safe timeout for a thinking-enabled select step."""
    thinking_enabled = thinking_budget in (None, "full", "on", "medium")
    if not thinking_enabled:
        return None
    if max_tokens is not None and max_tokens >= 6000:
        return 180
    if max_tokens is not None and max_tokens >= 3000:
        return 150
    return 120


def fix_issue_1(manifest_path, manifest_text, manifest_data):
    """Raise timeout_seconds on thinking-enabled select steps. Returns (new_text, change_count)."""
    if manifest_path.stem in ALREADY_FIXED_TIMEOUTS:
        return manifest_text, 0

    lines = manifest_text.splitlines(keepends=True)
    changes = 0
    steps = manifest_data.get("steps", []) or []

    # Build a map of step ordinal -> required timeout for select steps
    step_timeouts = {}  # ordinal -> required_timeout
    for step in steps:
        if step.get("action") != "select":
            continue
        template_ref = step.get("template_ref")
        if not template_ref:
            continue
        template_text = load_template(template_ref)
        tb = extract_thinking_budget(template_text)
        mt = extract_max_tokens(template_text)
        req = required_timeout(tb, mt)
        if req is None:
            continue
        ordinal = step.get("ordinal")
        current = step.get("timeout_seconds")
        if current is None:
            continue
        if current < req:
            step_timeouts[ordinal] = (current, req)

    if not step_timeouts:
        return manifest_text, 0

    # Walk lines tracking current step ordinal. Steps are list items under
    # `steps:` keyed by `- ordinal: N`. We update the first `timeout_seconds:`
    # line that follows each matching ordinal.
    current_ordinal = None
    i = 0
    while i < len(lines):
        line = lines[i]
        # Detect step ordinal: a line like "  - ordinal: 3"
        m = re.match(r'^(\s*)-\s*ordinal:\s*(\d+)\s*$', line)
        if m:
            current_ordinal = int(m.group(2))
        elif current_ordinal is not None and current_ordinal in step_timeouts:
            # Look for timeout_seconds: <num> within this step
            tm = re.match(r'^(\s*)timeout_seconds:\s*(\d+)\s*$', line)
            if tm:
                current_val = int(tm.group(2))
                _, req = step_timeouts[current_ordinal]
                if current_val < req:
                    indent = tm.group(1)
                    lines[i] = f"{indent}timeout_seconds: {req}\n"
                    changes += 1
                # Consume this step's timeout match (only first one per step)
                del step_timeouts[current_ordinal]
        i += 1

    return "".join(lines), changes


def fix_issue_4(manifest_text):
    """Replace dead `on_timeout: retry` with `on_timeout: abort` and
    `max_retries: N` with `max_retries: 0`. Idempotent."""
    lines = manifest_text.splitlines(keepends=True)
    changes = 0
    for i, line in enumerate(lines):
        if re.match(r'^(\s*)on_timeout:\s*retry\s*$', line):
            indent = re.match(r'^(\s*)', line).group(1)
            lines[i] = f"{indent}on_timeout: abort\n"
            changes += 1
        elif re.match(r'^(\s*)max_retries:\s*[1-9]\d*\s*$', line):
            indent = re.match(r'^(\s*)', line).group(1)
            lines[i] = f"{indent}max_retries: 0\n"
            changes += 1
    return "".join(lines), changes


def main():
    import yaml

    total_i1 = 0
    total_i4 = 0
    files_changed = 0

    for path in sorted(MANIFESTS_DIR.glob("*.yaml")):
        with open(path) as f:
            original = f.read()
        try:
            data = yaml.safe_load(original)
        except yaml.YAMLError as e:
            print(f"SKIP {path.name}: YAML parse error: {e}", file=sys.stderr)
            continue

        text = original
        text, c1 = fix_issue_1(path, text, data)
        text, c4 = fix_issue_4(text)

        if text != original:
            with open(path, "w") as f:
                f.write(text)
            files_changed += 1
            total_i1 += c1
            total_i4 += c4
            print(f"  {path.name}: +{c1} timeout(s) raised, +{c4} retry config stripped")

    print(f"\nTotal: {files_changed} files changed, {total_i1} timeouts raised, "
          f"{total_i4} retry configs stripped")


if __name__ == "__main__":
    main()
