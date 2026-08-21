---
title: "hkask-mcp-server — Reference: API Surface"
audience: [developers needing precise type and function signatures]
last_updated: 2026-08-13
version: "1.0.0"
status: "Active"
domain: "MCP"
mds_categories: [domain]
---

<!-- STALE — predates the hkask-mcp-server refactor (Candidates F + E). Symbols/structure cited
below that were REMOVED or CHANGED: `McpToolOutput` (inlined into `ToolSpanGuard::ok_json`), the
`tool_schema` module file (re-export inlined at the lib root), `McpToolError::timeout` constructor,
`McpToolError.details` field, `CapabilityTier::reg_available()`, and `src/server/mod.rs` (renamed to
`src/server.rs` per .rules). The SSRF wrappers `validate_tool_url_with_dns`/`validate_tool_url_permissive`
moved from `validation.rs` to `security.rs`. Line numbers have shifted — consult the source until this
doc is regenerated. -->

# hkask-mcp-server — Reference: API Surface

A lookup reference for the public types, functions, macros, and error
variants exported by `hkask-mcp-server`. Every entry cites the file:line
where it is defined so you can read the implementation directly.

## Module map

```mermaid
classDiagram
    class hkask_mcp_server {
        +run_server()
        +run_server_with_preloaded()
        +validate_field!()
        +impl_tool_context!()
        +mcp_server!()
    }
    class server_context {
        +credentials: HashMap
        +webid: WebID
        +capability_tier: CapabilityTier
        +open_database()
        +open_database_with_extensions()
    }
    class capability_tier {
        +embedded: bool
        +keystore_available: bool
        +persistence_available: bool
        +detect()
        +reg_available()
    }
    class credential_requirement {
        +env_var: String
        +description: String
        +required: bool
        +required()
        +optional()
    }
    class tool_span_guard {
        +new()
        +with_ontology()
        +ok()
        +error()
        +ok_json()
        +finish()
    }
    class mcp_error {
        <<enum>>
        DatabasePassphrase
        UnexpectedResponse
        MissingCredentials
        Storage
        Infrastructure
        Transport
    }
    class mcp_tool_error {
        +kind: McpErrorKind
        +message: String
        +to_json_string()
    }
    hkask_mcp_server --> server_context : constructs
    server_context --> capability_tier
    server_context --> credential_requirement : declares
    hkask_mcp_server --> tool_span_guard : emits via execute_tool
    tool_span_guard --> mcp_tool_error : serializes
    mcp_error --> mcp_tool_error : distinct layers
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-MCPSRV-020
verified_date: 2026-08-13
verified_against: kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:19-27
status: VERIFIED
-->

## Public re-exports

The crate root re-exports the public surface from `server/` and `tool_schema`
(`hkask_mcp_server.rs:19-31`).

| Symbol | Source |
|--------|--------|
| `CapabilityTier`, `CredentialRequirement`, `ServerContext` | `server/context.rs` |
| `McpError`, `McpToolError` | `server/error.rs` |
| `ToolContext`, `ToolSpanGuard`, `execute_tool`, `execute_tool_semantic` | `server/tool_span.rs` |
| `run_stdio_server`, `run_stdio_server_with_preloaded` | `server/transport.rs` |
| `load_dotenv`, `resolve_credential` | `server/credentials.rs` |
| `classify_http_error` | `server/http_helpers.rs` |
| `MAX_READ_BYTES`, `contain_for_read`, `contain_for_write`, `read_capped` | `server/validation.rs` |
| `map_infra_error`, `map_io_error`, `map_join_error`, `map_memory_store_error` | `server/validation.rs` |
| `validate_identifier`, `validate_path`, `validate_tool_url_permissive`, `validate_tool_url_with_dns` | `server/validation.rs` |
| `AnyJsonValue`, `find_boolean_schema_positions` | `tool_schema.rs` (re-export from `hkask_types::tool_schema`) |

## Entry points

### `run_server` — `hkask_mcp_server.rs:40-52`

```rust
pub async fn run_server<S, F>(
    name: &str,
    version: &str,
    factory: F,
    credentials: Vec<CredentialRequirement>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer> + rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
```

Canonical entry point. Delegates to `run_stdio_server`. `#[must_use]`.

### `run_server_with_preloaded` — `hkask_mcp_server.rs:56-69`

Like `run_server` but accepts a `HashMap<String, String>` of pre-resolved
`.env` credentials. Preloaded values take precedence over `resolve_credential`
(`transport.rs:54-79`).

## Server context

### `ServerContext` — `context.rs:134-142`

```rust
pub struct ServerContext {
    pub credentials: HashMap<String, String>,
    pub webid: hkask_types::WebID,
    pub capability_tier: CapabilityTier,
}
```

No ambient authority — all deps injected here (`context.rs:133`).

| Method | Signature | File:line |
|--------|-----------|-----------|
| `open_database` | `(&self, db_env_var: &str) -> Result<Database, McpError>` | `context.rs:165-174` |
| `open_database_with_extensions` | `(&self, db_env_var: &str, extensions: &str) -> Result<Database, McpError>` | `context.rs:182-200` |
| `resolve_db_credential` (private) | `(&self) -> Result<String, McpError>` | `context.rs:150-157` |

### `CapabilityTier` — `context.rs:66-74`

```rust
pub struct CapabilityTier {
    pub embedded: bool,
    pub keystore_available: bool,
    pub persistence_available: bool,
}
```

Two operating modes: **Embedded** (hKask runtime, non-anonymous WebID,
keystore reachable, persistence available, Regulation consumes spans) and
**Standalone** (IDE, anonymous WebID, keystore may be unavailable,
persistence unavailable, spans go to stderr) (`context.rs:60-65`).

| Method | Behavior | File:line |
|--------|----------|-----------|
| `detect` | `embedded` = WebID ≠ anonymous; `persistence_available` = credentials contain `HKASK_DB_PATH`; `keystore_available` = sentinel keychain probe | `context.rs:91-104` |
| `probe_keystore` (private) | Lightweight keychain read; `Ok`/`NotFound` → true, `Platform` → false | `context.rs:111-119` |
| `reg_available` | Returns `self.embedded` — Regulation spans are meaningful only in embedded mode | `context.rs:128-130` |

### `CredentialRequirement` — `context.rs:13-22`

```rust
pub struct CredentialRequirement {
    pub env_var: String,
    pub description: String,
    pub required: bool,
}
```

| Constructor | `required` value | File:line |
|--------------|------------------|-----------|
| `required(env_var, description)` | `true` | `context.rs:32-38` |
| `optional(env_var, description)` | `false` | `context.rs:47-53` |

## Tool execution

### `ToolContext` trait — `tool_span.rs:163-166`

```rust
pub trait ToolContext {
    fn webid(&self) -> &hkask_types::WebID;
}
```

Implemented for free by the `mcp_server!` macro via `impl_tool_context!`
(`hkask_mcp_server.rs:102-111`).

### `ToolSpanGuard` — `tool_span.rs:11-18`

RAII guard that emits a `reg.tool` span on drop. Fields: `tool_name`,
`start: Instant`, `caller: WebID`, `emitted: bool`, `ontology: Option<&'static str>`.

| Method | Effect | File:line |
|--------|--------|-----------|
| `new(tool_name, caller)` | Records start time | `tool_span.rs:26-34` |
| `with_ontology(concept)` | Tags span with a domain concept (builder) | `tool_span.rs:52-55` |
| `ok(output)` | Emits `ok` span, returns output | `tool_span.rs:62-74` |
| `error(kind, output)` | Emits `error` span with kind, returns output | `tool_span.rs:81-93` |
| `ok_json(value)` | `ok(McpToolOutput::new(value).to_json_string())` | `tool_span.rs:101-103` |
| `finish(result)` | `Ok` → `ok_json`, `Err` → `error(…)` | `tool_span.rs:111-116` |
| `Drop` | If not emitted, emits a `dropped` warning span | `tool_span.rs:119-134` |

### `execute_tool` — `tool_span.rs:185-193`

```rust
pub async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    fut: impl Future<Output = Result<Value, McpToolError>>,
) -> String
```

Creates a `ToolSpanGuard`, awaits `fut`, calls `span.finish(result)`. Returns
the MCP wire-format JSON string.

### `execute_tool_semantic` — `tool_span.rs:203-223`

Like `execute_tool` but accepts an `Option<&'static str>` ontology concept.
`None` emits a `tracing::warn!` naming the tool — the algedonic signal that a
registered tool lacks an ontology anchor (`tool_span.rs:212-220`).

### `emit_tool_span` (private) — `tool_span.rs:139-148`

Emits a `tracing::info!` at target `reg.tool` with `tool`, `outcome`,
`duration_ms`, `error_kind`, `caller`, `ontology` fields.

## Error types

### `McpError` — `error.rs:17-37`

Server-level failures. Replaces `anyhow::Error` in all public APIs
(`error.rs:13-15`).

| Variant | Carries | File:line |
|---------|--------|-----------|
| `DatabasePassphrase(String)` | `{0} set but HKASK_DB_PASSPHRASE missing` | `error.rs:18-19` |
| `UnexpectedResponse { context, detail }` | Unexpected downstream response | `error.rs:21-22` |
| `MissingCredentials { missing }` | Comma-joined missing env vars | `error.rs:24-27` |
| `Storage` (`#[from] DatabaseError`) | Storage-layer failure | `error.rs:29-30` |
| `Infrastructure` (`#[from] InfrastructureError`) | Infra failure | `error.rs:32-33` |
| `Transport(Box<RmcpError>)` | rmcp transport failure | `error.rs:35-36` |

`From<rmcp::RmcpError>` boxes the error (`error.rs:39-43`).

### `McpToolError` — `error.rs:48-54`

```rust
pub struct McpToolError {
    pub kind: McpErrorKind,
    pub message: String,
    details: Option<Value>,  // #[serde(default, skip_serializing_if = "Option::is_none")]
}
```

Structured tool-dispatch error with semantic classification. `kind` is
`hkask_types::McpErrorKind`.

| Constructor | `McpErrorKind` | File:line |
|-------------|---------------|-----------|
| `new(kind, message)` | given | `error.rs:63-69` |
| `internal(message)` | `Internal` | `error.rs:75-77` |
| `not_found(message)` | `NotFound` | `error.rs:83-85` |
| `invalid_argument(message)` | `InvalidArgument` | `error.rs:91-93` |
| `unavailable(message)` | `Unavailable` | `error.rs:99-101` |
| `timeout(message)` | `Timeout` | `error.rs:107-109` |
| `permission_denied(message)` | `PermissionDenied` | `error.rs:115-117` |
| `rate_limited(message)` | `RateLimited` | `error.rs:123-125` |
| `failed_precondition(message)` | `FailedPrecondition` | `error.rs:131-133` |

`to_json_string()` returns `{"error": <message>, "kind": <kind display>}`
(`error.rs:139-141`). The wire format is pinned by golden-string tests
(`server/mod.rs:106-140`).

## Validation helpers

### Identifier and path validation

| Function | Signature | File:line |
|----------|-----------|-----------|
| `validate_identifier` | `(name, value, max_len) -> Result<(), McpToolError>` | `validation.rs:13-34` |
| `validate_path` | `(name, value, max_len) -> Result<(), McpToolError>` | `validation.rs:43-69` |

### Path containment

| Function | Behavior | File:line |
|----------|----------|-----------|
| `contain_for_read(path)` | Canonicalize, reject escapes from cwd (target must exist) | `validation.rs:252-254` |
| `contain_for_write(path)` | Canonicalize leniently, reject escapes (target may not exist) | `validation.rs:245-247` |
| `read_capped(path, max_bytes)` | `contain_for_read` + size check + read | `validation.rs:261-282` |
| `MAX_READ_BYTES` | `32 * 1024 * 1024` (32 MiB) | `validation.rs:166` |

### URL validation

| Function | Behavior | File:line |
|----------|----------|-----------|
| `validate_tool_url_with_dns(url)` | Async; sync checks + DNS resolve, reject private/loopback | `validation.rs:303-307` |
| `validate_tool_url_permissive(url)` | Sync; allow private IPs and loopback | `validation.rs:321-324` |

Underlying config: `UrlValidationConfig` (`security.rs:30-50`) with
`default()` (strict) and `permissive()` (allow private + loopback,
`security.rs:42-50`).

### Error mappers

| Function | Source error | File:line |
|----------|-------------|-----------|
| `map_io_error` | `std::io::Error` | `validation.rs:82-90` |
| `map_join_error` | `tokio::task::JoinError` | `validation.rs:98-104` |
| `map_infra_error` | `InfrastructureError` | `validation.rs:114-129` |
| `map_memory_store_error` | `MemoryStoreError` | `validation.rs:139-162` |

## HTTP helpers

### `classify_http_error` — `http_helpers.rs:36-47`

```rust
pub fn classify_http_error(service: &str, status: reqwest::StatusCode, body: &str) -> McpToolError
```

Status → kind: `401/403 → permission_denied`, `404 → not_found`,
`422 → invalid_argument`, `429 → rate_limited`, `502/503 → unavailable`,
other 5xx → `unavailable`, else → `internal` (`http_helpers.rs:39-47`).
Body is sanitized via `sanitize_error_body` (`http_helpers.rs:37-38`).

### `McpToolOutput` (crate-private) — `http_helpers.rs:11-26`

Wraps a `Value` and serializes to `{"content": <value>}` for the rmcp tool
return value. Used by `ToolSpanGuard::ok_json` (`tool_span.rs:101-103`).

## Credential resolution

### `resolve_credential` — `credentials.rs:54-87`

Routes known credential names through the proper hkask keystore resolvers;
falls back to keychain lookup by env var name, then environment variable
(`credentials.rs:46-52`).

| `env_var` | Resolution | File:line |
|-----------|-----------|-----------|
| `HKASK_DB_PASSPHRASE` | `hkask_keystore::keychain::resolve_db_passphrase_string` | `credentials.rs:55-60` |
| other | `Keychain::retrieve_by_key` → `std::env::var` | `credentials.rs:61-86` |

### `load_dotenv` — `credentials.rs:18-44`

Walks up from cwd looking for the nearest `.env` file, returns its
key-value pairs without mutating the process environment. Deprecated in
favor of the OS keychain (`credentials.rs:6-9`).

## Macros

### `validate_field!` — `hkask_mcp_server.rs:84-91`

```rust
validate_field!(span, "session_id", &session_id, 256);
```

Expands to `if let Err(e) = validate_identifier(...) { return span.error(e.kind, e.to_json_string()); }`.

### `impl_tool_context!` — `hkask_mcp_server.rs:102-111`

Generates `impl ToolContext for $type { fn webid(&self) -> &WebID { &self.webid } }`.

### `mcp_server!` — `hkask_mcp_server.rs:128-181`

Generates a struct with a mandatory `webid: WebID` field plus custom fields,
a `new()` constructor, and a `ToolContext` impl. Two variants: with custom
fields (`:128-160`) and no custom fields (`:162-180`).

## Tool schema helpers

### `AnyJsonValue` and `find_boolean_schema_positions` — `tool_schema.rs:18`

Re-exported from `hkask_types::tool_schema`. The canonical implementation
lives in `hkask-types` so pure domain crates can use them without depending
on `hkask-mcp-server` (which drags in `rmcp`, `reqwest`, `hkask-keystore`,
`hkask-storage`, `tracing-subscriber`) (`tool_schema.rs:1-15`).

## Source citations

| Claim | File:line |
|-------|-----------|
| Public re-export list | `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:19-31` |
| `ServerContext` struct | `kask/crates/hkask-mcp-server/src/server/context.rs:134-142` |
| `CapabilityTier` struct + detect | `kask/crates/hkask-mcp-server/src/server/context.rs:66-104` |
| `CredentialRequirement` constructors | `kask/crates/hkask-mcp-server/src/server/context.rs:24-53` |
| `ToolSpanGuard` methods | `kask/crates/hkask-mcp-server/src/server/tool_span.rs:20-117` |
| `ToolSpanGuard::Drop` | `kask/crates/hkask-mcp-server/src/server/tool_span.rs:119-134` |
| `emit_tool_span` private | `kask/crates/hkask-mcp-server/src/server/tool_span.rs:139-148` |
| `McpError` variants | `kask/crates/hkask-mcp-server/src/server/error.rs:17-43` |
| `McpToolError` constructors | `kask/crates/hkask-mcp-server/src/server/error.rs:56-141` |
| Wire-format golden strings | `kask/crates/hkask-mcp-server/src/server/mod.rs:106-140` |
| `UrlValidationConfig` strict/permissive | `kask/crates/hkask-mcp-server/src/security.rs:30-50` |
| `AnyJsonValue` re-export rationale | `kask/crates/hkask-mcp-server/src/tool_schema.rs:1-18` |
