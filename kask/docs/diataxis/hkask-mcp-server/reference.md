---
title: "hkask-mcp-server — Reference: API Surface"
audience: [developers building or extending hKask MCP servers]
last_updated: 2026-09-28
version: "2.1.0"
status: "Active"
domain: "MCP"
mds_categories: [trust, curation]
---

# hkask-mcp-server — Reference: API Surface

Lookup reference for the current public types, functions, and macros exported by `hkask-mcp-server`. Citations use full repository-relative paths and were re-derived from the implementation on 2026-09-15.

## Module and export map

The crate declares private `security` and public `server` modules (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:10-11`). The `server` facade declares seven private implementation modules and re-exports their public APIs (`kask/crates/hkask-mcp-server/src/server.rs:22-48`).

```mermaid
classDiagram
    class crate_root {
        +run_server()
        +execute_tool()
        +mcp_server!()
        +impl_tool_context!()
        +validate_field!()
    }
    class server_context {
        +credentials: HashMap
        +webid: WebID
        +open_database()
    }
    class tool_execution {
        +ToolContext
        +execute_tool()
    }
    class tool_error {
        +kind: McpErrorKind
        +message: String
        +to_json_string()
    }
    class reg_tool_event {
        +tool
        +outcome
        +duration_ms
        +error_kind
        +caller
    }

    crate_root --> server_context
    crate_root --> tool_execution
    tool_execution --> tool_error
    tool_execution --> reg_tool_event
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-MCPSRV-020
verified_date: 2026-09-16
verified_against: kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:10-35,37-165; kask/crates/hkask-mcp-server/src/server.rs:22-48; kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-170; kask/crates/hkask-mcp-server/src/server/error.rs:44-155
status: VERIFIED
-->

### Root re-exports

Defined at `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:19-35`.

| Export | Implementation |
|---|---|
| `CapabilityTier`, `CredentialRequirement`, `ServerContext` | `kask/crates/hkask-mcp-server/src/server/context.rs:9-190` |
| `McpError` | `kask/crates/hkask-mcp-server/src/server/error.rs:11-42` |
| `ToolContext`, `execute_tool` | `kask/crates/hkask-mcp-server/src/server/tool_span.rs:123-170` |
| `parse_env_warn`, `resolve_credential`, `resolve_db_passphrase` | `kask/crates/hkask-mcp-server/src/server/credentials.rs:8-144` |
| `run_stdio_server` | `kask/crates/hkask-mcp-server/src/server/transport.rs:9-129` |
| `validate_identifier`, `validate_path` | `kask/crates/hkask-mcp-server/src/server/validation.rs:5-69` |
| `validate_tool_url_permissive`, `validate_tool_url_with_dns` | `kask/crates/hkask-mcp-server/src/security.rs:341-364` |
| `MAX_READ_BYTES`, `contain_for_read`, `contain_for_write`, `read_capped` | `kask/crates/hkask-mcp-server/src/server/validation.rs:165-169`, `kask/crates/hkask-mcp-server/src/server/validation.rs:315-328`, `kask/crates/hkask-mcp-server/src/server/validation.rs:472-498` |
| `map_infra_error`, `map_io_error`, `map_join_error`, `map_memory_store_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:71-163` |
| `AnyJsonValue`, `find_boolean_schema_positions` | `kask/crates/hkask-types/src/tool_schema.rs:55-59`, `kask/crates/hkask-types/src/tool_schema.rs:135-139` |

`McpToolError` and additional helpers remain available through the public `server` module (`kask/crates/hkask-mcp-server/src/server.rs:32-48`).

### `server` module re-exports

| Export | Facade location |
|---|---|
| `validate_resolved_addresses`, `validate_tool_url_literal`, `validate_tool_url_permissive`, `validate_tool_url_with_dns` | `kask/crates/hkask-mcp-server/src/server.rs:32-35` |
| `CapabilityTier`, `CredentialRequirement`, `ServerContext` | `kask/crates/hkask-mcp-server/src/server.rs:36` |
| `parse_env_warn`, `resolve_credential`, `resolve_db_passphrase` | `kask/crates/hkask-mcp-server/src/server.rs:37` |
| `McpError`, `McpToolError` | `kask/crates/hkask-mcp-server/src/server.rs:38` |
| `classify_http_error` | `kask/crates/hkask-mcp-server/src/server.rs:39` |
| `ToolContext`, `execute_tool` | `kask/crates/hkask-mcp-server/src/server.rs:40` |
| `run_stdio_server` | `kask/crates/hkask-mcp-server/src/server.rs:41` |
| `MAX_READ_BYTES`, containment/read helpers, `resolve_max_read_bytes` | `kask/crates/hkask-mcp-server/src/server.rs:42-44` |
| error mappers and input validators | `kask/crates/hkask-mcp-server/src/server.rs:45-48` |

## Entry points and macros

### `run_server`

```rust
pub async fn run_server<S, F>(
    name: &str,
    version: &str,
    factory: F,
    credentials: Vec<CredentialRequirement>,
) -> Result<(), McpError>
```

Delegates to `run_stdio_server`; generic bounds require an rmcp server service and a one-shot `ServerContext` factory (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:37-54`).

### `validate_field!`

Expands to `validate_identifier`; on error it returns `Err($span.error(e))` (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:56-76`). It is intended only where the caller owns a compatible span/guard expression.

### `impl_tool_context!`

Implements `ToolContext` for a type containing `webid: WebID` (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:78-96`).

### `mcp_server!`

Generates a WebID-bearing server struct, constructor, and `ToolContext` implementation. The custom-field expansion is at `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:115-145`; the fieldless expansion is at `:147-165`.

## Server construction types

### `CredentialRequirement`

```rust
pub struct CredentialRequirement {
    pub env_var: String,
    pub description: String,
    pub required: bool,
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/context.rs:9-22`.

| Constructor | Effect | Evidence |
|---|---|---|
| `required(env_var, description)` | `required = true` | `kask/crates/hkask-mcp-server/src/server/context.rs:24-38` |
| `optional(env_var, description)` | `required = false` | `kask/crates/hkask-mcp-server/src/server/context.rs:40-53` |

### `CapabilityTier`

```rust
pub struct CapabilityTier {
    pub embedded: bool,
    pub keystore_available: bool,
    pub persistence_available: bool,
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/context.rs:56-74`.

`detect(webid, resolved_credentials)` sets `embedded` from comparison with the anonymous persona, `persistence_available` from the presence of `HKASK_DB_PASSPHRASE`, and `keystore_available` from a keychain probe (`kask/crates/hkask-mcp-server/src/server/context.rs:76-123`).

### `ServerContext`

```rust
pub struct ServerContext {
    pub credentials: HashMap<String, String>,
    pub webid: hkask_types::WebID,
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/context.rs:126-135`.

| Method | Return | Evidence |
|---|---|---|
| `open_database(db_env_var)` | persistent database or in-memory fallback | `kask/crates/hkask-mcp-server/src/server/context.rs:149-164` |

## Tool execution

### `ToolContext`

```rust
pub trait ToolContext {
    fn webid(&self) -> &hkask_types::WebID;
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/tool_span.rs:123-143`.

### `execute_tool`

```rust
pub async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    fut: impl Future<Output = Result<Value, McpToolError>>,
) -> Result<String, McpToolError>
```

Creates a private `ToolSpanGuard`, awaits the future, and finishes the guard with the typed result (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:145-170`).

Success is a JSON string containing the MCP `{"content": value}` envelope (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:62-74`). Failure remains `Err(McpToolError)`.

### `reg.tool` event

`emit_tool_span` emits an info event at target `reg.tool`, message `REG` (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-119`).

| Field | Type/values |
|---|---|
| `tool` | string |
| `outcome` | `ok`, `error`, `dropped` |
| `duration_ms` | `u64` |
| `error_kind` | `McpErrorKind` display string or empty string |
| `caller` | WebID display string or empty string |

`ok` and `error` emission paths are at `kask/crates/hkask-mcp-server/src/server/tool_span.rs:32-60`; the `dropped` path is at `:92-106`. The child event is stderr observability, while production outcome recording occurs in client dispatch paths (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:123-139`).

## Error types

### `McpError`

Server-level enum defined at `kask/crates/hkask-mcp-server/src/server/error.rs:11-42`.

| Variant | Payload |
|---|---|
| `DatabasePassphrase` | `String` |
| `UnexpectedResponse` | `{ context: String, detail: String }` |
| `MissingCredentials` | `{ missing: String }` |
| `Storage` | `hkask_storage::DatabaseError` |
| `Infrastructure` | `hkask_types::InfrastructureError` |
| `Transport` | boxed `rmcp::RmcpError` |

### `McpToolError`

```rust
pub struct McpToolError {
    pub kind: McpErrorKind,
    pub message: String,
}
```

Definition and constructors: `kask/crates/hkask-mcp-server/src/server/error.rs:44-127`.

| Constructor | Kind |
|---|---|
| `new` | caller-supplied |
| `internal` | `Internal` |
| `not_found` | `NotFound` |
| `invalid_argument` | `InvalidArgument` |
| `unavailable` | `Unavailable` |
| `permission_denied` | `PermissionDenied` |
| `rate_limited` | `RateLimited` |
| `failed_precondition` | `FailedPrecondition` |

`to_json_string()` returns `{"error": message, "kind": kind}` (`kask/crates/hkask-mcp-server/src/server/error.rs:122-127`). Its rmcp conversion sets `is_error`, text content, and structured content (`kask/crates/hkask-mcp-server/src/server/error.rs:130-155`).

## Credential APIs

| API | Behavior | Evidence |
|---|---|---|
| `resolve_credential(env_var)` | API/config values from env; shared DB passphrase via keystore resolver | `kask/crates/hkask-mcp-server/src/server/credentials.rs:8-59` |
| `resolve_db_passphrase(credentials)` | credential map, then canonical passphrase chain; typed permission failure | `kask/crates/hkask-mcp-server/src/server/credentials.rs:61-104` |
| `parse_env_warn(key, default)` | parse env value; warn and default on malformed value | `kask/crates/hkask-mcp-server/src/server/credentials.rs:106-144` |

## Validation APIs

### Identifiers and paths

| API | Behavior | Evidence |
|---|---|---|
| `validate_identifier` | non-empty, bounded, alphanumeric plus `_ . - :` | `kask/crates/hkask-mcp-server/src/server/validation.rs:5-34` |
| `validate_path` | non-empty, bounded, no control chars or parent traversal | `kask/crates/hkask-mcp-server/src/server/validation.rs:36-69` |
| `contain_for_write` | canonicalize leniently under an allowed root | `kask/crates/hkask-mcp-server/src/server/validation.rs:315-321` |
| `contain_for_read` | canonicalize existing target under an allowed root | `kask/crates/hkask-mcp-server/src/server/validation.rs:323-328` |
| `read_capped` | contain, stat, enforce cap, read | `kask/crates/hkask-mcp-server/src/server/validation.rs:472-498` |
| `MAX_READ_BYTES` | 32 MiB | `kask/crates/hkask-mcp-server/src/server/validation.rs:165-169` |
| `resolve_max_read_bytes` | `HKASK_MCP_MAX_READ_BYTES`, warn/default on invalid | `kask/crates/hkask-mcp-server/src/server/validation.rs:171-201` |

Allowed roots are the process current directory, hKask data directory, and hKask artifacts directory (`kask/crates/hkask-mcp-server/src/server/validation.rs:253-313`).[^cwe22]

### URL safety

| API | Behavior | Evidence |
|---|---|---|
| `validate_tool_url_with_dns` | strict syntax/literal checks plus DNS address validation | `kask/crates/hkask-mcp-server/src/security.rs:341-354` |
| `validate_tool_url_permissive` | allows private and loopback addresses | `kask/crates/hkask-mcp-server/src/security.rs:356-364` |
| `validate_tool_url_literal` | strict synchronous literal-address checks, no DNS | `kask/crates/hkask-mcp-server/src/security.rs:366-380` |
| `validate_resolved_addresses` | strict policy over exact connection addresses | `kask/crates/hkask-mcp-server/src/security.rs:382-395` |

The strict policy rejects non-HTTP(S) schemes, embedded credentials, loopback, private, and unspecified destinations, including mapped/embedded IPv4 forms (`kask/crates/hkask-mcp-server/src/security.rs:65-183`, `kask/crates/hkask-mcp-server/src/security.rs:185-266`).[^cwe918]

### Error mappers

| API | Source | Evidence |
|---|---|---|
| `map_io_error` | `std::io::Error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:71-90` |
| `map_join_error` | `tokio::task::JoinError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:92-104` |
| `map_infra_error` | `InfrastructureError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:106-129` |
| `map_memory_store_error` | `MemoryStoreError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:131-163` |
| `classify_http_error` | HTTP status/body | `kask/crates/hkask-mcp-server/src/server/http_helpers.rs:1-27` |

## Tool-schema helpers

`AnyJsonValue` and `find_boolean_schema_positions` are re-exported from `hkask_types::tool_schema` at the crate root (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:29-35`). Use `AnyJsonValue` for tool inputs that intentionally accept arbitrary JSON.

## Procedures

Use these independent recipes when extending an hKask MCP server. Each recipe points to the current public entry point and its implementation.

### Task map

```mermaid
flowchart TD
    A[Declare credentials] --> B[Construct from ServerContext]
    B --> C[Execute a typed tool]
    C --> D[Map failures by variant]
    D --> E[Contain file paths]
    E --> F[Validate URLs]
    F --> G[Inspect reg.tool outcome fields]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-MCPSRV-010
verified_date: 2026-09-16
verified_against: kask/crates/hkask-mcp-server/src/server/context.rs:9-53,126-190; kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-170; kask/crates/hkask-mcp-server/src/server/validation.rs:5-169,253-328,472-498; kask/crates/hkask-mcp-server/src/security.rs:334-395
status: VERIFIED
-->

### Declare required and optional credentials

```rust
use hkask_mcp_server::CredentialRequirement;

let requirements = vec![
    CredentialRequirement::required("SERVICE_API_KEY", "Service API key"),
    CredentialRequirement::optional("SERVICE_CACHE_PATH", "Persistent cache path"),
];
```

`required` and `optional` set the `required` field to `true` and `false`, respectively (`kask/crates/hkask-mcp-server/src/server/context.rs:24-53`). Bootstrap resolves every requirement and rejects the complete missing-required set before calling the factory (`kask/crates/hkask-mcp-server/src/server/transport.rs:68-89`).

API keys resolve from environment variables. `HKASK_DB_PASSPHRASE` alone uses the dedicated hKask keystore resolver (`kask/crates/hkask-mcp-server/src/server/credentials.rs:8-59`).

### Open a database from `ServerContext`

```rust
let database = ctx.open_database("SERVICE_DB_PATH")?;
```

When the named path exists in `ctx.credentials`, the helper resolves the shared passphrase and opens that database. When the path is absent, it opens an in-memory database (`kask/crates/hkask-mcp-server/src/server/context.rs:137-164`).

For custom DDL, open with `hkask_storage::Database::open_with_extensions` directly, as the research server does. For tools that resolve the passphrase after startup, call the root-exported `resolve_db_passphrase(&ctx.credentials)`; missing configuration is `McpToolError::permission_denied` and names the env/keychain sources (`kask/crates/hkask-mcp-server/src/server/credentials.rs:61-104`).

### Execute a tool and preserve typed errors

```rust
use hkask_mcp_server::{McpToolError, execute_tool};
use serde_json::json;

async fn status(&self) -> Result<String, McpToolError> {
    execute_tool(self, "status", async {
        Ok(json!({"ready": true}))
    })
    .await
}
```

The future must return `Result<serde_json::Value, McpToolError>`. `execute_tool` returns `Result<String, McpToolError>` (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:145-170`). A successful value becomes a `{"content": value}` JSON string (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:62-74`). A failure remains a typed error; rmcp marks it with `is_error: true` and includes `{"error", "kind"}` as structured content (`kask/crates/hkask-mcp-server/src/server/error.rs:130-155`).

Use the constructor that matches the recovery action:

```rust
return Err(McpToolError::not_found("record does not exist"));
```

Constructors cover `internal`, `not_found`, `invalid_argument`, `unavailable`, `permission_denied`, `rate_limited`, and `failed_precondition` (`kask/crates/hkask-mcp-server/src/server/error.rs:53-127`).

### Validate an identifier or path string

For identifiers:

```rust
use hkask_mcp_server::validate_identifier;

validate_identifier("session_id", &session_id, 256)?;
```

Allowed characters are alphanumeric, `_`, `.`, `-`, and `:`; empty and overlong inputs fail as `invalid_argument` (`kask/crates/hkask-mcp-server/src/server/validation.rs:5-34`).

For path syntax:

```rust
use hkask_mcp_server::validate_path;

validate_path("output", &output_path, 4096)?;
```

`validate_path` rejects empty/overlong values, control characters, and parent-directory components (`kask/crates/hkask-mcp-server/src/server/validation.rs:36-69`). Syntax validation does not replace containment.

### Contain and read caller-supplied files

```rust
use hkask_mcp_server::{MAX_READ_BYTES, contain_for_write, read_capped};

let destination = contain_for_write(&output_path)?;
let input = read_capped(&input_path, MAX_READ_BYTES)?;
```

`contain_for_read` and `contain_for_write` accept paths only under the process working directory, hKask data directory, or artifact directory after canonicalization (`kask/crates/hkask-mcp-server/src/server/validation.rs:253-328`). Writes may target a not-yet-created path; reads require an existing path. `read_capped` checks metadata length before reading (`kask/crates/hkask-mcp-server/src/server/validation.rs:472-498`). The default cap is 32 MiB (`kask/crates/hkask-mcp-server/src/server/validation.rs:165-169`).[^cwe22]

If the tool needs the environment-configurable cap, use `hkask_mcp_server::server::resolve_max_read_bytes()` (`kask/crates/hkask-mcp-server/src/server/validation.rs:171-201`). It warns on zero or malformed values and falls back to `MAX_READ_BYTES`.

### Validate an untrusted URL

For a normal tool input:

```rust
use hkask_mcp_server::validate_tool_url_with_dns;

validate_tool_url_with_dns(&url).await?;
```

This root-exported helper checks scheme, embedded credentials, literal destination addresses, and all DNS-resolved addresses (`kask/crates/hkask-mcp-server/src/security.rs:217-266`, `kask/crates/hkask-mcp-server/src/security.rs:341-354`).[^cwe918]

For a user-curated local URL, opt into the permissive helper:

```rust
use hkask_mcp_server::validate_tool_url_permissive;

validate_tool_url_permissive(&feed_url)?;
```

It permits private and loopback addresses and must not gate arbitrary untrusted input (`kask/crates/hkask-mcp-server/src/security.rs:48-62`, `kask/crates/hkask-mcp-server/src/security.rs:356-364`).

A redirect policy or connect-time validating resolver can use the public `server` module:

```rust
hkask_mcp_server::server::validate_tool_url_literal(&redirect_url)?;
hkask_mcp_server::server::validate_resolved_addresses(hostname, &addresses)?;
```

The first performs synchronous strict checks without DNS; the second validates the exact resolved addresses used for connection (`kask/crates/hkask-mcp-server/src/security.rs:366-395`).

### Classify HTTP and infrastructure errors

`classify_http_error` is available through the public `server` module:

```rust
return Err(hkask_mcp_server::server::classify_http_error(
    "service",
    status,
    &body,
));
```

It sanitizes the body and maps HTTP statuses to tool error kinds (`kask/crates/hkask-mcp-server/src/server/http_helpers.rs:1-27`).

For non-HTTP failures, use the root exports:

- `map_io_error`: file-not-found and permission failures remain caller-fixable (`kask/crates/hkask-mcp-server/src/server/validation.rs:71-90`);
- `map_join_error`: cancellation becomes `unavailable`, panic becomes `internal` (`kask/crates/hkask-mcp-server/src/server/validation.rs:92-104`);
- `map_infra_error`: not-found and database connection variants retain their categories (`kask/crates/hkask-mcp-server/src/server/validation.rs:106-129`);
- `map_memory_store_error`: memory/storage variants delegate to the same classification policy (`kask/crates/hkask-mcp-server/src/server/validation.rs:131-163`).

### Inspect `reg.tool` telemetry

Every `execute_tool` call emits one event on success or typed error. If its private guard is dropped before completion, `Drop` emits `dropped` (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:32-60`, `kask/crates/hkask-mcp-server/src/server/tool_span.rs:92-106`).

Filter tracing output for target `reg.tool` and inspect these fields:

| Field | Value |
|---|---|
| `tool` | tool name passed to `execute_tool` |
| `outcome` | `ok`, `error`, or `dropped` |
| `duration_ms` | elapsed wall-clock milliseconds |
| `error_kind` | typed `McpErrorKind` string, empty otherwise |
| `caller` | server `ToolContext.webid()` |

The event definition is `kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-119`. It is child-process observability; production Regulation outcome recording occurs in host dispatch paths (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:123-139`).

## See also

- [Tutorial: build your first MCP server](./tutorial.md)
- [Explanation: why the framework is narrow](./explanation.md)

---

[^cwe22]: MITRE. (n.d.). *CWE-22: Improper Limitation of a Pathname to a Restricted Directory.* <https://cwe.mitre.org/data/definitions/22.html>.
[^cwe918]: MITRE. (n.d.). *CWE-918: Server-Side Request Forgery.* <https://cwe.mitre.org/data/definitions/918.html>.
