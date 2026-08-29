---
title: "hkask-inference — Explanation: Why Inference Routes Through the IPC Bridge (and What the Fallbacks Are)"
audience: [architects, developers]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Inference"
mds_categories: [trust, curation]
---

# hkask-inference — Explanation: Why Inference Routes Through the IPC Bridge (and What the Fallbacks Are)

`hkask-inference` is primarily an IPC-bridge facade. MCP server child
processes route chat, vision, embedding, batch, media, tool dispatch, and
worktree spawn back to zed's `LanguageModelRegistry` over a Unix socket
(`HKASK_INFERENCE_SOCKET`), rather than holding API keys or speaking HTTP
directly. But the crate is no longer bridge-only: since the lazy-fallback
refactor, `resolve_inference_port()` (`hkask_inference.rs:94`) returns a
`LazyInferencePort` (`hkask_inference.rs:102`) that re-attempts the bridge
on **every call** and falls back to a direct-HTTP port or a standalone
media router when the socket is unavailable. This document explains why
the bridge is the primary path, why the fallbacks exist, why the stubs
are never silent, and why provider selection is prefix-based.

## Source citations

| Symbol | Location |
|--------|----------|
| `resolve_inference_port` | `kask/crates/hkask-inference/src/hkask_inference.rs:94` |
| `LazyInferencePort` | `kask/crates/hkask-inference/src/hkask_inference.rs:102` |
| `connect_bridge` | `kask/crates/hkask-inference/src/hkask_inference.rs:59` |
| `IPC_BRIDGE_UNAVAILABLE` | `kask/crates/hkask-inference/src/hkask_inference.rs:48` |
| `DirectEmbeddingPort` | `kask/crates/hkask-inference/src/hkask_inference.rs:337` |
| `DIRECT_EMBEDDING_PROVIDERS` | `kask/crates/hkask-inference/src/hkask_inference.rs:359` |
| `UnavailableToolDispatch` | `kask/crates/hkask-inference/src/hkask_inference.rs:725` |
| `UnavailableWorktreeSpawn` | `kask/crates/hkask-inference/src/hkask_inference.rs:764` |
| `InferenceIpcClient` struct | `kask/crates/hkask-inference/src/inference_ipc_client.rs:295` |
| `InferenceIpcClient::from_env` | `kask/crates/hkask-inference/src/inference_ipc_client.rs:330` |
| `ipc_roundtrip` | `kask/crates/hkask-inference/src/inference_ipc_client.rs:352` |
| `ipc_read_timeout` | `kask/crates/hkask-inference/src/inference_ipc_client.rs:147` |
| `MAX_IPC_LINE_BYTES` | `kask/crates/hkask-inference/src/inference_ipc_client.rs:74` |
| `MediaRouter` | `kask/crates/hkask-inference/src/media_router.rs:43` |
| `ProviderId::parse_from_model` | `kask/crates/hkask-inference/src/config.rs:59` |
| `resolve_api_key` | `kask/crates/hkask-inference/src/config.rs:221` |

## Startup and per-call selection state

```mermaid
stateDiagram-v2
    [*] --> Resolve: MCP server startup calls resolve_inference_port
    Resolve --> Lazy: LazyInferencePort (no connection yet)
    Lazy --> Bridge: each call retries InferenceIpcClient::from_env
    Bridge --> BridgeOk: socket reachable
    Bridge --> Fallback: socket unset/unreachable
    BridgeOk --> [*]: chat/vision/embed/batch/media via zed
    Fallback --> Direct: generate/embed -> DirectEmbeddingPort (env keys)
    Fallback --> MediaRouter: media_generate -> standalone MediaRouter
    Fallback --> NamedErr: vision/list_models/batch -> socket-named Err
    Direct --> [*]
    MediaRouter --> [*]
    NamedErr --> [*]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-INF-004
verified_date: 2026-08-28
verified_against: kask/crates/hkask-inference/src/hkask_inference.rs:94-292 (LazyInferencePort impl), :337 (DirectEmbeddingPort), :271-291 (media_generate fallback); kask/crates/hkask-inference/src/inference_ipc_client.rs:330 (from_env)
status: VERIFIED
-->

`connect_bridge(label)` (`hkask_inference.rs:59`) is the single match+log
site for the tool-dispatch and worktree-spawn resolvers: on
`Some(Ok(client))` it logs `info`, on `Some(Err(e))` it warns with the
error, and on `None` (env unset) it logs `info`. The inference resolver
does **not** use it — `LazyInferencePort` retries the bridge inside each
trait method instead (`hkask_inference.rs:172`, `:205`, `:230`, `:260`,
`:280`), which is the fix for the resolve-once-at-startup problem: a
corpus MCP server that starts before the IPC socket exists no longer
needs a restart to pick it up (doc comment, `hkask_inference.rs:86-93`).

## Why the IPC bridge is the primary path

In zed-kask, the zed process is the trust boundary for inference
credentials. It holds the API keys in its `CredentialsProvider` keychain
(`kask://credentials/<key>`) and the guard that governs tool dispatch and
worktree spawn. When zed launches an MCP server child process, it injects
the keys the child needs as environment variables and passes the socket
path via `HKASK_INFERENCE_SOCKET` so the child routes inference back to
zed's `LanguageModelRegistry`.

