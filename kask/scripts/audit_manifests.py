#!/usr/bin/env python3
"""Audit all kask manifests for executor compliance.

Checks per the audit checklist:
1. Action compliance — only canonical actions
2. Template ref resolution — every template_ref resolves to a .j2 or .yaml
3. Gas budget presence and adequacy
4. rJoule budget presence (when inference is used)
5. Convergence block completeness (for skill category)
6. Sub-manifest gas/rjoule (for .yaml template_refs)
7. Category field validity
"""
import os
import sys
import yaml
import json
from pathlib import Path

ROOT = Path("/home/mdz-axolotl/Clones/zed-kask")
MANIFEST_DIR = ROOT / "kask/registry/manifests"
TEMPLATE_DIR = ROOT / "kask/registry/templates"

CANONICAL_ACTIONS = {
    "select", "populate", "compute", "execute", "feedback",
    "validate", "retrieve", "render", "flowdef",
    "loop", "choice", "abort", "escalate",
}

VALID_CATEGORIES = {"skill", "qa-script", "runtime-config", "daemon-process", "pipeline"}

# Build a set of all template files (relative paths without extension, plus with extension)
template_files = set()
template_files_with_ext = set()
for root, dirs, files in os.walk(TEMPLATE_DIR):
    for f in files:
        full = Path(root) / f
        rel = full.relative_to(TEMPLATE_DIR)
        template_files_with_ext.add(str(rel))
        # also without extension
        template_files.add(str(rel.with_suffix("")))
        template_files.add(str(rel))  # also with extension

def resolve_template_ref(ref):
    """Return resolved path if exists, else None."""
    if not ref:
        return None
    # Try as-is (with extension)
    candidates = []
    candidates.append(TEMPLATE_DIR / ref)
    if not ref.endswith(".j2") and not ref.endswith(".yaml"):
        candidates.append(TEMPLATE_DIR / (ref + ".j2"))
        candidates.append(TEMPLATE_DIR / (ref + ".yaml"))
    for c in candidates:
        if c.exists():
            return c
    return None

def find_all_manifests():
    manifests = []
    for root, dirs, files in os.walk(MANIFEST_DIR):
        for f in files:
            if f.endswith(".yaml"):
                manifests.append(Path(root) / f)
    return sorted(manifests)

