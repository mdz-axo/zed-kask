# hkask-mcp-regulation

MCP server for Regulation span history query tools. Exposes read-only access to the
persistent `NuEventStore` for runtime telemetry analysis.

## Architecture

```
src/
  lib.rs — Server struct, two query tools, SQLite-backed NuEventStore
```

The server opens the Regulation event database (SQLCipher via `hkask-database`) and
exposes two query tools. The stored `span_category` column holds the short name
(e.g. "guard.input", "regulation", "gas") — the `SpanNamespace::short_name()`
with the `reg.` prefix stripped. Callers pass the full `reg.*` namespace (e.g.
"reg.guard"); the server strips the `reg.` prefix before querying so the
`LIKE 'prefix%'` predicate hits the index on `(span_category, phase)`.

## Tools (2)

| Tool | Description |
|------|-------------|
| `reg_query_spans` | Query Regulation ν-event history by namespace prefix within a time window. Returns events ordered by timestamp ASC. Use `reg.guard` for guard violations, `reg.regulation` for regulation events, `hkask` for performative telemetry. |
| `reg_span_stats` | Aggregate Regulation ν-event counts by exact span_category within a namespace prefix and time window. Returns a JSON object mapping each span_category to its count, ordered by count DESC. |

## Usage

These tools are the runtime telemetry surface that the `runtime-posture-monitor`
skill consumes to observe `reg.guard.*`, `reg.regulation`, and `hkask.*`
performative spans. They are read-only — no tool writes to the event store.

## Dependencies

- `hkask-database` — SQLite/SQLCipher driver
- `hkask-storage` — `NuEventStore` for persistent Regulation event storage
- `hkask-mcp` — MCP server framework, `DaemonClient`, `McpToolError`
