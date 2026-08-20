#!/usr/bin/env python3
"""Fix empty Type cells left by the first migration pass.

Replaces | `filename` |  | purpose | → | `filename` | purpose |
"""

import re
from pathlib import Path

SKILLS_DIR = Path(".agents/skills")

# Match: | `filename` |  | purpose...
# The empty cell is " |  | " (pipe, space, space, pipe, space)
EMPTY_CELL = re.compile(r'^(\| `[^`]+` \| ) (\| )')

def main():
    total = 0
    for skill_md in sorted(SKILLS_DIR.glob("*/SKILL.md")):
        lines = skill_md.read_text().splitlines(keepends=True)
        changed = False
        new_lines = []

        for line in lines:
            had_nl = line.endswith('\n')
            stripped = line[:-1] if had_nl else line
            new = EMPTY_CELL.sub(r'\1', stripped)
            if new != stripped:
                changed = True
                total += 1
            new_lines.append(new + ('\n' if had_nl else ''))

        if changed:
            skill_md.write_text(''.join(new_lines))

    print(f"Fixed {total} lines with empty Type cells")

if __name__ == '__main__':
    main()