#!/usr/bin/env python3
"""Fix Issue 2 (remaining): add enforce_inputs: true to skills that already
wire {{ task }} into their input_mapping. These skills were not in the Issue 3
list because they already use {{ task }}, but they still lack enforce_inputs.

Skills: harness-optimize, lisp-scaffold-reasoning, lora-training,
        swarm-intelligence, swarm-steering, upstream-rebase.

Idempotent.
"""
import re
from pathlib import Path

MANIFESTS_DIR = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry/manifests")

SKILLS = [
    "harness-optimize",
    "lisp-scaffold-reasoning",
    "lora-training",
    "swarm-intelligence",
    "swarm-steering",
    "upstream-rebase",
]


def add_enforce_inputs(path):
    with open(path) as f:
        text = f.read()
    if re.search(r"^\s*enforce_inputs:\s*true\s*$", text, re.MULTILINE):
        return False
    lines = text.splitlines(keepends=True)
    in_manifest = False
    manifest_last_line = None
    for i, line in enumerate(lines):
        stripped = line.rstrip()
        if re.match(r"^manifest:\s*$", stripped):
            in_manifest = True
            continue
        if in_manifest:
            if stripped and not line.startswith(" ") and not line.startswith("\t"):
                break
            if stripped and (line.startswith(" ") or line.startswith("\t")):
                if not stripped.lstrip().startswith("#"):
                    manifest_last_line = i
    if manifest_last_line is None:
        return False
    lines.insert(manifest_last_line + 1, "  enforce_inputs: true\n")
    with open(path, "w") as f:
        f.write("".join(lines))
    return True


def main():
    n = 0
    for skill in SKILLS:
        path = MANIFESTS_DIR / f"{skill}.yaml"
        if not path.exists():
            print(f"SKIP {skill}: not found")
            continue
        if add_enforce_inputs(path):
            n += 1
            print(f"  {skill}: enforce_inputs: true added")
        else:
            print(f"  {skill}: already has enforce_inputs")
    print(f"\nTotal: {n} files changed")


if __name__ == "__main__":
    main()