Routing through the bridge gives the MCP server three properties it
cannot get standalone:

1. **Credential isolation.** The child holds only the env-var keys zed
   chose to inject. `resolve_api_key` (`config.rs:221`) reads only the
   environment — it does not fall back to the `hkask` keychain namespace,
   which is reserved for sovereignty keys; the doc comment at
   `config.rs:210-217` records why the old fallback was a spec violation.
2. **Governed tool dispatch / worktree spawn.** These capabilities only
   exist on the zed side. `resolve_tool_dispatch_port`
   (`hkask_inference.rs:713`) and `resolve_worktree_spawn_port`
   (`hkask_inference.rs:753`) return the IPC-bridge client when the
   socket is available, or a stub that returns a clear error naming the
   missing socket. There is no standalone fallback for these.
3. **Unified model routing.** Chat, vision, and embedding all route
   through zed's `LanguageModelRegistry`, which resolves provider
   prefixes (`OpenRouter/`, `ollama/`, `RunPod/`) to the configured
   provider. The MCP server just sends a model name.

## Why the fallbacks exist (and their limits)

The lazy fallbacks cover exactly the standalone scenarios, and nothing
more:

- **`DirectEmbeddingPort`** (`hkask_inference.rs:337`) serves
  `generate_with_model` and `embed` only, by resolving the model's
  provider prefix against `DIRECT_EMBEDDING_PROVIDERS` (`:359` — DeepInfra,
  OpenRouter, ollama) and calling the OpenAI-compatible endpoints
  directly with env-var keys. The table deliberately mirrors
  `kask_bridge`'s `INFERENCE_PROVIDERS` static
  (`kask/crates/kask_bridge/src/inference_providers.rs:55`) — duplicated
  because `hkask-inference` cannot depend on `kask_bridge` without
  inverting the D8 seam (`hkask_inference.rs:337-340` doc comment).
- **Standalone `MediaRouter`** (`media_router.rs:43`) serves
  `media_generate` when the bridge is down — e.g. the media MCP server
  running outside zed's launch with `DEEPINFRA_API_KEY` or
  `OPENROUTER_API_KEY` set in the shell (`hkask_inference.rs:283-289`).
- **No fallback** for `generate_vision`, `list_models`, or
  `generate_batch`: they return socket-named `Connection` errors
  (`hkask_inference.rs:145-149`, `:233-235`, `:265-267`). Vision needs
  zed's multimodal providers; model listing needs zed's registry; batch
  needs the zed side to hold the provider Batch API keys
  (`inference_ipc_client.rs:443-447`).

## Why the stubs are never silent

The `Unavailable*` stubs override the trait defaults that are **not**
socket-named. `UnavailableToolDispatch` (`hkask_inference.rs:725`) and
`UnavailableWorktreeSpawn` (`:764`) return `Connection` errors naming
`IPC_BRIDGE_UNAVAILABLE` (`:48`). On the lazy port, `list_models`
(`:233-235`) returns `Err` rather than the trait default
`Ok(Vec::new())` — otherwise a broken bridge would read as an empty model
registry, the `.rules` broken-feedback-loop trap. The doc comment at
`hkask_inference.rs:293-311` records this "every method names the missing
socket" contract.

`UnavailableWorktreeSpawn` is `pub(crate)` because
`LazyLocalSwarmRuntime` names the type when it falls back to in-memory
delegation; the tool-dispatch stub is private because every call site
goes through the `Arc<dyn ToolDispatchPort>` trait object.

## Why per-request connections

