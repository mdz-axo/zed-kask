#!/usr/bin/env python3
"""Re-enable on_timeout: retry now that StepMachine::dispatch_with_retry
implements it (Issue 4 Option B follow-up).

The previous session implemented retry in step_machine.rs and set metacognition
and task-breakdown to on_timeout: retry. The other 54 manifests were set to
on_timeout: abort by the Issue 4 Option A fix (when retry was not yet
implemented). Now that retry IS implemented, re-enable it on all manifests to
benefit from the resilience against cold-cache timeouts on the thinking-mode
cloud model.

Sets on_timeout: retry, max_retries: 1, retry_backoff_seconds: 1 on every
manifest that currently has on_timeout: abort. Idempotent.
"""
import re
from pathlib import Path

MANIFESTS_DIR = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry/manifests")


def reenable_retry(path):
    with open(path) as f:
        text = f.read()
    lines = text.splitlines(keepends=True)
    changed = False
    in_error_handling = False
    for i, line in enumerate(lines):
        stripped = line.rstrip()
        if re.match(r"^error_handling:\s*$", stripped):
            in_error_handling = True
            continue
        if in_error_handling:
            if stripped and not line.startswith(" ") and not line.startswith("\t"):
                in_error_handling = False
                continue
            if re.match(r"^(\s*)on_timeout:\s*abort\s*$", stripped):
                indent = re.match(r"^(\s*)", line).group(1)
                lines[i] = f"{indent}on_timeout: retry\n"
                changed = True
            elif re.match(r"^(\s*)max_retries:\s*0\s*$", stripped):
                indent = re.match(r"^(\s*)", line).group(1)
                lines[i] = f"{indent}max_retries: 1\n"
                changed = True
    if changed:
        with open(path, "w") as f:
            f.write("".join(lines))
    return changed


def main():
    n = 0
    for path in sorted(MANIFESTS_DIR.glob("*.yaml")):
        if reenable_retry(path):
            n += 1
            print(f"  {path.name}: on_timeout: retry, max_retries: 1")
    print(f"\nTotal: {n} files changed")


if __name__ == "__main__":
    main()
