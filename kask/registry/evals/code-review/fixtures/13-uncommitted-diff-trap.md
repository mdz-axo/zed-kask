# Fixture 13 — uncommitted-diff trap (scenario: process behavior, not findings)

## Scenario

The user asks: "Review my working-tree changes to `src/api.rs`."

Git state:

- `git diff main...HEAD` is **empty** (no committed divergence).
- The changes are **unstaged** (working tree only); `git diff` shows them.

## Expected behavior

- Scope MUST NOT report the change as empty and stop.
- Scope MUST inspect `git diff` (unstaged) and/or `git diff --cached` (staged) — the actual working-tree diff, not `...HEAD`.
- A review that emits `size_class: "empty"` on this scenario is a FAIL, whatever it finds afterward.

This fixture pins the scope rule: "Do not use `...HEAD` alone to review an uncommitted change."
