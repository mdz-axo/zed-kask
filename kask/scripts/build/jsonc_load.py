"""JSONC-aware loader for the zed-kask installer.

Zed writes settings.json as JSONC (JSON with ``//`` line comments and
``/* */`` block comments). The standard library ``json`` module rejects
comments, so the installer cannot round-trip a Zed-managed settings file
without first stripping them.

This module exposes ``load_jsonc`` and ``loads_jsonc`` which strip comments
*outside* string literals (so ``"https://example.com"`` is preserved) and
trailing commas (``,]`` or ``,}``) — both are JSONC features Zed tolerates
in settings.json — and then delegate to ``json``.

Used by kask/scripts/build/install-common.sh via ``python3 -c`` so the
installer keeps its no-external-Python-deps property.
"""

from __future__ import annotations

import json
from typing import Any


def _strip_comments(text: str) -> str:
    """Remove ``//`` line and ``/* */`` block comments outside strings.

    Walks the text tracking whether the current position is inside a
    string literal. Handles escaped quotes (``\\"``) and the JSON escape
    of a literal backslash (``\\\\``). Block comments spanning newlines
    are replaced with a single newline so line numbers in parse errors
    stay meaningful.
    """
    out: list[str] = []
    i = 0
    n = len(text)
    in_string = False
    while i < n:
        ch = text[i]

        if in_string:
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                # Preserve the escaped character verbatim (handles \\", \\\\, etc.).
                out.append(text[i + 1])
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue

        # Not in a string.
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue

        if ch == "/" and i + 1 < n:
            nxt = text[i + 1]
            if nxt == "/":
                # Line comment: skip to end of line (do not consume the newline).
                end = text.find("\n", i)
                if end == -1:
                    i = n
                else:
                    i = end
                continue
            if nxt == "*":
                # Block comment: skip to */. Replace with a newline if the
                # comment spanned a line, so downstream line numbers stay sane.
                end = text.find("*/", i + 2)
                if end == -1:
                    raise ValueError("Unterminated block comment in JSONC input")
                block = text[i : end + 2]
                if "\n" in block:
                    out.append("\n")
                i = end + 2
                continue

        out.append(ch)
        i += 1

    return "".join(out)


def _strip_trailing_commas(text: str) -> str:
    """Remove commas that immediately precede ``]`` or ``}`` (outside strings).

    Run *after* comment stripping so commas inside removed comments don't
    confuse the scan. Walks the text tracking string state, same as the
    comment stripper.
    """
    out: list[str] = []
    i = 0
    n = len(text)
    in_string = False
    while i < n:
        ch = text[i]

        if in_string:
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(text[i + 1])
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue

        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue

        # Look ahead past whitespace for ] or } preceded by a comma.
        if ch == ",":
            j = i + 1
            while j < n and text[j] in " \t\n\r":
                j += 1
            if j < n and text[j] in "]}":
                # Drop the comma; keep the whitespace to preserve line numbers.
                i += 1
                continue

        out.append(ch)
        i += 1

    return "".join(out)


def loads_jsonc(text: str, **kwargs: Any) -> Any:
    """Parse a JSONC string (comments and trailing commas stripped first)."""
    return json.loads(_strip_trailing_commas(_strip_comments(text)), **kwargs)


def load_jsonc(fp, **kwargs: Any) -> Any:
    """Parse a JSONC file object (comments stripped first)."""
    return loads_jsonc(fp.read(), **kwargs)


if __name__ == "__main__":
    import sys

    with open(sys.argv[1]) as f:
        print(json.dumps(load_jsonc(f), indent=2))
