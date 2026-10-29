---
title: "kask_bridge — Reference"
audience: [developers, architects, agents working at the zed↔hKask seam]
last_updated: 2026-10-28
version: "1.5.5"
status: "Active"
domain: "Integration"
mds_categories: [domain, composition, trust, lifecycle]
---

# kask_bridge — Reference

`kask_bridge` is the D8 adapter between portable hKask ports and Zed-side
runtime facilities. Its governing dependency invariant and module root are at
`kask/crates/kask_bridge/src/kask_bridge.rs:3-10,23-121`.

## Current module surface

The crate root declares 30 top-level production modules
(`kask/crates/kask_bridge/src/kask_bridge.rs:23-121`; a 31st declaration,
`algedonic_board_real_tests`, is `#[cfg(test)]` at `:122-123`):

`condenser_bridge`, `context_injector`, `credentials`,
`database_maintenance`, `delegation_grants`, `host_skill_tools`, `identity`,
`inference_chat`, `inference_edit_prediction`, `inference_resilience`,
`passphrase_rotation`, `inference_embedding`, `inference_ipc_server`,
`inference_providers`, `inference_socket`, `mcp_env`, `mcp_servers`, `memory`,
`model_resolution`, `settings`, `metacognition_bridge`, `directive_bridge`,
`algedonic_log_bridge`, `algedonic_board`, `context_server_health_bridge`,
`ocr_health_bridge`, and `rollout_event_bridge`.

The public re-exports are defined at
`kask/crates/kask_bridge/src/kask_bridge.rs:27-124`. Skill execution is not a
bridge module; the agent-side `SkillTool` owns body injection — the bridge root
exports no skill executor (`kask/crates/kask_bridge/src/kask_bridge.rs:50-124`).

## Settings hierarchy

`KaskSettings` contains the data and artifact roots plus 11 subsystem sections:
`general`, `mcp`, `curator`, `memory`, `condenser`, `research`,
`corpus`, `scenarios`, `prediction_markets`, `media`, and
`models` (`kask/crates/kask_bridge/src/settings.rs:56-89`; the former
`companies` section was removed with the companies server 2026-10-18,
commit `3439db5e00`, and the `swarm` section with the whole local swarm
capability 2026-10-28).