def audit_manifest(path):
    issues = []  # list of (severity, check, msg)
    try:
        with open(path) as f:
            data = yaml.safe_load(f)
    except Exception as e:
        return [("ERROR", "parse", f"YAML parse failed: {e}")]

    if not isinstance(data, dict):
        return [("ERROR", "structure", "Top-level YAML is not a mapping")]

    manifest = data.get("manifest", {})
    if not isinstance(manifest, dict):
        return [("ERROR", "structure", "Missing 'manifest' block")]

    mid = manifest.get("id", path.stem)
    category = manifest.get("category")
    if category is None:
        # Back-compat: treated as skill. Flag for clarity.
        issues.append(("WARN", "category", f"manifest.category missing (defaults to 'skill')"))
    elif category not in VALID_CATEGORIES:
        issues.append(("ERROR", "category", f"manifest.category='{category}' not in {sorted(VALID_CATEGORIES)}"))

    steps = data.get("steps", [])
    if not isinstance(steps, list):
        issues.append(("ERROR", "structure", "Missing or non-list 'steps'"))
        steps = []

    # Track inference usage
    uses_inference = False
    sub_manifest_refs = []

    for i, step in enumerate(steps, 1):
        if not isinstance(step, dict):
            issues.append(("ERROR", f"step{i}", "step is not a mapping"))
            continue
        action = step.get("action")
        if action is None:
            issues.append(("ERROR", f"step{i}", "step missing 'action'"))
            continue
        if action not in CANONICAL_ACTIONS:
            issues.append(("ERROR", f"step{i}.action", f"non-canonical action: '{action}'"))
        if action == "select":
            uses_inference = True
        tref = step.get("template_ref")
        if tref:
            resolved = resolve_template_ref(tref)
            if resolved is None:
                issues.append(("ERROR", f"step{i}.template_ref", f"unresolved template_ref: '{tref}'"))
            else:
                if resolved.suffix == ".yaml":
                    sub_manifest_refs.append((i, tref, resolved))
        # If action is flowdef, template_ref must point to .yaml
        if action == "flowdef" and tref:
            resolved = resolve_template_ref(tref)
            if resolved and resolved.suffix != ".yaml":
                issues.append(("WARN", f"step{i}.flowdef", f"flowdef template_ref '{tref}' resolves to non-.yaml: {resolved.name}"))

    # Gas block
    gas = data.get("gas")
    if not isinstance(gas, dict):
        issues.append(("ERROR", "gas", "missing 'gas' block"))
    else:
        cap = gas.get("cap")
        if cap is None:
            issues.append(("ERROR", "gas.cap", "missing gas.cap"))
        elif cap == 0:
            issues.append(("ERROR", "gas.cap", "gas.cap == 0"))
        for fld in ("cost_per_iteration", "alert_threshold", "hard_limit"):
            if fld not in gas:
                issues.append(("WARN", f"gas.{fld}", f"missing gas.{fld} (will default)"))
        # Heuristic adequacy
        if cap is not None and isinstance(cap, (int, float)):
            n_steps = len(steps)
            if n_steps <= 3 and cap < 5000:
                issues.append(("WARN", "gas.cap", f"cap={cap} seems low for {n_steps}-step skill"))
            if n_steps >= 6 and cap < 50000:
                issues.append(("WARN", "gas.cap", f"cap={cap} seems low for {n_steps}-step skill"))

    # rJoule block
    rjoule = data.get("rjoule")
    if not isinstance(rjoule, dict):
        if uses_inference:
            issues.append(("ERROR", "rjoule", "uses inference (action: select) but no 'rjoule' block"))
    else:
        rcap = rjoule.get("cap")
        if rcap is None:
            if uses_inference:
                issues.append(("ERROR", "rjoule.cap", "uses inference but rjoule.cap missing"))
        elif rcap == 0 and uses_inference:
            issues.append(("ERROR", "rjoule.cap", "uses inference but rjoule.cap == 0"))
        elif isinstance(rcap, (int, float)) and rcap > 0 and not uses_inference:
            issues.append(("WARN", "rjoule.cap", f"rjoule.cap={rcap} but no inference steps"))

    # Convergence block (only for skill category)
    is_skill = category in (None, "skill")
    conv = data.get("convergence")
    if is_skill:
        if not isinstance(conv, dict):
            issues.append(("ERROR", "convergence", "skill manifest missing 'convergence' block"))
        else:
            for fld in ("threshold", "max_iterations", "convergence_field", "on_not_reached"):
                if fld not in conv:
                    issues.append(("ERROR", f"convergence.{fld}", f"missing convergence.{fld}"))
            if "min_iterations" not in conv:
                issues.append(("WARN", "convergence.min_iterations", "missing (defaults to 0)"))
            thr = conv.get("threshold")
            if isinstance(thr, (int, float)):
                if thr < 0.05 or thr > 0.30:
                    issues.append(("WARN", "convergence.threshold", f"threshold={thr} outside typical 0.05-0.30 range"))
    else:
        if isinstance(conv, dict):
            issues.append(("INFO", "convergence", f"non-skill category '{category}' has convergence block (acceptable)"))

    # Sub-manifest gas/rjoule
    for (i, tref, resolved) in sub_manifest_refs:
        try:
            with open(resolved) as f:
                sub = yaml.safe_load(f)
            if not isinstance(sub, dict):
                issues.append(("ERROR", f"step{i}.template_ref", f"sub-manifest {resolved.name} not a mapping"))
                continue
            sub_gas = sub.get("gas")
            if not isinstance(sub_gas, dict):
                issues.append(("WARN", f"step{i}.template_ref", f"sub-manifest {resolved.name} missing 'gas' block (defaults to 100000)"))
            sub_rjoule = sub.get("rjoule")
            if not isinstance(sub_rjoule, dict):
                issues.append(("WARN", f"step{i}.template_ref", f"sub-manifest {resolved.name} missing 'rjoule' block (defaults to 0)"))
            else:
                src = sub_rjoule.get("cap")
                if src == 0:
                    issues.append(("WARN", f"step{i}.template_ref", f"sub-manifest {resolved.name} rjoule.cap == 0"))
        except Exception as e:
            issues.append(("ERROR", f"step{i}.template_ref", f"failed to parse sub-manifest {resolved.name}: {e}"))

    return issues

def main():
    manifests = find_all_manifests()
    print(f"Found {len(manifests)} manifests\n")
    total_errors = 0
    total_warns = 0
    total_infos = 0
    failing = []
    for m in manifests:
        issues = audit_manifest(m)
        if not issues:
            continue
        errs = [x for x in issues if x[0] == "ERROR"]
        warns = [x for x in issues if x[0] == "WARN"]
        infos = [x for x in issues if x[0] == "INFO"]
        total_errors += len(errs)
        total_warns += len(warns)
        total_infos += len(infos)
        if errs:
            failing.append(m.name)
        print(f"### {m.name}")
        for sev, check, msg in issues:
            print(f"  [{sev}] {check}: {msg}")
        print()
    print("=" * 60)
    print(f"TOTAL: {total_errors} errors, {total_warns} warnings, {total_infos} infos")
    print(f"Failing manifests ({len(failing)}):")
    for f in failing:
        print(f"  - {f}")

if __name__ == "__main__":
    main()
