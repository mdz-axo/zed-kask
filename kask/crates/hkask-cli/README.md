# hkask-cli

CLI for hKask — admin, config, startup, shutdown, and the single `tui` runtime launch.

**Design rule:** CLI = admin, config, startup, shutdown. Runtime operations
(skills, bundles, templates, kata, kanban, goals, adapters, Regulation queries,
curator escalations, consolidation, style, web search) live in the TUI's
REPL slash commands or are invoked via MCP tools from within the runtime.
The CLI does not expose side-doors to MCP tools.

## Commands

| Group | Commands |
|-------|----------|
| `tui` | Launch interactive ratatui workspace (embeds REPL); `-f` for non-interactive/pipe mode |
| `init`, `onboard`, `doctor`, `settings`, `keystore` | Config |
| `daemon` (start/status/stop), `serve`, `matrix` (deploy/register/status) | Startup |
| `userpod` (register/login/logout/sessions/list/show/rename/delete/invite) | User lifecycle |
| `backup`, `git`, `export`, `repair` | Storage admin |
| , `token`, `wallet` | Admin |
| `deploy init --domain` | Remote cluster bootstrap (K3s/Hetzner) |
| `pod` (export-container, export-k8s) | Deployment artifact generation |
| `sovereignty verify` | Magna Carta structural audit |
| `mcp` (list-servers, list-tools, get-tool) | MCP inventory (read-only) |
| `adapter` | Trained adapter lifecycle (Phase 2 — pending MCP migration) |

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PROVIDER` | Database provider (`sqlite` or `postgres`) |
| `HKASK_DB_PATH` | SQLite database path |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase |

## Observability

CLI Regulation spans log the **command group only** (e.g., `backup`, `deploy`) to avoid leaking sensitive arguments such as passphrases.
