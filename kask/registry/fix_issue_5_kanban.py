#!/usr/bin/env python3
"""Fix Issue 5 (kanban-task-management): add `triage_phase: string` to the
contract.input of every kanban-task-management template that uses it.

The templates reference {{ triage_phase }} but don't declare it in
contract.input, so the input_mapping ↔ contract cross-check flags it as a
potential typo. It's actually a real input the templates consume — the
contract is just incomplete. This adds the declaration so the contract
documents what the template uses.

Idempotent.
"""
import re
from pathlib import Path

TEMPLATES_DIR = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry/templates/kanban-task-management")


def add_triage_phase_to_contract(path):
    with open(path) as f:
        text = f.read()
    if "triage_phase:" in text.split("---")[0]:
        return False  # already declared in frontmatter
    # Find the contract.input block and add triage_phase: string.
    # Pattern: `contract:\n  input:\n    <key>: <type>\n`
    # We insert `    triage_phase: string\n` after the `  input:` line.
    lines = text.splitlines(keepends=True)
    in_input = False
    inserted = False
    for i, line in enumerate(lines):
        stripped = line.rstrip()
        if re.match(r"^\s*input:\s*$", stripped):
            in_input = True
            continue
        if in_input:
            # First key line under input:
            if re.match(r"^(\s+)\w+:\s*\w+", stripped):
                indent = re.match(r"^(\s+)", line).group(1)
                lines.insert(i, f"{indent}triage_phase: string\n")
                inserted = True
                break
    if not inserted:
        return False
    with open(path, "w") as f:
        f.write("".join(lines))
    return True


def main():
    n = 0
    for path in sorted(TEMPLATES_DIR.glob("*.j2")):
        with open(path) as f:
            content = f.read()
        if "triage_phase" not in content:
            continue
        if add_triage_phase_to_contract(path):
            n += 1
            print(f"  {path.name}: added triage_phase: string to contract.input")
        else:
            print(f"  {path.name}: already has triage_phase in contract (or no contract)")
    print(f"\nTotal: {n} templates updated")


if __name__ == "__main__":
    main()
