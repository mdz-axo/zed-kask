#!/usr/bin/env python3
"""Surgical migration of skill manifest span_namespace to reg.skill.<id>.

Text-based (not yaml.safe_dump) to preserve comments, formatting, and
all sections. Only touches the `span_namespace:` line and optionally
removes the `spans:` list and adds `telemetry_namespace:`.

Idempotent: safe to run multiple times.
"""
import re
import sys
from pathlib import Path

MANIFEST_DIR = Path("registry/manifests")


def extract_skill_id(text: str) -> str | None:
    """Extract manifest.id from YAML text without full parse."""
    m = re.search(r"^  id:\s*(\S+)\s*$", text, re.MULTILINE)
    if not m:
        return None
    skill_id = m.group(1)
    # Sanitize: replace / with - (some manifests use id: mcp/... or process/...)
    return skill_id.replace("/", "-")


def migrate_manifest(path: Path) -> str:
    """Migrate a single manifest. Returns status string."""
    text = path.read_text()
    skill_id = extract_skill_id(text)
    if not skill_id:
        return "skip (no manifest.id)"

    expected_ns = f"reg.skill.{skill_id}"

    # Check if already migrated
    current = re.search(r"^(\s*)span_namespace:\s*(\S+)\s*$", text, re.MULTILINE)
    if current and current.group(2) == expected_ns:
        # Already done — check if spans: list needs removal
        if not re.search(r"^(\s*)spans:\s*$", text, re.MULTILINE):
            return "already conforming"
        # Fall through to remove spans: list

    # Replace span_namespace value
    if current:
        old_ns = current.group(2)
        indent = current.group(1)
        text = re.sub(
            r"^(\s*)span_namespace:\s*\S+\s*$",
            f"{indent}span_namespace: {expected_ns}",
            text,
            count=1,
            flags=re.MULTILINE,
        )
    else:
        # No span_namespace line — add it after ledger: if present
        ledger_match = re.search(r"^(\s*)ledger:\s*$", text, re.MULTILINE)
        if ledger_match:
            indent = ledger_match.group(1) + "  "
            text = text.replace(
                ledger_match.group(0),
                ledger_match.group(0) + f"{indent}span_namespace: {expected_ns}\n",
                1,
            )
        else:
            return "skip (no ledger section)"

    # Add telemetry_namespace if old ns was hkask.template.* and not already present
    old_ns = current.group(2) if current else ""
    if old_ns.startswith("hkask.template.") and "telemetry_namespace:" not in text:
        telemetry_ns = f"hkask.template.{skill_id}"
        # Add after span_namespace line
        text = re.sub(
            r"^(\s*)span_namespace:\s*reg\.skill\.\S+\s*$",
            f"\\1span_namespace: {expected_ns}\n\\1telemetry_namespace: {telemetry_ns}",
            text,
            count=1,
            flags=re.MULTILINE,
        )

    # Remove the spans: list (abolished — ambiguous, unused by executor)
    # The spans: list is under ledger: and looks like:
    #   spans:
    #     - reg.foo.bar
    #     - reg.baz.qux
    # We remove from "spans:" to the next non-list-line (a line that doesn't
    # start with "    - " or is blank within the list).
    spans_match = re.search(
        r"^(\s*)spans:\s*\n((?:\s*-\s.*\n)*)", text, re.MULTILINE
    )
    if spans_match:
        text = text[: spans_match.start()] + text[spans_match.end():]

    path.write_text(text)
    return f"migrated ({old_ns} → {expected_ns})"


def main():
    dry_run = "--dry-run" in sys.argv
    migrated = 0
    skipped = 0
    conforming = 0

    for path in sorted(MANIFEST_DIR.glob("*.yaml")):
        if dry_run:
            # In dry-run, don't write — just report what would happen
            text = path.read_text()
            skill_id = extract_skill_id(text)
            if not skill_id:
                print(f"SKIP (no manifest.id): {path}")
                skipped += 1
                continue
            expected_ns = f"reg.skill.{skill_id}"
            current = re.search(r"^(\s*)span_namespace:\s*(\S+)\s*$", text, re.MULTILINE)
            has_spans = bool(re.search(r"^(\s*)spans:\s*$", text, re.MULTILINE))
            if current and current.group(2) == expected_ns and not has_spans:
                conforming += 1
                continue
            old_ns = current.group(2) if current else "(none)"
            print(f"WOULD MIGRATE: {path.name}")
            print(f"  span_namespace: {old_ns} → {expected_ns}")
            if has_spans:
                print("  spans: (remove list)")
            if old_ns.startswith("hkask.template."):
                print(f"  telemetry_namespace: hkask.template.{skill_id}")
            migrated += 1
        else:
            status = migrate_manifest(path)
            if "migrated" in status:
                print(f"MIGRATED: {path.name} — {status}")
                migrated += 1
            elif "skip" in status:
                print(f"SKIP: {path.name} — {status}")
                skipped += 1
            else:
                conforming += 1

    print(f"\nSummary: {migrated} migrated, {conforming} already conforming, {skipped} skipped.")
    if dry_run:
        print("(dry-run — no changes written)")


if __name__ == "__main__":
    main()
