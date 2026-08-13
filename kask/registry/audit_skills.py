#!/usr/bin/env python3
"""Audit hKask skill manifests and templates for the 5 systemic issues."""
import os
import re
import sys
import yaml
from pathlib import Path

REGISTRY = Path("/home/mdz-axolotl/Clones/zed-kask/kask/registry")
MANIFESTS_DIR = REGISTRY / "manifests"
TEMPLATES_DIR = REGISTRY / "templates"

ALREADY_FIXED_TIMEOUTS = {"metacognition"}
ALREADY_FIXED_TASK_WIRED = {"task-breakdown"}


def load_manifests():
    manifests = {}
    for path in sorted(MANIFESTS_DIR.glob("*.yaml")):
        with open(path) as f:
            try:
                data = yaml.safe_load(f)
            except yaml.YAMLError as e:
                print(f"YAML parse error in {path.name}: {e}", file=sys.stderr)
                continue
        if data:
            manifests[path.stem] = (path, data)
    return manifests


def load_template(template_ref):
    path = TEMPLATES_DIR / f"{template_ref}.j2"
    if not path.exists():
        return None
    with open(path) as f:
        return f.read()


def extract_thinking_budget(template_text):
    if not template_text:
        return None
    # Templates use Jinja2-style assignment: thinking_budget = "full"
    # (with `=` and optional quotes), not YAML-style `thinking_budget:`.
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


def extract_contract_inputs(template_text):
    if not template_text:
        return set()
    keys = set()
    # Contract input format is `key: type` under `contract:\n  input:`:
    #   contract:
    #     input:
    #       target: object
    #       quality_criteria: string
    m = re.search(
        r'contract:\s*\n\s*input:\s*\n((?:\s*\w+\s*:\s*\w+\s*\n)+)',
        template_text,
    )
    if m:
        for line in m.group(1).splitlines():
            km = re.match(r'\s*(\w+)\s*:', line)
            if km:
                keys.add(km.group(1))
    # Also try `inputs:` directly (key: type form)
    m = re.search(
        r'^inputs:\s*\n((?:\s*\w+\s*:\s*\w+\s*\n)+)',
        template_text,
        re.MULTILINE,
    )
    if m:
        for line in m.group(1).splitlines():
            km = re.match(r'\s*(\w+)\s*:', line)
            if km:
                keys.add(km.group(1))
    return keys


def audit_issue_1(manifests):
    findings = []
    for name, (path, data) in manifests.items():
        if name in ALREADY_FIXED_TIMEOUTS:
            continue
        steps = data.get("steps", []) or []
        for step in steps:
            if step.get("action") != "select":
                continue
            timeout = step.get("timeout_seconds")
            if timeout is None:
                continue
            template_ref = step.get("template_ref")
            if not template_ref:
                continue
            template_text = load_template(template_ref)
            tb = extract_thinking_budget(template_text)
            mt = extract_max_tokens(template_text)
            thinking_enabled = tb in (None, "full", "on", "medium")
            if not thinking_enabled:
                continue
            if mt is not None and mt >= 6000:
                required = 180
            elif mt is not None and mt >= 3000:
                required = 150
            else:
                required = 120
            if timeout < required:
                findings.append({
                    "skill": name,
                    "step": step.get("ordinal"),
                    "template_ref": template_ref,
                    "thinking_budget": tb,
                    "max_tokens": mt,
                    "current_timeout": timeout,
                    "required_timeout": required,
                })
    return findings


def audit_issue_2(manifests):
    findings = []
    for name, (path, data) in manifests.items():
        header = data.get("manifest", {}) or {}
        enforce = header.get("enforce_inputs")
        inputs = data.get("inputs", []) or []
        has_required = any(
            (i.get("required") is True) for i in inputs if isinstance(i, dict)
        )
        if has_required and enforce is not True:
            findings.append({"skill": name, "enforce_inputs": enforce, "required_input_count": sum(1 for i in inputs if isinstance(i, dict) and i.get("required") is True)})
    return findings


def audit_issue_3(manifests):
    findings = []
    for name, (path, data) in manifests.items():
        if name in ALREADY_FIXED_TASK_WIRED:
            continue
        inputs = data.get("inputs", []) or []
        has_required = any(
            (i.get("required") is True) for i in inputs if isinstance(i, dict)
        )
        if not has_required:
            continue
        steps = data.get("steps", []) or []
        uses_task = False
        for step in steps:
            mapping = step.get("input_mapping", {}) or {}
            for v in mapping.values():
                if isinstance(v, str) and "{{ task" in v:
                    uses_task = True
                    break
            if uses_task:
                break
        if not uses_task:
            required_inputs = [i.get("name") for i in inputs if isinstance(i, dict) and i.get("required") is True]
            findings.append({"skill": name, "required_inputs": required_inputs})
    return findings


