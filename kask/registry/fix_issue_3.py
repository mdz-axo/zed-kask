#!/usr/bin/env python3
"""Fix Issue 3: wire {{ task }} into the primary input of 45 skills.

For each skill, the primary required input is mapped from {{ task }} in step 1's
input_mapping (with a fallback to the explicit input if the caller passed it
directly). The primary input is declared required: false (because validate_inputs
runs before input_mapping applies, and `task` is a system key). After wiring,
enforce_inputs: true is added to the manifest header.

The template empty-input validation is added separately (per-skill) — this script
only does the manifest-side wiring.

Idempotent: re-running on already-fixed files is a no-op.
"""
import re
import sys
import yaml
from pathlib import Path

REGISTRY = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry")
MANIFESTS_DIR = REGISTRY / "manifests"

# Primary input for each skill — the one that semantically matches what `task`
# (the user's natural-language request) provides. For single-required-input
# skills this is obvious; for multi-required-input skills, the primary is the
# one representing the user's intent/request (the others are secondary params
# that the caller may pass explicitly or that have sensible defaults).
PRIMARY_INPUT = {
    "adversarial-red-team": "target_output",
    "bug-hunt": "target",
    "capabilities-reasoner": "target_system",
    "caveman": "draft_response",
    "code-review": "change_spec",
    "constraint-forces-recast": "seed_concepts",
    "create-skill": "user_description",
    "deep-module": "module_path",
    "diagnose": "bug_description",
    "diataxis-diagram": "target",
    "eqm-improvement": "rationale",
    "eqm": "rationales",
    "essentialist": "artifact",
    "falsifiability": "target",
    "goal-analysis": "user_intent",
    "gpa-evolution": "target_artifact",
    "gradient-hunter": "target_region",
    "gradient-seeded-recombination": "ontology_registry",
    "graph-audit": "mode",
    "grill-me": "topic",
    "hypothesis-framer": "broad_topic",
    "idiomatic-lisp": "design_problem",
    "idiomatic-rust": "design_problem",
    "kali-audit": "target_surface",
    "kata-coaching": "challenge",
    "lean-prover": "proposition",
    "listening": "transcript",
    "logo-builder": "name",
    "mcda": "decision_question",
    "metacognition": "goal",
    "pragmatic-cybernetics": "loop_description",
    "pragmatic-semantics": "statement",
    "proptest": "target",
    "refactor-architecture": "focus_area",
    "runtime-posture-monitor": "userpod_host",
    "scenario-builder": "focal_question",
    "self-improvement": "challenge",
    "sequential-inquiry": "problem",
    "skill-bundler": "user_intent",
    "structured-extraction": "source_text",
    "superforecasting": "forecasting_question",
    "supply-chain-sentinel": "manifest_path",
    "ui-layout-discipline": "target",
    "wardley-mapper": "target_system",
    # Skills added in the second pass (were in the Issue 3 list but initially missed).
    "coding-guidelines": "task_description",
    "prompt-enhance": "prompt",
    "sankey-flow": "prompt",
    "skill-router": "task_description",
    "tdd": "task_description",
    # upstream-rebase already maps {{ task }} directly via its `task` input
    # (a system key declared as required). Its other required input, target_file,
    # is a file path the caller must provide explicitly — not mappable from task.
    # Handled in the Issue 2 pass (enforce_inputs only, no task wiring needed).
}

# swarm-compose-guide has a different shape (action/surface/mode are all
# required and none maps cleanly from task). It's already enforce_inputs:true
# per the audit (Issue 2 list doesn't include it). Skip it for task wiring —
# its inputs are programmatic, not natural-language.
SKIP_TASK_WIRING = {"swarm-compose-guide"}