```mermaid
classDiagram
    class KaskSettings {
        +data_dir: String
        +artifacts_dir: String
        +general: KaskGeneralSettings
        +mcp: KaskMcpSettings
        +curator: KaskCuratorSettings
        +memory: KaskMemorySettings
        +condenser: KaskCondenserSettings
        +research: KaskResearchSettings
        +corpus: KaskCorpusSettings
        +scenarios: KaskScenariosSettings
        +prediction_markets: KaskPredictionMarketsSettings
        +media: KaskMediaSettings
        +models: KaskModelsSettings
    }
    class KaskGeneralSettings {
        +max_concurrency: u32
        +inference_timeout_secs: u64
        +inference_circuit_failure_threshold: u32
        +inference_circuit_open_secs: u64
    }
    class KaskMcpSettings {
        +delegated_tools: Map
        +load_default: bool
        +overrides: Map
    }
    class KaskModelsSettings {
        +default_model: String
        +embedding_model: String
        +classifier_model: String
        +qa_generation_model: String
        +ocr_model: String
        +rerank_model: String
    }
    KaskSettings --> KaskGeneralSettings
    KaskSettings --> KaskMcpSettings
    KaskSettings --> KaskModelsSettings
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-BRIDGE-003
verified_date: 2026-10-28
verified_against: kask/crates/kask_bridge/src/settings.rs:35-90 (KaskSettings); settings.rs general + Default; settings.rs mcp + Default; settings.rs media + Default; settings.rs models + Default
status: VERIFIED
-->

There is no general batch-size setting and no per-DB passphrase setting.
The database passphrase is provisioned once and shared by SQLCipher consumers
(`crates/zed/src/main.rs:1690-1693`); scheduled rotation is applied at startup
by `run_pending_db_passphrase_rotation` (`crates/zed/src/main.rs:373`) — see
[`kask-settings.md`](../../reference/kask-settings.md) under Passphrase rotation.

### Defaults

| Settings type | Current defaults | Evidence |
| --- | --- | --- |
| `KaskGeneralSettings` | concurrency 96; timeout 300s; circuit threshold 3; open interval 30s | `kask/crates/kask_bridge/src/settings.rs:102-130` |
| `KaskMcpSettings` | load defaults; empty overrides; empty delegated-tool grants | `kask/crates/kask_bridge/src/settings.rs:140-160` |
| `KaskCuratorSettings` | always on; algedonic threshold 0.8 | `kask/crates/kask_bridge/src/settings.rs:170-196` |
| `KaskMemorySettings` | consolidation 300s; confidence 0.3; recall 5 at 0.3; auto-inject; federated auto-inject off with empty `federated_source_ids`; distillation 600s/300s; memory life 90 from `MemoryStore::default_memory_life_days`; forgetting 7 | `kask/crates/kask_bridge/src/settings.rs:226-291`; `kask/crates/hkask-memory/src/memory_store.rs:199` |
| `KaskCondenserSettings` | normal profile; incoming-result compression off | `kask/crates/kask_bridge/src/settings.rs:299-318` |
| `KaskCorpusSettings` | dimension 1024; embedding default `OpenRouter/qwen/qwen3-embedding-8b` (MRL — every embed request carries `dimensions = embedding_dim`); template root `kask/registry` | `kask/crates/kask_bridge/src/settings.rs:332-352` |
| `KaskMediaSettings` | STT and vision use shared constants; TTS/image/video empty | `kask/crates/kask_bridge/src/settings.rs:443-456` |
| `KaskModelsSettings` | default GLM 5.3; classifier GLM 5.2; QA generator empty; OCR `ollama/glm-ocr:latest`; reranker from `DEFAULT_RERANK_MODEL` | `kask/crates/kask_bridge/src/settings.rs:489-561` |

## Built-in MCP registry

`BUILT_IN_MCP_SERVERS` contains 8 descriptors
(`kask/crates/kask_bridge/src/mcp_servers.rs:49-407`). IDs and descriptions
are derived from that registry
(`kask/crates/kask_bridge/src/mcp_servers.rs:411-412,458-463`).

```mermaid
classDiagram
    class BuiltinMcpServer {
        +id: str
        +binary: str
        +description: str
        +credentials: allowlist
        +config_env: allowlist
    }
    class Registry {
        corpus
        curator
        experimentation
        kata-kanban
        research
        scenarios
        prediction-markets
        media
    }
    Registry --> BuiltinMcpServer : 8 entries
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-BRIDGE-004
verified_against: kask/crates/kask_bridge/src/mcp_servers.rs:26-46 (BuiltinMcpServer); kask/crates/kask_bridge/src/mcp_servers.rs:49-407 (8-entry registry)
status: VERIFIED
verified_date: 2026-10-28
-->

### Allowlist sizes

Sizes below are counted directly from each descriptor in
`kask/crates/kask_bridge/src/mcp_servers.rs:49-407` (method: extraction of the
quoted env-var names inside each `credentials` / `config_env` array).

| Server | Credentials | Config |
| --- | ---: | ---: |
| `corpus` | 1 | 18 |
| `curator` | 1 | 12 |
| `kata-kanban` | 1 | 3 |
| `research` | 6 | 6 |
| `scenarios` | 0 | 2 |
| `prediction-markets` | 1 | 3 |
| `media` | 4 | 10 |
| `experimentation` | 1 | 2 |

The media server's live router is pinned at 74 tools
(`kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs:455-465`).

## Child environment lifecycle

`build_mcp_server_env` receives settings, a credential provider, an optional
inference socket, and an optional timeout
(`kask/crates/kask_bridge/src/mcp_servers.rs:548-556@build_mcp_server_env`, full fn to `:746`). It then:

1. filters non-secret configuration and adds the database catalog and delegated
   tool grant (`kask/crates/kask_bridge/src/mcp_servers.rs:560-588@build_mcp_server_env`);
2. filters and resolves credentials from shell or keychain, with first-run
   provisioning for the shared database passphrase
   (`kask/crates/kask_bridge/src/mcp_servers.rs:589-721@build_mcp_server_env`);
3. adds the inference socket and timeout
   (`kask/crates/kask_bridge/src/mcp_servers.rs:472@build_mcp_server_env`).

Unknown IDs receive neither credentials nor configuration
(`kask/crates/kask_bridge/src/mcp_servers.rs:414@filter_credentials_for_server,803@filter_config_env_for_server`).

## Nonblocking composition

The deferred composition task samples the current Zed user but never waits for
one; it uses `"kask"` when absent (`crates/zed/src/main.rs:1589-1646`). The
same task provisions storage, installs memory/context hooks, installs the
condenser, and launches the configured servers
(`crates/zed/src/main.rs:1690-1760,1982-2109,2510-2519,2545-2600`).

## Procedures

### Add a built-in MCP server

