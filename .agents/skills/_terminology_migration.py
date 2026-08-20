#!/usr/bin/env python3
"""Remove deprecated manifest-executor terminology from SKILL.md body text.

Removes: KnowAct, WordAct, RenderAct, FlowDef, flowdef, flow definition, cascade
(regarding manifest cascade), and related inline references. Removes the "Type"
column from Registry Templates tables. Preserves frontmatter, methodology
instructions, CASCADE-format/cascade_note proper names, and create-skill/
skill-maintenance migration context.
"""

import re
import sys
from pathlib import Path

SKILLS_DIR = Path(".agents/skills")

# Table row: | `filename` | Type | purpose |
# filename can be .j2, .yaml, or a path like ../foo/bar.j2
TABLE_ROW = re.compile(
    r'^(\| `[^`]+` \| )(KnowAct|WordAct|RenderAct|FlowDef)( \| )'
)


def replace_inline(text: str) -> str:
    """Apply terminology replacements to a single line of body text."""

    # --- Compound cascade terms (handle before bare 'cascade') ---
    text = re.sub(r'\bpost-cascade\b', 'post-step', text)
    text = re.sub(r'\bpre-cascade\b', 'pre-step', text)
    text = re.sub(r'\bmid-cascade\b', 'mid-process', text)
    text = re.sub(r'\bsub-cascade\b', 'sub-process', text)

    # --- flowdef / flow definition ---
    text = re.sub(r'\bflowdef\b', 'process', text)
    text = re.sub(r'\bflow definition\b', 'process definition', text, flags=re.IGNORECASE)

    # --- FlowDef ---
    text = re.sub(r'\bFlowDef steps\b', 'steps', text)
    text = re.sub(r'\bFlowDef execution\b', 'skill execution', text)
    text = re.sub(r'\bFlowDef path\b', 'skill path', text)
    text = re.sub(r'\bFlowDef\b', 'skill', text)

    # --- RenderAct ---
    # "Reference documents are `RenderAct`" → "Reference documents are rendering templates"
    text = re.sub(r'Reference documents are `RenderAct`', 'Reference documents are rendering templates', text)
    text = re.sub(r'Reference documents are RenderAct', 'Reference documents are rendering templates', text)
    # "RenderAct — " (starts a purpose description) → "Rendering template — "
    text = re.sub(r'`RenderAct —', '`Rendering template —', text)
    # "RenderAct (step" → "Rendering step (step"
    text = re.sub(r'(?<![\w`])RenderAct \(step', 'Rendering step (step', text)
    # "RenderAct —" without backtick
    text = re.sub(r'(?<![\w`])RenderAct —', 'Rendering template —', text)
    # "(RenderAct " → "(rendering template "
    text = re.sub(r'\(RenderAct\b', '(rendering template', text)
    # remaining bare RenderAct
    text = re.sub(r'`RenderAct`', '`rendering template`', text)
    text = re.sub(r'(?<![\w`])RenderAct\b', 'rendering template', text)

    # --- KnowAct / WordAct in "All templates are..." patterns ---
    text = re.sub(r'are `KnowAct` type with `Public` visibility', 'are prompt templates with `Public` visibility', text)
    text = re.sub(r'are KnowAct type with Public visibility', 'are prompt templates with Public visibility', text)
    text = re.sub(r'are `KnowAct` with `Public` visibility', 'are prompt templates with `Public` visibility', text)
    text = re.sub(r"are `KnowAct`, `Public`", 'are prompt templates with `Public` visibility', text)
    text = re.sub(r'use KnowAct with Public visibility', 'are prompt templates with Public visibility', text)
    text = re.sub(r'across all `?KnowAct`? templates', 'across all templates', text)

    # --- Specific KnowAct/WordAct contexts ---
    text = re.sub(r'A selector KnowAct', 'A selector step', text)
    text = re.sub(r'Reference/authoring KnowAct', 'Reference/authoring template', text)
    text = re.sub(r'individual WordActs', 'individual modes', text)
    text = re.sub(r'the appropriate WordAct', 'the appropriate mode', text)

    # --- Remaining KnowAct/WordAct (backticked or bare) ---
    text = re.sub(r'`KnowAct`', 'prompt template', text)
    text = re.sub(r'`WordAct`', 'prompt template', text)
    text = re.sub(r'(?<![\w`])KnowAct\b', 'prompt step', text)
    text = re.sub(r'(?<![\w`])WordAct\b', 'prompt step', text)

    # --- cascade (bare word; CASCADE-format and cascade_note are safe) ---
    # \bcascade\b won't match CASCADE (case-sensitive) or cascade_note (_ is word char)
    text = re.sub(r'\bcascade\b', 'process', text)

    return text


def process_file(path: Path) -> int:
    """Process a single SKILL.md file. Returns number of lines changed."""
    lines = path.read_text().splitlines(keepends=True)
    changed = 0
    new_lines = []

    for line in lines:
        original = line

        # Strip trailing newline for processing
        had_newline = line.endswith('\n')
        stripped = line[:-1] if had_newline else line

        # 1. Table header: | Template | Type | Purpose | → | Template | Purpose |
        if stripped == '| Template | Type | Purpose |':
            stripped = '| Template | Purpose |'
        # 2. Table separator: |----------|------|---------| → |----------|---------|
        elif stripped == '|----------|------|---------|':
            stripped = '|----------|---------|'
        # 3. Table data row with Type column
        else:
            stripped = TABLE_ROW.sub(r'\1\3', stripped)

        # 4. Inline terminology replacements (on all body lines)
        stripped = replace_inline(stripped)

        if stripped != line[:-1] if had_newline else stripped != line:
            changed += 1

        new_line = stripped + ('\n' if had_newline else '')
        new_lines.append(new_line)

    if changed > 0:
        path.write_text(''.join(new_lines))

    return changed


def main():
    total_changed = 0
    files_changed = 0

    for skill_dir in sorted(SKILLS_DIR.iterdir()):
        if not skill_dir.is_dir():
            continue
        skill_md = skill_dir / "SKILL.md"
        if not skill_md.exists():
            continue

        changed = process_file(skill_md)
        if changed > 0:
            files_changed += 1
            total_changed += changed
            print(f"  {skill_md.relative_to(SKILLS_DIR)}: {changed} lines")

    print(f"\nTotal: {files_changed} files, {total_changed} lines changed")


if __name__ == '__main__':
    main()