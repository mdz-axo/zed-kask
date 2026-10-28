---
title: "hkask-mcp-server — Reference: API Surface"
audience: [developers building or extending hKask MCP servers]
last_updated: 2026-10-19
version: "3.0.0"
status: "Active"
domain: "MCP"
mds_categories: [trust, curation]
---

# hkask-mcp-server — Reference: API Surface

Lookup reference for the current public types, functions, and macros exported by `hkask-mcp-server`. Citations use full repository-relative paths and were re-derived from the implementation on 2026-09-28.

## Module and export map

The crate declares private `security` and public `server` modules (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:10-11`). The `server` facade declares seven private implementation modules and re-exports their public APIs (`kask/crates/hkask-mcp-server/src/server.rs:22-49`).

```mermaid
classDiagram
    class crate_root {
        +run_server()
        +execute_tool()
        +mcp_server!()
    }
    class server_context {
        +credentials: HashMap
        +webid: WebID

    }
    class tool_execution {
        +ToolContext
        +execute_tool()
        +record_tool_usage()
    }
    class tool_error {
        +kind: McpErrorKind
        +message: String
        +to_json_string()
    }
    class tool_result_envelope {
        +content
        +duration_ms
        +usage
    }

    crate_root --> server_context
    crate_root --> tool_execution
    tool_execution --> tool_error
    tool_execution --> tool_result_envelope
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-MCPSRV-020
verified_date: 2026-10-19
verified_against: kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:1-28,364-464; kask/crates/hkask-mcp-server/src/server.rs:22-50; kask/crates/hkask-mcp-server/src/server/tool_execution.rs:1-149; kask/crates/hkask-mcp-server/src/server/error.rs (McpToolError, IntoCallToolResult impl)
status: VERIFIED
-->

### Root re-exports

Defined at `kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:19-35`.

| Export | Implementation |
|---|---|
| `CredentialRequirement`, `ServerContext` | `kask/crates/hkask-mcp-server/src/server/context.rs` (`CredentialRequirement`, `ServerContext`) |
| `McpError` | `kask/crates/hkask-mcp-server/src/server/error.rs` (`McpError`) |
| `ToolContext`, `execute_tool` | `kask/crates/hkask-mcp-server/src/server/tool_execution.rs:97-149` |
| `parse_env_warn`, `resolve_credential`, `resolve_db_passphrase` | `kask/crates/hkask-mcp-server/src/server/credentials.rs:8-144` |
| `run_stdio_server` | `kask/crates/hkask-mcp-server/src/server/transport.rs` (`run_stdio_server`) |
| `validate_identifier`, `validate_path` | `kask/crates/hkask-mcp-server/src/server/validation.rs:5-69` |
| `validate_tool_url_permissive`, `validate_tool_url_with_dns` | `kask/crates/hkask-mcp-server/src/security.rs:339-366` |
| `MAX_READ_BYTES`, `contain_for_read`, `contain_for_write`, `read_capped` | `kask/crates/hkask-mcp-server/src/server/validation.rs:498-500`, `kask/crates/hkask-mcp-server/src/server/validation.rs:440-442`, `kask/crates/hkask-mcp-server/src/server/validation.rs:778-799` |
| `map_infra_error`, `map_io_error`, `map_join_error`, `map_memory_store_error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:71-163` |
| `AnyJsonValue`, `find_boolean_schema_positions` | `kask/crates/hkask-types/src/tool_schema.rs` (`AnyJsonValue`, `find_boolean_schema_positions`) |

`McpToolError` and additional helpers remain available through the public `server` module (`kask/crates/hkask-mcp-server/src/server.rs:34-51`).

### `server` module re-exports

| Export | Facade location |
|---|---|
| `validate_resolved_addresses`, `validate_tool_url_literal`, `validate_tool_url_permissive`, `validate_tool_url_with_dns` | `kask/crates/hkask-mcp-server/src/server.rs:34-37` |
| `CredentialRequirement`, `ServerContext` | `kask/crates/hkask-mcp-server/src/server.rs` (`pub use context`) |
| `parse_env_warn`, `resolve_credential`, `resolve_db_passphrase` | `kask/crates/hkask-mcp-server/src/server.rs:39` |
| `McpError`, `McpToolError` | `kask/crates/hkask-mcp-server/src/server.rs:40` |
| `classify_http_error` | `kask/crates/hkask-mcp-server/src/server.rs:41` |
| `ToolContext`, `execute_tool` | `kask/crates/hkask-mcp-server/src/server.rs:42` |
| `run_stdio_server` | `kask/crates/hkask-mcp-server/src/server.rs:43` |
| `MAX_READ_BYTES`, containment/read helpers, `resolve_max_read_bytes`, `set_artifact_owner` | `kask/crates/hkask-mcp-server/src/server.rs:44-47` |
| error mappers and input validators | `kask/crates/hkask-mcp-server/src/server.rs:48-51` |

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

Delegates to `run_stdio_server`; generic bounds require an rmcp server service and a one-shot `ServerContext` factory (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:41-54`).

### `mcp_server!`

Generates a WebID-bearing server struct, constructor, and `ToolContext` implementation. With an annotate path, the generated implementation overrides `annotate_response`; without one, it uses the identity default. There is no separate context-implementation macro or compatibility layer (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:364-464`).

## Server construction types

### `CredentialRequirement`

```rust
pub struct CredentialRequirement {
    pub env_var: String,
    pub description: String,
    pub required: bool,
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/context.rs` (`CredentialRequirement`).

| Constructor | Effect | Evidence |
|---|---|---|
| `required(env_var, description)` | `required = true` | `kask/crates/hkask-mcp-server/src/server/context.rs` (`CredentialRequirement::required`) |
| `optional(env_var, description)` | `required = false` | `kask/crates/hkask-mcp-server/src/server/context.rs` (`CredentialRequirement::optional`) |


### `ServerContext`

```rust
pub struct ServerContext {
    pub credentials: HashMap<String, String>,
    pub webid: hkask_types::WebID,
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/context.rs` (`ServerContext`). The context carries resolved credentials and WebID; database opening remains server-specific.

## Tool execution

### `ToolContext`

```rust
pub trait ToolContext {
    fn annotate_response(&self, tool_name: &str, body: Value) -> Value {
        body
    }
}
```

Definition: `kask/crates/hkask-mcp-server/src/server/tool_execution.rs:93-102`. The default is an identity transform; ontology-enabled servers override it through `mcp_server!`.

### `execute_tool`

```rust
pub async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    fut: impl Future<Output = Result<Value, McpToolError>>,
) -> Result<String, McpToolError>
```

Records the start time, scopes inference-usage collection to this call, awaits the business future, and applies the server annotation only to success (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:104-149`).

Success is a JSON string containing `content`, numeric `duration_ms`, and either aggregate `usage` or explicit `null`. Failure remains the original `Err(McpToolError)` so rmcp carries native error semantics. The framework emits no duplicate child-side outcome event; governed and agent-path dispatch record outcomes at the host boundary.

## Error types

### `McpError`

Server-level enum defined at `kask/crates/hkask-mcp-server/src/server/error.rs` (`McpError`).

| Variant | Payload |
|---|---|

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

Definition and constructors: `kask/crates/hkask-mcp-server/src/server/error.rs` (`McpToolError` and its inherent impl).

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

`to_json_string()` returns `{"error": message, "kind": kind}` (`kask/crates/hkask-mcp-server/src/server/error.rs`, `McpToolError::to_json_string`). Its rmcp conversion sets `is_error`, text content, and structured content (`kask/crates/hkask-mcp-server/src/server/error.rs`, `IntoCallToolResult for McpToolError`).

## Credential APIs

| API | Behavior | Evidence |
|---|---|---|
| `resolve_credential(env_var)` | API/config values from env; shared DB passphrase via keystore resolver | `kask/crates/hkask-mcp-server/src/server/credentials.rs:8-59` |
| `resolve_db_passphrase(credentials)` | credential map, then canonical passphrase chain; typed permission failure | `kask/crates/hkask-mcp-server/src/server/credentials.rs:75-113` |
| `parse_env_warn(key, default)` | parse env value; warn and default on malformed value | `kask/crates/hkask-mcp-server/src/server/credentials.rs:106-144` |

## Validation APIs

### Identifiers and paths

| API | Behavior | Evidence |
|---|---|---|
| `validate_identifier` | non-empty, bounded, alphanumeric plus `_ . - :` | `kask/crates/hkask-mcp-server/src/server/validation.rs:5-34` |
| `validate_path` | non-empty, bounded, no control chars or parent traversal | `kask/crates/hkask-mcp-server/src/server/validation.rs:36-69` |
| `contain_for_write` | canonicalize leniently under an allowed root | `kask/crates/hkask-mcp-server/src/server/validation.rs:440-442` |
| `contain_for_read` | canonicalize existing target under an allowed root | `kask/crates/hkask-mcp-server/src/server/validation.rs:375-378` |
| `read_capped` | contain, stat, enforce cap, read | `kask/crates/hkask-mcp-server/src/server/validation.rs:778-799` |
| `set_artifact_owner` | record the running server's artifact owner once at bootstrap | `kask/crates/hkask-mcp-server/src/server/validation.rs:258-274` |
| `MAX_READ_BYTES` | 32 MiB | `kask/crates/hkask-mcp-server/src/server/validation.rs:498-500` |
| `resolve_max_read_bytes` | `HKASK_MCP_MAX_READ_BYTES`, warn/default on invalid | `kask/crates/hkask-mcp-server/src/server/validation.rs:171-201` |

Allowed roots are the process current directory, hKask data directory, and hKask artifacts directory. Reads may use the whole artifacts tree; writes are confined to the running server's own `{server}-mcp/` folder through the artifact owner recorded by `set_artifact_owner` at bootstrap (`kask/crates/hkask-mcp-server/src/server/validation.rs:258-299`).[^cwe22]

### URL safety

| API | Behavior | Evidence |
|---|---|---|
| `validate_tool_url_with_dns` | strict syntax/literal checks plus DNS address validation | `kask/crates/hkask-mcp-server/src/security.rs:339-353` |
| `validate_tool_url_permissive` | allows private and loopback addresses | `kask/crates/hkask-mcp-server/src/security.rs:356-366` |
| `validate_tool_url_literal` | strict synchronous literal-address checks, no DNS | `kask/crates/hkask-mcp-server/src/security.rs:371-383` |
| `validate_resolved_addresses` | strict policy over exact connection addresses | `kask/crates/hkask-mcp-server/src/security.rs:385-397` |

The strict policy rejects non-HTTP(S) schemes, embedded credentials, loopback, private, and unspecified destinations, including mapped/embedded IPv4 forms (`kask/crates/hkask-mcp-server/src/security.rs:65-183`, `kask/crates/hkask-mcp-server/src/security.rs:185-275`).[^cwe918]

### Error mappers

| API | Source | Evidence |
|---|---|---|
| `map_io_error` | `std::io::Error` | `kask/crates/hkask-mcp-server/src/server/validation.rs:71-90` |
| `map_join_error` | `tokio::task::JoinError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:92-104` |
| `map_infra_error` | `InfrastructureError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:106-129` |
| `map_memory_store_error` | `MemoryStoreError` | `kask/crates/hkask-mcp-server/src/server/validation.rs:131-163` |
| `classify_http_error` | HTTP status/body | `kask/crates/hkask-mcp-server/src/server/http_helpers.rs:1-27` |

## Tool-schema helpers

`AnyJsonValue` and `find_boolean_schema_positions` are re-exported from `hkask_types::tool_schema` at the crate root (`kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs:29-35`). Use `AnyJsonValue` for tool inputs that intentionally accept arbitrary JSON. Its model-facing schema is a string containing complete JSON text; serde decodes it to the inner `serde_json::Value` before the handler runs. This replaces the open-object wire format across providers, without provider-specific parsers.

For an object, send `"value": "{\"text\":\"lesson\",\"count\":7}"`; for a literal string, send `"value": "\"plain text\""`; for an array, send `"value": "[1,true]"`. JSON text `"null"` is a null value, distinct from omitting an optional argument. Bare unencoded objects and malformed JSON text are rejected. Native `render_template.context`, `lisp_eval.env`, and their delegated host forms use the same JSON-text contract but require an object document. Skill/template context examples describe semantic values: encode the outer argument once, not each nested value separately.

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
verified_date: 2026-10-23
verified_against: kask/crates/hkask-mcp-server/src/server/context.rs (CredentialRequirement, ServerContext); kask/crates/hkask-mcp-server/src/server/tool_execution.rs:1-96; kask/crates/hkask-mcp-server/src/server/validation.rs:5-169,258-381,590-611; kask/crates/hkask-mcp-server/src/security.rs:334-397
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

`required` and `optional` set the `required` field to `true` and `false`, respectively (`kask/crates/hkask-mcp-server/src/server/context.rs`, `CredentialRequirement::required` and `CredentialRequirement::optional`). Bootstrap resolves every requirement and rejects the complete missing-required set before calling the factory (`kask/crates/hkask-mcp-server/src/server/transport.rs:70-91`).

API keys resolve from environment variables. `HKASK_DB_PASSPHRASE` alone uses the dedicated hKask keystore resolver (`kask/crates/hkask-mcp-server/src/server/credentials.rs:8-59`).

### Resolve a database passphrase for server-specific persistence

For custom DDL, open with `hkask_storage::Database::open_with_extensions` directly, as the research server does. For tools that resolve the passphrase after startup, call the root-exported `resolve_db_passphrase(&ctx.credentials)`; missing configuration is `McpToolError::permission_denied` and names the env/keychain sources (`kask/crates/hkask-mcp-server/src/server/credentials.rs:75-113`).

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

The future must return `Result<serde_json::Value, McpToolError>`. `execute_tool` returns `Result<String, McpToolError>` (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:78-96`). A successful value becomes a `{"content": value, "duration_ms": …, "usage": …}` JSON string (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:90-95`). A failure remains a typed error; rmcp marks it with `is_error: true` and includes `{"error", "kind"}` as structured content (`kask/crates/hkask-mcp-server/src/server/error.rs`, `IntoCallToolResult for McpToolError`).

Use the constructor that matches the recovery action:

```rust
return Err(McpToolError::not_found("record does not exist"));
```

Constructors cover `internal`, `not_found`, `invalid_argument`, `unavailable`, `permission_denied`, `rate_limited`, and `failed_precondition` (`kask/crates/hkask-mcp-server/src/server/error.rs`, `impl McpToolError`).

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

`contain_for_read` and `contain_for_write` accept paths only under the process working directory, hKask data directory, or artifact directory after canonicalization (`kask/crates/hkask-mcp-server/src/server/validation.rs:440-442,498-500`). Reads may use the whole artifacts tree; writes are confined to the running server's own `{server}-mcp/` folder through the artifact owner recorded at bootstrap. Writes may target a not-yet-created path; reads require an existing path. `read_capped` checks metadata length before reading (`kask/crates/hkask-mcp-server/src/server/validation.rs:778-799`). The default cap is 32 MiB (`kask/crates/hkask-mcp-server/src/server/validation.rs:498-500`).[^cwe22]

If the tool needs the environment-configurable cap, use `hkask_mcp_server::server::resolve_max_read_bytes()` (`kask/crates/hkask-mcp-server/src/server/validation.rs:171-201`). It warns on zero or malformed values and falls back to `MAX_READ_BYTES`.

### Validate an untrusted URL

For a normal tool input:

```rust
use hkask_mcp_server::validate_tool_url_with_dns;

validate_tool_url_with_dns(&url).await?;
```

This root-exported helper checks scheme, embedded credentials, literal destination addresses, and all DNS-resolved addresses (`kask/crates/hkask-mcp-server/src/security.rs:185-275`, `kask/crates/hkask-mcp-server/src/security.rs:339-353`).[^cwe918]

For a user-curated local URL, opt into the permissive helper:

```rust
use hkask_mcp_server::validate_tool_url_permissive;

validate_tool_url_permissive(&feed_url)?;
```

It permits private and loopback addresses and must not gate arbitrary untrusted input (`kask/crates/hkask-mcp-server/src/security.rs:49-62`, `kask/crates/hkask-mcp-server/src/security.rs:356-366`).

A redirect policy or connect-time validating resolver can use the public `server` module:

```rust
hkask_mcp_server::server::validate_tool_url_literal(&redirect_url)?;
hkask_mcp_server::server::validate_resolved_addresses(hostname, &addresses)?;
```

The first performs synchronous strict checks without DNS; the second validates the exact resolved addresses used for connection (`kask/crates/hkask-mcp-server/src/security.rs:371-397`).

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

### Inspect tool outcome accounting

Every `execute_tool` call returns a JSON envelope on success or preserves the typed error (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:78-96`):

| Field | Value |
|---|---|
| `content` | the handler's successful value, annotated once by the `ToolContext` |
| `duration_ms` | elapsed wall-clock milliseconds |
| `usage` | isolated inference usage for the call (null when none was recorded) |

Inference usage recorded inside the handler via `record_tool_usage` is isolated per call (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:15-34`) and never mixes across concurrent calls. Identity and outcome recording happen in the caller-side runtime (host dispatch), not by a child-side guard (`kask/crates/hkask-mcp-server/src/server/tool_execution.rs:67-73`).

## See also

- [Tutorial: build your first MCP server](./tutorial.md)
- [Explanation: why the framework is narrow](./explanation.md)

---

[^cwe22]: MITRE. (n.d.). *CWE-22: Improper Limitation of a Pathname to a Restricted Directory.* <https://cwe.mitre.org/data/definitions/22.html>.
[^cwe918]: MITRE. (n.d.). *CWE-918: Server-Side Request Forgery.* <https://cwe.mitre.org/data/definitions/918.html>.
