---
title: "hkask-mcp-server — How-To: Common Server Tasks"
audience: [developers building or extending hKask MCP servers]
last_updated: 2026-08-20
version: "1.1.0"
status: "Active"
domain: "MCP"
mds_categories: [composition]
---

# hkask-mcp-server — How-To: Common Server Tasks

Procedural recipes for the recurring tasks when building or extending an
hKask MCP server. Each recipe is self-contained: copy the snippet, adapt the
names, and run. All recipes assume the framework entry points and types
re-exported from `hkask_mcp_server.rs:19-27`.

## Task index

```mermaid
flowchart LR
    A[Add a required credential] --> B[Open a database]
    B --> C[Validate tool input]
    C --> D[Classify an HTTP error]
    D --> E[Contain a caller path]
    E --> F[Validate a tool URL]
    F --> G[Tag a span with ontology]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-MCPSRV-010
verified_date: 2026-08-13
verified_against: kask/crates/hkask-mcp-server/src/server/context.rs:13-22
status: VERIFIED
-->

## How-to: Declare a required or optional credential

Use `CredentialRequirement::required` for credentials the server cannot
function without; `optional` for credentials that enable a degraded mode.
The bootstrap returns `McpError::MissingCredentials` listing every missing
required credential before the server struct is constructed
(`transport.rs:128-131`).

```rust
use hkask_mcp_server::CredentialRequirement;

let credentials = vec![
    CredentialRequirement::required("HKASK_GITHUB_TOKEN", "GitHub PAT"),
    CredentialRequirement::optional("HKASK_DB_PATH", "SQLite DB path"),
];
```

Resolution order: `resolve_credential` (keychain → env var)
(`transport.rs`). For `HKASK_DB_PASSPHRASE` specifically, the resolver
routes through `hkask_keystore::keychain::resolve_db_passphrase_string`
(`credentials.rs`).

## How-to: Open a database from the ServerContext

`ServerContext::open_database` looks up the env var you name in the
credentials map, resolves the passphrase via the keystore chain, and opens
the database. If the env var is unset, it falls back to an in-memory database
(`context.rs:151-160`).

```rust
let db = ctx.open_database("HKASK_DB_PATH")?;
```

For servers that need custom DDL (e.g. FTS5 tables), use
`open_database_with_extensions` (`context.rs:168-183`):

```rust
let db = ctx.open_database_with_extensions(
    "HKASK_DB_PATH",
    "CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(...);",
)?;
```

The DB passphrase is resolved from the credentials map first, then falls back
to `resolve_credential("HKASK_DB_PASSPHRASE")` (`context.rs:139-143`).

## How-to: Validate a tool input identifier

Use `validate_identifier` for tool names, server names, and other
alphanumeric identifiers. Allowed characters: alphanumeric, `_`, `.`, `-`, `:`
(`validation.rs:13-34`). For the common 3-line early-return pattern, use the
`validate_field!` macro (`hkask_mcp_server.rs:88-94`).

```rust
use hkask_mcp_server::{validate_identifier, McpToolError};

validate_identifier("session_id", &session_id, 256)?;
```

For filesystem paths, use `validate_path` instead — it allows legitimate
filename punctuation but rejects NUL/control characters and parent-directory
traversal (`validation.rs:43-69`).

## How-to: Classify an HTTP error response

`classify_http_error` maps an HTTP status code to a structured `McpToolError`
kind. Use it after a `reqwest` call fails so the MCP client gets a meaningful
error classification instead of a blanket `internal` (`http_helpers.rs:15-26`).

```rust
use hkask_mcp_server::classify_http_error;

let resp = client.get(url).send().await?;
if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(classify_http_error("GitHub", status, &body));
}
```

Status mapping: `401/403 → permission_denied`, `404 → not_found`,
`422 → invalid_argument`, `429 → rate_limited`, `502/503 → unavailable`,
other 5xx → `unavailable`, anything else → `internal` (`http_helpers.rs:18-26`).
The body is sanitized via `sanitize_error_body` before formatting
(`http_helpers.rs:16`).

## How-to: Contain a caller-supplied path under the project root

`contain_for_read` and `contain_for_write` canonicalize a caller-supplied
path and reject anything that escapes the process cwd (the project root).
Reads require the target to exist; writes canonicalize leniently so the
target may not exist yet (`validation.rs:280-289`).

```rust
use hkask_mcp_server::contain_for_read;

let resolved = contain_for_read(&user_path)?;
let bytes = std::fs::read(&resolved)?;
```

For a one-call read with a size cap, use `read_capped` — it combines
containment with a metadata size check before reading, defending against
CWE-200 (arbitrary file read) and CWE-400 (memory exhaustion)
(`validation.rs:296-316`). The default cap is `MAX_READ_BYTES = 32 MiB`
(`validation.rs:168`).

```rust
use hkask_mcp_server::{read_capped, MAX_READ_BYTES};

