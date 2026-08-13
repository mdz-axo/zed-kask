#!/usr/bin/env python3
"""Measure the context-window cost of the agent system prompt and its layers.

Reports bytes and estimated tokens for each layer that occupies the context
window on every turn: the base template, the skill catalog, the overlay prompts,
the built-in tool schemas, and the MCP tool schemas.

Token estimate uses bytes/4, the standard rough heuristic for English prose in
BPE tokenizers. It over-counts for prose and under-counts for JSON schemas
(punctuation-dense text tokenizes worse), so schema figures are conservative.
Run with an exact tokenizer if a precise number matters.

Usage: python3 kask/scripts/measure-prompt-budget.py [--json]
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BYTES_PER_TOKEN = 4


def tok(n: int) -> int:
    return round(n / BYTES_PER_TOKEN)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def frontmatter_field(text: str, field: str) -> str:
    """Extract a single-line YAML frontmatter field value."""
    match = re.search(rf"^{field}:\s*(.+)$", text, re.MULTILINE)
    if not match:
        return ""
    return match.group(1).strip().strip("\"'")


def base_template() -> tuple[int, int]:
    """Static prose of the rendered prompt, minus the handlebars scaffolding.

    The template's `{{...}}` directives do not reach the model; the conditional
    bodies mostly do. This measures the raw template as an upper bound on the
    static prose, and separately the directive overhead that is stripped.
    """
    text = read(REPO / "crates/agent/src/templates/system_prompt.hbs")
    directives = sum(len(m) for m in re.findall(r"\{\{[^}]*\}\}", text))
    return len(text), directives


def skill_catalog() -> tuple[int, int, list[tuple[str, int]]]:
    """The <available_skills> block: one entry per non-hidden skill.

    Mirrors the rendering in system_prompt.hbs (name, description, location)
    including the XML wrapper indentation, so the figure matches what ships.
    """
    entries: list[tuple[str, int]] = []
    total = 0
    skills_dir = REPO / ".agents/skills"
    for skill_md in sorted(skills_dir.glob("*/SKILL.md")):
        text = read(skill_md)
        name = frontmatter_field(text, "name") or skill_md.parent.name
        desc = frontmatter_field(text, "description")
        location = str(skill_md)
        entry = (
            "  <skill>\n"
            f"    <name>{name}</name>\n"
            f"    <description>{desc}</description>\n"
            f"    <location>{location}</location>\n"
            "  </skill>\n"
        )
        entries.append((name, len(entry)))
        total += len(entry)
    # The <available_skills> open/close tags.
    total += len("<available_skills>\n</available_skills>\n")
    return total, len(entries), entries


def overlays() -> dict[str, int]:
    """Static-context overlays, extracted as string-literal byte counts.

    Each is a Rust string literal or format!; measuring the source span
    over-counts slightly (escapes, continuations) but is the honest upper bound
    without running the renderer.
    """
    out: dict[str, int] = {}

    curator = read(REPO / "crates/agent/src/curator_agent_server.rs")
    m = re.search(
        r'pub const CURATOR_STATIC_CONTEXT: &str = "(.*?)";', curator, re.DOTALL
    )
    if m:
        body = m.group(1).replace("\\n", "\n").replace("\\", "")
        out["curator"] = len(body)

    for label, rel, fn in (
        ("swarm_steer", "crates/swarm_panel/src/swarm_panel.rs", "steer_system_prompt"),
        (
            "kanban_steer",
            "crates/kanban_panel/src/kanban_panel.rs",
            "steer_system_prompt",
        ),
    ):
        text = read(REPO / rel)
        start = text.find(f"fn {fn}(")
        if start == -1:
            continue
        # Span to the closing brace of the function, found by brace balance.
        depth = 0
        seen = False
        end = start
        for i, ch in enumerate(text[start:], start):
            if ch == "{":
                depth += 1
                seen = True
            elif ch == "}":
                depth -= 1
                if seen and depth == 0:
                    end = i
                    break
        body = text[start:end]
        # Count only the quoted prose, not the Rust scaffolding. DOTALL matters:
        # these are multi-line `format!` literals using `\` line continuations,
        # so a line-anchored match truncates them to a fraction of their size.
        quoted = sum(
            len(s) for s in re.findall(r'"((?:[^"\\]|\\.)*)"', body, re.DOTALL)
        )
        out[label] = quoted

    return out


def builtin_tool_schemas() -> tuple[int, int]:
    """Built-in Zed agent tools: doc comments become the model-facing schema.

    Counts the doc comments on each `*ToolInput` struct plus its fields, which
    is what schemars renders into the tool description.
    """
    total = 0
    count = 0
    tools_dir = REPO / "crates/agent/src/tools"
    for src in sorted(tools_dir.glob("*_tool.rs")):
        text = read(src)
        if "ToolInput" not in text:
            continue
        count += 1
        # Doc comments (`///`) are the description surface.
        total += sum(len(line) for line in re.findall(r"^\s*///.*$", text, re.MULTILINE))
    return total, count


def mcp_tool_schemas() -> tuple[int, int, list[tuple[str, int, int]]]:
    """MCP server tool surfaces: #[tool] attribute + doc comment per tool.

    These are NOT part of the system prompt, but they are sent with every
    request in the `tools` array, so they consume the same context window.
    """
    per_server: list[tuple[str, int, int]] = []
    total = 0
    total_tools = 0
    servers_dir = REPO / "kask/mcp-servers"
    if not servers_dir.is_dir():
        return 0, 0, []
    for server in sorted(p for p in servers_dir.iterdir() if p.is_dir()):
        sbytes = 0
        stools = 0
        for src in server.rglob("*.rs"):
            if "/tests/" in str(src):
                continue
            text = read(src)
            # Count each #[tool(...)] and the doc comment block above it.
            for m in re.finditer(r"#\[tool\b", text):
                stools += 1
                # Walk backwards over contiguous doc-comment lines.
                head = text[: m.start()]
                lines = head.splitlines()
                j = len(lines) - 1
                while j >= 0 and lines[j].strip().startswith("///"):
                    sbytes += len(lines[j])
                    j -= 1
            # The description= strings inside tool attributes.
            for m in re.finditer(r'description\s*=\s*"((?:[^"\\]|\\.)*)"', text):
                sbytes += len(m.group(1))
        if stools:
            per_server.append((server.name, stools, sbytes))
            total += sbytes
            total_tools += stools
    return total, total_tools, per_server


def project_rules() -> int:
    rules = REPO / ".rules"
    return len(read(rules)) if rules.is_file() else 0


def main() -> int:
    tmpl_bytes, directive_bytes = base_template()
    static_prose = tmpl_bytes - directive_bytes
    catalog_bytes, skill_count, entries = skill_catalog()
    ov = overlays()
    builtin_bytes, builtin_count = builtin_tool_schemas()
    mcp_bytes, mcp_tools, per_server = mcp_tool_schemas()
    rules_bytes = project_rules()

    base_turn = static_prose + catalog_bytes + rules_bytes + builtin_bytes
    with_mcp = base_turn + mcp_bytes
    worst = with_mcp + max(ov.values(), default=0)

    if "--json" in sys.argv:
        print(
            json.dumps(
                {
                    "bytes_per_token": BYTES_PER_TOKEN,
                    "base_template_raw": tmpl_bytes,
                    "base_template_static_prose": static_prose,
                    "skill_catalog": catalog_bytes,
                    "skill_count": skill_count,
                    "overlays": ov,
                    "builtin_tool_schemas": builtin_bytes,
                    "builtin_tool_count": builtin_count,
                    "mcp_tool_schemas": mcp_bytes,
                    "mcp_tool_count": mcp_tools,
                    "project_rules": rules_bytes,
                    "totals": {
                        "no_mcp": base_turn,
                        "with_mcp": with_mcp,
                        "worst_case_with_overlay": worst,
                    },
                },
                indent=2,
            )
        )
        return 0

    def row(label: str, b: int) -> str:
        return f"{label:<44} {b:>8,} B  {tok(b):>7,} tok"

    print("=" * 72)
    print("Agent context-window budget (est. tokens = bytes / 4)")
    print("=" * 72)
    print()
    print("IN THE SYSTEM PROMPT")
    print(row("  Base template (raw .hbs)", tmpl_bytes))
    print(row("  Base template (static prose, rendered)", static_prose))
    print(row(f"  Skill catalog ({skill_count} skills)", catalog_bytes))
    print(row("  Project rules (.rules, injected)", rules_bytes))
    print()
    print("OVERLAYS (mutually exclusive; via static_context)")
    for k in ("curator", "swarm_steer", "kanban_steer"):
        if k in ov:
            print(row(f"  {k}", ov[k]))
    print()
    print("NOT IN THE PROMPT, BUT IN EVERY REQUEST")
    print(row(f"  Built-in tool schemas ({builtin_count} tools)", builtin_bytes))
    print(row(f"  MCP tool schemas ({mcp_tools} tools)", mcp_bytes))
    for name, n, b in per_server:
        print(row(f"    {name} ({n})", b))
    print()
    print("TOTALS (per-turn fixed overhead)")
    print(row("  Prompt + rules + built-in tools", base_turn))
    print(row("  + all MCP servers", with_mcp))
    print(row("  + largest overlay (worst case)", worst))
    print()
    for window, name in ((200_000, "200k"), (1_048_576, "1M (glm-5.2)")):
        pct = 100.0 * tok(worst) / window
        print(f"  Worst case as share of {name} context: {pct:.1f}%")
    print()
    print("Caveats:")
    print("  - MCP tool schemas are JSON-dense; bytes/4 UNDER-counts them.")
    print("  - `LazyToolRouter` (crates/agent/src/tool_router.rs:142) can filter MCP")
    print("    tools, but it is lazy AND fail-open: it only activates on complex or")
    print("    tool-directed messages, and returns None (all tools retained) otherwise.")
    print("    So the MCP figure above is what a SHORT message actually pays.")
    print("  - Overlay figures count Rust string literals, an upper bound: escapes and")
    print("    line continuations are included, and `{}` placeholders are unexpanded.")
    print("  - Built-in tool schemas are counted from `///` doc comments, which is what")
    print("    schemars renders; the JSON envelope adds more.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