def audit_issue_4(manifests):
    """Issue 4: manifests with on_timeout: retry.

    NOTE: As of the Issue 4 Option B follow-up, retry IS implemented
    (StepMachine::dispatch_with_retry). on_timeout: retry is now the desired
    default — it is no longer dead config. This audit entry is retained for
    historical visibility but the count is now informational, not a defect.
    """
    findings = []
    for name, (path, data) in manifests.items():
        eh = data.get("error_handling", {}) or {}
        on_timeout = eh.get("on_timeout")
        if on_timeout == "retry":
            findings.append({"skill": name, "on_timeout": on_timeout, "max_retries": eh.get("max_retries")})
    return findings


def audit_issue_5(manifests):
    mapping_has_not_contract = []
    contract_has_not_mapping = []
    for name, (path, data) in manifests.items():
        steps = data.get("steps", []) or []
        for step in steps:
            if step.get("action") != "select":
                continue
            template_ref = step.get("template_ref")
            if not template_ref:
                continue
            mapping = step.get("input_mapping", {}) or {}
            mapping_keys = set(mapping.keys())
            template_text = load_template(template_ref)
            contract_keys = extract_contract_inputs(template_text)
            if not contract_keys:
                continue
            extra = mapping_keys - contract_keys
            if extra:
                mapping_has_not_contract.append({
                    "skill": name, "step": step.get("ordinal"),
                    "template_ref": template_ref,
                    "extra_keys": sorted(extra),
                })
            missing = contract_keys - mapping_keys
            if missing:
                contract_has_not_mapping.append({
                    "skill": name, "step": step.get("ordinal"),
                    "template_ref": template_ref,
                    "missing_keys": sorted(missing),
                })
    return mapping_has_not_contract, contract_has_not_mapping


def main():
    manifests = load_manifests()
    print(f"Loaded {len(manifests)} manifests\n")

    print("=" * 70)
    print("ISSUE 1: action:select steps with thinking enabled but timeout < 120")
    print("=" * 70)
    i1 = audit_issue_1(manifests)
    print(f"Count: {len(i1)} steps across {len(set(f['skill'] for f in i1))} skills\n")
    for f in i1:
        print(f"  {f['skill']} step {f['step']} ({f['template_ref']}): "
              f"tb={f['thinking_budget']} mt={f['max_tokens']} "
              f"timeout={f['current_timeout']} -> needs {f['required_timeout']}")

    print()
    print("=" * 70)
    print("ISSUE 2: required inputs declared but enforce_inputs not true")
    print("=" * 70)
    i2 = audit_issue_2(manifests)
    print(f"Count: {len(i2)} skills\n")
    for f in i2:
        print(f"  {f['skill']}: enforce_inputs={f['enforce_inputs']}, "
              f"required_inputs={f['required_input_count']}")

    print()
    print("=" * 70)
    print("ISSUE 3: required inputs declared but no {{ task }} in any input_mapping")
    print("=" * 70)
    i3 = audit_issue_3(manifests)
    print(f"Count: {len(i3)} skills\n")
    for f in i3:
        print(f"  {f['skill']}: required_inputs={f['required_inputs']}")

    print()
    print("=" * 70)
    print("ISSUE 4: on_timeout: retry (now ENFORCED — dispatch_with_retry)")
    print("=" * 70)
    i4 = audit_issue_4(manifests)
    print(f"Count: {len(i4)} skills\n")
    for f in i4:
        print(f"  {f['skill']}: on_timeout={f['on_timeout']}, max_retries={f['max_retries']}")

    print()
    print("=" * 70)
    print("ISSUE 5: input_mapping / template contract.input mismatches")
    print("=" * 70)
    mhnc, chnm = audit_issue_5(manifests)
    print(f"Mapping-has-not-contract (potential typos): {len(mhnc)}")
    for f in mhnc:
        print(f"  {f['skill']} step {f['step']} ({f['template_ref']}): {f['extra_keys']}")
    print()
    print(f"Contract-has-not-mapping (template expects, mapping doesn't provide): {len(chnm)}")
    for f in chnm:
        print(f"  {f['skill']} step {f['step']} ({f['template_ref']}): {f['missing_keys']}")


if __name__ == "__main__":
    main()
