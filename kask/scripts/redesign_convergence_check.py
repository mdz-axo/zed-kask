#!/usr/bin/env python3
"""
Redesign kata.convergence_check — Option D migration.

For each manifest that uses kata.convergence_check:
1. Remove the kata.convergence_check compute step entirely (it was a dead
   diagnostic — the ConvergenceTracker is the real gate).
2. Renumber subsequent step ordinals to close the gap.
3. Rename kata_hypotenuse: -> convergence_signal: in the loop step.
4. Remove `| default(1.0)` from convergence_metric bindings in the
   convergence_signal line (the premature-convergence bug — a missing
   convergence_metric should be NaN, not a fake 1.0).
5. Update any remaining _convergence.hypotenuse_epsilon ->
   _convergence.gap_epsilon and _convergence.hypotenuse_history ->
   _convergence.signal_history references (these appear in steps that
   read the _convergence context block, e.g. report steps).

The script is idempotent: running it twice produces the same output.
"""
import re
import sys
from pathlib import Path


def find_convergence_check_block(lines):
    """Find the start and end line indices of the kata.convergence_check step block.

    Returns (start_idx, end_idx) where start_idx is the line with
    `- ordinal: N` and end_idx is the last line of the block (exclusive
    in slice terms: lines[start_idx:end_idx] is the block).

    The block ends at the next `- ordinal:` line or at the next top-level
    key (error_handling:, ledger:, audit:, etc.) or EOF.
    """
    for i, line in enumerate(lines):
        # Match the compute_ref inside a step block
        if "compute_ref: kata.convergence_check" in line:
            # Walk backwards to find the `- ordinal:` that starts this block
            start = i
            while start > 0 and not re.match(r"\s*-\s+ordinal:", lines[start]):
                start -= 1
            # Walk forwards to find the next `- ordinal:` or top-level key
            end = i + 1
            while end < len(lines):
                next_line = lines[end]
                # Next step block starts at `- ordinal:`
                if re.match(r"\s*-\s+ordinal:", next_line):
                    break
                # Top-level key (non-indented, non-blank, not a comment)
                if (
                    next_line
                    and not next_line.startswith(" ")
                    and not next_line.startswith("\t")
                    and not next_line.startswith("#")
                    and ":" in next_line
                    and not next_line.startswith("  ")
                ):
                    break
                end += 1
            return (start, end)
    return None


def get_ordinal(lines, block_start):
    """Extract the ordinal number from a step block's first line."""
    m = re.search(r"ordinal:\s*(\d+)", lines[block_start])
    return int(m.group(1)) if m else None


def remove_convergence_check_step(lines):
    """Remove the kata.convergence_check step block and renumber subsequent steps.

    Returns (new_lines, removed_ordinal) or (lines, None) if no change.
    """
    block = find_convergence_check_block(lines)
    if block is None:
        return (lines, None)
    start, end = block
    removed_ordinal = get_ordinal(lines, start)
    if removed_ordinal is None:
        return (lines, None)

    # Remove the block. Also remove a trailing blank line if present
    # (to avoid double blanks).
    new_lines = lines[:start] + lines[end:]
    # If we created a double blank at the seam, collapse it.
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


def rename_kata_hypotenuse(lines):
    """Rename kata_hypotenuse: -> convergence_signal: in loop step input_mapping.

    Also remove `| default(1.0)` from convergence_metric bindings (the
    premature-convergence bug). A missing convergence_metric should resolve
    to null/NaN, not a fake 1.0 that causes premature Cauchy convergence.
    """
    new_lines = []
    for line in lines:
        if "kata_hypotenuse:" in line:
            # Rename the key
            line = line.replace("kata_hypotenuse:", "convergence_signal:")
            # Remove `| default(1.0)` from convergence_metric bindings.
            # The pattern is: convergence_metric | default(1.0)
            # We want: convergence_metric  (no default — let it be null/NaN)
            # But only remove the default(1.0), not other defaults like default(0.0).
            line = re.sub(
                r"convergence_metric\s*\|\s*default\(1\.0\)",
                "convergence_metric",
                line,
            )
        new_lines.append(line)
    return new_lines


def update_convergence_context_refs(lines):
    """Update _convergence.hypotenuse_* references in remaining steps.

    After removing the convergence_check step, other steps (e.g. report steps)
    may still reference _convergence.hypotenuse_epsilon or
    _convergence.hypotenuse_history. Update them to the renamed keys.
    """
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


def process_manifest(path):
    """Process a single manifest. Returns True if changed, False if unchanged."""
    original = path.read_text()
    lines = original.splitlines(keepends=False)

    # Step 1: Remove the kata.convergence_check step + renumber
    lines, removed = remove_convergence_check_step(lines)

    # Step 2: Rename kata_hypotenuse -> convergence_signal + fix default(1.0)
    lines = rename_kata_hypotenuse(lines)

    # Step 3: Update _convergence.hypotenuse_* refs
    lines = update_convergence_context_refs(lines)

    new_content = "\n".join(lines)
    if not original.endswith("\n"):
        original = original  # preserve no-trailing-newline
    else:
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
        if "kata.convergence_check" not in content and "kata_hypotenuse" not in content:
            skipped += 1
            continue
        if process_manifest(yaml_file):
            changed += 1
            print(f"  CHANGED: {yaml_file.name}")
        else:
            print(f"  UNCHANGED: {yaml_file.name}")
    print(f"\n{changed} manifests changed, {skipped} skipped (no convergence_check/hypotenuse)")


if __name__ == "__main__":
    main()