The IPC client opens a **new connection per request**
(`ipc_roundtrip`, `inference_ipc_client.rs:352`, connects at `:369`). The
previous single-`Mutex<UnixStream>` design serialized all requests —
parallel embedding of large corpora was impractical even with concurrent
server-side tasks (module doc, `inference_ipc_client.rs:16-28`). Unix
domain socket `connect()` is a kernel-level operation, so the per-request
overhead is negligible against inference calls that take seconds. The
client struct (`:295`) therefore holds only the socket path and a shared
`AtomicU64` request-id counter — nothing to null or reconnect.

## Why the read deadline tracks the server

`ipc_read_timeout` (`inference_ipc_client.rs:147`) computes the client
read deadline as the server's published establishment timeout
(`HKASK_INFERENCE_TIMEOUT_SECS`) **plus 30 s grace**
(`IPC_READ_TIMEOUT_GRACE`, `:117`). If the client gave up first, it would
close its socket and the server's response write would hit `EPIPE` — a
`BrokenPipe` warn storm with two contradictory timeouts for one slow
inference. With alignment, a timed-out inference produces exactly one
timeout: the server's. Malformed or zero values fall back to 600 s
(`:127`) with a `tracing::warn!` naming the value — the `.rules`
requirement for numeric env vars.

## Why prefix-based provider selection

Provider selection is prefix-based: a caller chooses the provider by
prefixing the model name (`OpenRouter/...`, `ollama/...`, `RunPod/...`,
`DeepInfra/...`). `ProviderId::parse_from_model` (`config.rs:59`) parses
the prefix; an unprefixed name uses the default provider (OpenRouter by
default, `config.rs:157`). The prefix keeps the provider choice explicit
and auditable — a span that records the model name also records the
provider. Unrecognized prefixes are not rejected here; the model string
passes through to zed's `LanguageModelRegistry` (or, on the direct
fallback, to `DIRECT_EMBEDDING_PROVIDERS` prefix matching,
`hkask_inference.rs:371-375`), which does the actual routing.
`from_prefix_segment` (`config.rs:94`) classifies a prefix segment for
model-listing labels; it does not gate routing.

## IPC request-response sequence

```mermaid
sequenceDiagram
    participant MCP as MCP server
    participant Client as InferenceIpcClient
    participant Socket as Unix socket (fresh per request)
    participant Zed as zed LanguageModelRegistry

    MCP->>Client: generate_with_model(model, prompt, params)
    Client->>Client: next_id.fetch_add(1)
    Client->>Socket: connect + write InferenceRequest JSON + "\n"
    Socket->>Zed: dispatch to LanguageModelRegistry
    Zed-->>Socket: InferenceResponse JSON + "\n"
    Socket-->>Client: read_response_line (capped 16 MiB, server timeout + 30s)
    Client->>Client: match response.id == request.id
    Client-->>MCP: Ok(InferenceResult) | Err(InferenceError)
    Note over Client,Socket: Batch calls use a 6h+60s deadline; every outcome match is exhaustive
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-INF-005
verified_date: 2026-08-28
verified_against: kask/crates/hkask-inference/src/inference_ipc_client.rs:352-410 (ipc_roundtrip), :74 (MAX_IPC_LINE_BYTES), :117 (grace), :147 (ipc_read_timeout), :183 (batch timeout)
status: VERIFIED
-->

The protocol is newline-delimited JSON over a Unix socket: one line per
request, one per response, correlated by `id`. Transport failures (dead
socket, malformed line, id mismatch) are `IpcTransportError`s
(`inference_ipc_client.rs:233`) mapped to each method's error type;
outcome classification stays at the call site with exhaustive matches
(module doc, `:37-48`). `MAX_IPC_LINE_BYTES` (16 MiB, `:74`) caps
unbounded `read_line` growth (CWE-400).

## See also

- [hkask-inference Reference](./reference.md): class diagram and the full
  citation table.
- [hkask-inference Tutorial](./tutorial.md): routing your first request.
- [hkask-inference How-to](./how-to.md): wiring an MCP server to the
  bridge and adding a chat provider.
- [hkask-types Reference](../hkask-types/reference.md): the
  `InferencePort`, `ToolDispatchPort`, and `WorktreeSpawnPort` traits
  (`kask/crates/hkask-types/src/ports/inference_port.rs:147`, `:97`,
  `:135`).

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The port-trait boundary that lets the IPC-bridge client, the lazy fallbacks, and the unavailable stubs be swapped behind one trait object.

[^cwe400]: MITRE. (n.d.). *CWE-400: Uncontrolled Resource Consumption.* <https://cwe.mitre.org/data/definitions/400.html>. The unbounded `read_line` growth that `MAX_IPC_LINE_BYTES` caps.