Use this procedure to add a managed MCP server without leaking unrelated
configuration or credentials. The canonical registry currently contains 9
servers (`kask/crates/kask_bridge/src/mcp_servers.rs:49-407`).

```mermaid
flowchart TD
    A[Add one BuiltinMcpServer entry] --> B[Declare credentials allowlist]
    B --> C[Declare config_env allowlist]
    C --> D[Emit required non-secret settings]
    D --> E[Pin allowlist and live router surfaces]
    E --> F[Run focused tests and script/clippy]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-BRIDGE-002
verified_date: 2026-10-19
verified_against: kask/crates/kask_bridge/src/mcp_servers.rs:28-44 (descriptor shape); kask/crates/kask_bridge/src/mcp_servers.rs:49-407 (registry); kask/crates/kask_bridge/src/mcp_servers.rs:536-734 (build_mcp_server_env)
status: VERIFIED
-->

#### 1. Add the descriptor

Add one `BuiltinMcpServer` value to `BUILT_IN_MCP_SERVERS`. Supply a unique
`id`, the binary name, the settings description, and both allowlists. The
registry order is stable and used by the panel
(`kask/crates/kask_bridge/src/mcp_servers.rs:26-50`). The ID and description
views derive from the registry, so no companion list is maintained
(`kask/crates/kask_bridge/src/mcp_servers.rs:411-412,458-463`).

#### 2. Declare the credential boundary

Declare `&[]` when the server reads no secrets, or `&[...]` with only the
exact credential environment names it consumes — the allowlist is
non-optional, so every server declares its credential boundary explicitly
(`kask/crates/kask_bridge/src/mcp_servers.rs:37-44`).

`filter_credentials_for_server` passes only the declared names and gives an
unknown ID no credentials (`kask/crates/kask_bridge/src/mcp_servers.rs:414@filter_credentials_for_server`).
Add an allowlist-alignment test beside the existing per-server tests, such as
`research_allowlist_matches_actual_reads`
(`kask/crates/kask_bridge/src/mcp_servers.rs:1196@research_allowlist_matches_actual_reads`).

#### 3. Declare the non-secret configuration boundary

Set `config_env` to `&[]` or an exact list of non-secret variables emitted
by `KaskSettings::mcp_env` (`kask/crates/kask_bridge/src/settings.rs:662`). The
config filter also fails closed for unknown IDs
(`kask/crates/kask_bridge/src/mcp_servers.rs:803@filter_config_env_for_server`). Do not place database
passphrases or API keys in this list.

The current `media` descriptor is the worked example: three credentials
(`OPENROUTER_API_KEY`, `DEEPINFRA_API_KEY`, `HKASK_SERPAPI_API_KEY`)
and seven configuration variables, including the IPC socket,
data/artifact roots, gallery DB, two media-model fields, and the embedding
model (`kask/crates/kask_bridge/src/mcp_servers.rs:348-384`).

#### 4. Emit new settings through `mcp_env`

If the server needs a new non-secret setting, add it to the relevant settings
sub-structure and its `mcp_env` emitter, then add that exact environment name to
the server descriptor. Defaults belong only in `Default` implementations
(`kask/crates/kask_bridge/src/settings.rs:24-34`). The emitted-default seam is
pinned by `mcp_env_emits_nothing_for_default_settings`
(`kask/crates/kask_bridge/src/mcp_env.rs:370`).

Do not invent a batch-size control or a second database-passphrase setting. The
general settings expose concurrency, timeout, and circuit-breaker controls
(`kask/crates/kask_bridge/src/settings.rs:102-130`), and all SQLCipher consumers
share `HKASK_DB_PASSPHRASE`. Rotation is
startup-coordinated via the pending slot and is documented in
[`kask-settings.md`](../../reference/kask-settings.md) under Passphrase rotation.

#### 5. Pin the registered tool surface

A router can compile while silently omitting a sub-router. Add an end-to-end
count/name pin in the server crate. The current media server pins exactly 99
registered tools in `tool_surface_is_exactly_102_registered_tools`
(`kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs`).

#### 6. Validate

Run the focused server tests, the bridge allowlist tests, and then
`./script/clippy`. Confirm that:

- the registry count is still intentional;
- credential and config read sites equal their allowlists;
- the composed environment path preserves both key classes;
- the live router equals the advertised tool surface.

The composed-path regression test is
`build_mcp_server_env_composition_respects_allowlists`
(`kask/crates/kask_bridge/src/mcp_servers.rs:1370@build_mcp_server_env_composition_respects_allowlists`).

## Further reading

- [Bridge explanation](./explanation.md)
