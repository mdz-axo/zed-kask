# hkask-repl

Interactive REPL for hKask — discoverable, self-documenting, alive.

## Purpose

Provides an interactive agent session with slash-command dispatch, tab completion, and fuzzy matching. The REPL is the primary interactive interface for the `kask chat` runtime.

## Design principles

- Every capability is reachable from `/help`
- Tab completion for slash commands and agent names
- Fuzzy matching on slash commands (e.g. `/model`)
- Welcome banner with the Kask amphora logo
- Categorized help so the menu is scannable

## Features

- `tui` feature gate for ratatui-based interactive console
- Regulation display for observability
- Builtin MCP server management

## Dependencies

- `hkask-cli` — command dispatch
- `hkask-tui` (optional, feature-gated) — terminal UI bridge