#!/usr/bin/env python3
"""
Redesign kata.convergence_check — Option D migration (v2).

Fixes from v1:
- Robust step-block detection: walks backwards from the compute_ref line to
  the nearest `- ordinal:` and forwards to the next `- ordinal:` or top-level
  key. Handles conditional steps (condition: field) correctly.
- Removes `| default(1.0)` from ALL convergence_metric bindings in loop steps
  (not just the convergence_signal line), since these feed the same
  premature-convergence bug.
- Updates comments that reference kata_hypotenuse -> convergence_signal.
- Updates comments that reference kata.convergence_check (marks them as
  removed, since the primitive no longer exists).
"""
import re
import sys
from pathlib import Path


def find_step_block_containing(lines, needle):
    """Find the start and end line indices of the step block containing `needle`.

    A step block starts at `- ordinal: N` and ends at the next `- ordinal:`
    line or at a top-level key (non-indented line with `:`).

    Returns (start_idx, end_idx) or None.
    """
    for i, line in enumerate(lines):
        if needle in line:
            # Walk backwards to find the `- ordinal:` that starts this block
            start = i
            while start > 0:
                if re.match(r"\s*-\s+ordinal:", lines[start]):
                    break
                start -= 1
            else:
                # No `- ordinal:` found before this line — not a step block
                continue

            # Walk forwards to find the next `- ordinal:` or top-level key
            end = i + 1
            while end < len(lines):
                next_line = lines[end]
                # Next step block starts at `- ordinal:`
                if re.match(r"\s*-\s+ordinal:", next_line):
                    break
                # Top-level key (non-indented, non-blank, not a comment)
                stripped = next_line.lstrip()
                if (
                    next_line
                    and not next_line[0].isspace()
                    and stripped
                    and not stripped.startswith("#")
                    and ":" in next_line
                ):
                    break
                end += 1
            return (start, end)
    return None


def get_ordinal(lines, block_start):
    """Extract the ordinal number from a step block's first line."""
    m = re.search(r"ordinal:\s*(\d+)", lines[block_start])
    return int(m.group(1)) if m else None


def remove_step_containing(lines, needle):
    """Remove the step block containing `needle` and renumber subsequent steps.

    Returns (new_lines, removed_ordinal) or (lines, None) if no change.
    """
    block = find_step_block_containing(lines, needle)
    if block is None:
        return (lines, None)
    start, end = block
    removed_ordinal = get_ordinal(lines, start)
    if removed_ordinal is None:
        return (lines, None)

    # Remove the block
    new_lines = lines[:start] + lines[end:]

    # Collapse a double blank at the seam if we created one
    if start > 0 and start < len(new_lines):
        if (
            new_lines[start - 1].strip() == ""
            and start < len(new_lines)
            and new_lines[start].strip() == ""
        ):
            new_lines = new_lines[:start] + new_lines[start + 1 :]

    # Renumber subsequent steps: every `- ordinal: N` where N > removed_ordinal
    # becomes N - 1.
    for i, line in enumerate(new_lines):
        m = re.match(r"(\s*-\s+ordinal:\s*)(\d+)(\s*.*)", line)
        if m:
            n = int(m.group(2))
            if n > removed_ordinal:
                new_lines[i] = f"{m.group(1)}{n - 1}{m.group(3)}"

    return (new_lines, removed_ordinal)


def fix_loop_step_bindings(lines):
    """Fix bindings in loop steps:
    1. Rename kata_hypotenuse: -> convergence_signal:
    2. Remove `| default(1.0)` from convergence_metric bindings (the bug).
    3. Remove `| default(1.0)` from convergence_signal bindings that reference
       convergence_metric (the bug).
    """
    new_lines = []
    for line in lines:
        # Rename kata_hypotenuse key -> convergence_signal
        if "kata_hypotenuse:" in line:
            line = line.replace("kata_hypotenuse:", "convergence_signal:")

        # Remove `| default(1.0)` from convergence_metric bindings.
        # The pattern is: convergence_metric | default(1.0)
        # We want: convergence_metric  (no default — let it be null/NaN)
        line = re.sub(
            r"convergence_metric\s*\|\s*default\(1\.0\)",
            "convergence_metric",
            line,
        )

        # Also remove `| default(1.0)` from convergence_signal bindings that
        # reference convergence_metric (e.g. "{{ step_N_result.convergence_metric | default(1.0) }}")
        # These are the same premature-convergence bug.
        if "convergence_signal:" in line and "default(1.0)" in line:
            line = line.replace("| default(1.0)", "")

        new_lines.append(line)
    return new_lines


def update_convergence_context_refs(lines):
    """Update _convergence.hypotenuse_* references in remaining steps."""
    new_lines = []
    for line in lines:
        line = line.replace(
            "_convergence.hypotenuse_epsilon", "_convergence.gap_epsilon"
        )
        line = line.replace(
            "_convergence.hypotenuse_history", "_convergence.signal_history"
        )
        new_lines.append(line)
    return new_lines


def update_comments(lines):
    """Update comments that reference the old names."""
    new_lines = []
    for line in lines:
        # Update comments referencing kata_hypotenuse -> convergence_signal
        if "#" in line and "kata_hypotenuse" in line:
            line = line.replace("kata_hypotenuse", "convergence_signal")
        new_lines.append(line)
    return new_lines


def process_manifest(path):
    """Process a single manifest. Returns True if changed."""
    original = path.read_text()
    lines = original.splitlines(keepends=False)

    # Step 1: Remove ALL kata.convergence_check step blocks + renumber.
    # Some manifests have multiple (conditional) convergence_check steps.
    changed = True
    while changed:
        lines, removed = remove_step_containing(lines, "compute_ref: kata.convergence_check")
        changed = removed is not None

    # Step 2: Fix loop step bindings
    lines = fix_loop_step_bindings(lines)

    # Step 3: Update _convergence.hypotenuse_* refs
    lines = update_convergence_context_refs(lines)

    # Step 4: Update comments
    lines = update_comments(lines)

    new_content = "\n".join(lines)
    if original.endswith("\n"):
        new_content = new_content + "\n"

    if new_content != original:
        path.write_text(new_content)
        return True
    return False


def main():
    manifests_dir = Path("kask/registry/manifests")
    changed = 0
    skipped = 0
    for yaml_file in sorted(manifests_dir.glob("*.yaml")):
        content = yaml_file.read_text()
        if "kata.convergence_check" not in content and "kata_hypotenuse" not in content and "hypotenuse_epsilon" not in content and "hypotenuse_history" not in content:
            skipped += 1
            continue
        if process_manifest(yaml_file):
            changed += 1
            print(f"  CHANGED: {yaml_file.name}")
        else:
            print(f"  UNCHANGED: {yaml_file.name}")
    print(f"\n{changed} manifests changed, {skipped} skipped")


if __name__ == "__main__":
    main()