let bytes = read_capped(&user_path, MAX_READ_BYTES)?;
```

## How-to: Validate a tool URL against SSRF

For untrusted URLs (e.g. a `web_extract` tool input), use
`validate_tool_url_with_dns` — it runs sync scheme/credential/literal-IP
checks then resolves the hostname via `tokio::net::lookup_host` and rejects
if any resolved IP is loopback or private (`security.rs:240`).

```rust
use hkask_mcp_server::validate_tool_url_with_dns;

validate_tool_url_with_dns(&user_url).await?;
```

For user-curated URL lists where the user has explicitly chosen a local
address (e.g. a self-hosted RSS aggregator), use
`validate_tool_url_permissive` — it allows private IPs and loopback
(`security.rs:251`). Do NOT use the permissive variant for arbitrary
untrusted input.

```rust
use hkask_mcp_server::validate_tool_url_permissive;

validate_tool_url_permissive(&feed_url)?;
```

A TOCTOU between DNS resolution and the downstream `reqwest` connect (DNS
rebinding) remains; closing that requires a custom reqwest connector
(`security.rs:147-151`).

## How-to: Map an infrastructure or IO error to an McpToolError

Use the canonical per-variant mappers instead of
`McpToolError::internal(format!("...: {e}"))`, which mis-classifies
caller-fixable errors as Internal.

- `map_io_error` — `NotFound`/`PermissionDenied` → caller-fixable kinds,
  everything else → `internal` (`validation.rs:82-90`).
- `map_join_error` — cancellation → `unavailable`, panic → `internal`
  (`validation.rs:98-104`).
- `map_infra_error` — `NotFound` → `not_found`, DB connection failures →
  `unavailable`, lock poisoning/serialization/IO/query → `internal`
  (`validation.rs:114-129`).
- `map_memory_store_error` — wraps `map_infra_error` for HMem/Embedding
  infra variants; missing entities and centroid embeddings → `not_found`
  (`validation.rs:139-162`).

```rust
use hkask_mcp_server::map_io_error;

let file = std::fs::File::open(&resolved).map_err(|e| map_io_error(e, "open issues db"))?;
```

## How-to: Tag a Regulation span with a domain ontology concept

Use `execute_tool_semantic` with a `&'static str` concept from
`hkask-bridge-ontology` so the Regulation loop can route feedback by type
(`tool_span.rs:205-225`).

```rust
use hkask_mcp_server::execute_tool_semantic;
use hkask_bridge_ontology::pko::STEP_EXECUTION;

execute_tool_semantic(self, "record_step_execution", Some(STEP_EXECUTION), async {
    // ... business logic ...
    Ok(serde_json::json!({"recorded": true}))
}).await
```

If you pass `None`, the framework emits a `tracing::warn!` naming the tool —
the algedonic signal that a registered tool lacks an ontology anchor
(`tool_span.rs:215-222`). Add an arm to your server's `ontology_anchor` fn
rather than leaving the anchor unset.

## Source citations

| Claim | File:line |
|-------|-----------|
| `CredentialRequirement::required` / `optional` | `kask/crates/hkask-mcp-server/src/server/context.rs:32-53` |
| Bootstrap credential resolution loop | `kask/crates/hkask-mcp-server/src/server/transport.rs` |
| `resolve_credential` DB passphrase routing | `kask/crates/hkask-mcp-server/src/server/credentials.rs` |
| `ServerContext::open_database` | `kask/crates/hkask-mcp-server/src/server/context.rs:151-160` |
| `open_database_with_extensions` | `kask/crates/hkask-mcp-server/src/server/context.rs:168-183` |
| `resolve_db_credential` fallback | `kask/crates/hkask-mcp-server/src/server/context.rs:139-143` |
| `validate_identifier` allowed chars | `kask/crates/hkask-mcp-server/src/server/validation.rs:13-34` |
| `validate_path` traversal rejection | `kask/crates/hkask-mcp-server/src/server/validation.rs:43-69` |
| `validate_field!` macro | `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:88-94` |
| `classify_http_error` status mapping | `kask/crates/hkask-mcp-server/src/server/http_helpers.rs:15-26` |
| `contain_for_read` / `contain_for_write` | `kask/crates/hkask-mcp-server/src/server/validation.rs:280-289` |
| `read_capped` + `MAX_READ_BYTES` | `kask/crates/hkask-mcp-server/src/server/validation.rs:296-316`, `:168` |
| `validate_tool_url_with_dns` | `kask/crates/hkask-mcp-server/src/security.rs:240` |
| `validate_tool_url_permissive` | `kask/crates/hkask-mcp-server/src/security.rs:251` |
| TOCTOU caveat for DNS rebinding | `kask/crates/hkask-mcp-server/src/security.rs:147-151` |
| `map_io_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:82-90` |
| `map_join_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:98-104` |
| `map_infra_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:114-129` |
| `map_memory_store_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:139-162` |
| `execute_tool_semantic` ontology warn | `kask/crates/hkask-mcp-server/src/server/tool_span.rs:205-225` |