def fix_manifest(manifest_path, primary_input):
    """Wire {{ task }} into the primary input. Returns (new_text, changes)."""
    with open(manifest_path) as f:
        text = f.read()
    data = yaml.safe_load(text)
    if data is None:
        return text, 0

    lines = text.splitlines(keepends=True)
    changes = []

    # 1. Add enforce_inputs: true to the manifest header if not present.
    header = data.get("manifest", {}) or {}
    if header.get("enforce_inputs") is not True:
        # Find the manifest block and add enforce_inputs: true after the
        # last existing key in the manifest block (before the next top-level key).
        # We insert it right after the `visibility:` line (or last field) in
        # the manifest block.
        in_manifest = False
        manifest_last_line = None
        for i, line in enumerate(lines):
            stripped = line.rstrip()
            if re.match(r"^manifest:\s*$", stripped):
                in_manifest = True
                continue
            if in_manifest:
                # A top-level key (no indent, not blank, not comment) ends the block
                if stripped and not line.startswith(" ") and not line.startswith("\t"):
                    break
                # Track the last indented key line in the manifest block
                if stripped and (line.startswith(" ") or line.startswith("\t")):
                    if not stripped.lstrip().startswith("#"):
                        manifest_last_line = i
        if manifest_last_line is not None:
            # Insert enforce_inputs: true after the last manifest key.
            # Use 2-space indent to match the manifest block style.
            indent = "  "
            lines.insert(manifest_last_line + 1, f"{indent}enforce_inputs: true\n")
            changes.append("enforce_inputs: true")

    # 2. Set the primary input's required: false.
    # Find the inputs block and the primary input entry.
    inputs = data.get("inputs", []) or []
    primary_idx = None
    for idx, inp in enumerate(inputs):
        if isinstance(inp, dict) and inp.get("name") == primary_input:
            primary_idx = idx
            break
    if primary_idx is not None and inputs[primary_idx].get("required") is True:
        # Find the `required: true` line for the primary input and change to false.
        # We locate the primary input's entry by scanning for `- name: <primary>`
        # then the next `required: true` within that entry.
        in_primary = False
        for i, line in enumerate(lines):
            stripped = line.rstrip()
            # Detect the start of an input entry
            m = re.match(r"^(\s*)-\s*name:\s*(\w+)\s*$", stripped)
            if m and m.group(2) == primary_input:
                in_primary = True
                continue
            if in_primary:
                # Next entry starts with `- name:` → end of primary entry
                if re.match(r"^(\s*)-\s*name:\s*\w+\s*$", stripped):
                    in_primary = False
                    continue
                # A top-level key ends the inputs block
                if stripped and not line.startswith(" ") and not line.startswith("\t"):
                    in_primary = False
                    continue
                # Look for required: true
                rm = re.match(r"^(\s*)required:\s*true\s*$", stripped)
                if rm:
                    indent = rm.group(1)
                    lines[i] = f"{indent}required: false\n"
                    changes.append(f"{primary_input}: required false")

    # 3. Map the primary input from {{ task }} in step 1's input_mapping.
    # Find step 1 (first step with action: select) and its input_mapping.
    # Pattern: in step 1's input_mapping, add/replace the primary input line
    # with `primary_input: "{{ task | default(<primary_input>) }}"`.
    # This lets a caller pass the input explicitly to override the task mapping.
    steps = data.get("steps", []) or []
    step1 = None
    for s in steps:
        if s.get("ordinal") == 1 or (s.get("ordinal") == 0 and step1 is None):
            step1 = s
            if s.get("action") == "select":
                break
    if step1 is None or step1.get("action") != "select":
        return "".join(lines), len(changes)

    # Find step 1's input_mapping block and the primary input key within it.
    # Walk lines to find ordinal: 1, then the input_mapping: line, then the
    # primary input key line (or insert it).
    current_ordinal = None
    in_step1_mapping = False
    mapping_indent = None
    primary_in_mapping = False
    mapping_start_line = None
    mapping_keys = {}

    for i, line in enumerate(lines):
        stripped = line.rstrip()
        m = re.match(r"^(\s*)-\s*ordinal:\s*(\d+)\s*$", stripped)
        if m:
            current_ordinal = int(m.group(2))
            in_step1_mapping = False
            continue
        if current_ordinal == 1:
            mm = re.match(r"^(\s*)input_mapping:\s*$", stripped)
            if mm:
                in_step1_mapping = True
                mapping_indent = mm.group(1) + "  "  # keys are indented 2 more
                mapping_start_line = i
                continue
            if in_step1_mapping:
                # A key line: `  key: value`
                km = re.match(r"^(\s+)(\w+):\s*(.*)$", stripped)
                if km:
                    key = km.group(2)
                    if key == primary_input:
                        primary_in_mapping = True
                        # Replace this line with the task mapping
                        lines[i] = f'{mapping_indent}{primary_input}: "{{{{ task | default({primary_input}) }}}}\"\n'
                        changes.append(f"step1 mapping: {primary_input} <- task")
                    mapping_keys[key] = i
                else:
                    # End of input_mapping (less-indented or blank or next step)
                    if stripped and not line.startswith(mapping_indent[:-2]):
                        in_step1_mapping = False

    if not primary_in_mapping and mapping_start_line is not None:
        # Insert the primary input mapping right after the input_mapping: line.
        lines.insert(mapping_start_line + 1, f'{mapping_indent}{primary_input}: "{{{{ task | default({primary_input}) }}}}\"\n')
        changes.append(f"step1 mapping: {primary_input} <- task (inserted)")

    return "".join(lines), len(changes)


def main():
    total = 0
    files_changed = 0
    for skill, primary in sorted(PRIMARY_INPUT.items()):
        if skill in SKIP_TASK_WIRING:
            continue
        path = MANIFESTS_DIR / f"{skill}.yaml"
        if not path.exists():
            print(f"SKIP {skill}: manifest not found", file=sys.stderr)
            continue
        with open(path) as f:
            original = f.read()
        new_text, n = fix_manifest(path, primary)
        if new_text != original:
            with open(path, "w") as f:
                f.write(new_text)
            files_changed += 1
            total += n
            print(f"  {skill}: primary={primary}, +{n} change(s)")
        else:
            print(f"  {skill}: no changes (already wired?)")

    print(f"\nTotal: {files_changed} files changed, {total} changes")


if __name__ == "__main__":
    main()
