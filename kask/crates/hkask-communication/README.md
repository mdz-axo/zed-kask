# hkask-communication

Core Matrix transport, agent registry, 7R7 listener, and Regulation bridge for hKask.

## Architecture

```
Matrix message (Conduit)
       │
       ▼
  7R7 Listener (30s poll)
       │
       ├──► tracing log
       │
       └──► NuEvent::persist() → NuEventStore
               │
               ▼
       CurationLoop.sense() (filters communication.* events)
               │
               ▼
       MetacognitionLoop (CAT evaluate → respond template)
               │
               ▼
       MCP: communication.send_message() → Conduit
```

## Features

- **Matrix transport** — full matrix-sdk integration: login, send/receive messages, create rooms, invite users, list rooms, upload/send files
- **Agent registry** — WebID ↔ Matrix UserId mapping with thread watchlists
- **7R7 listener** — passive room observer, polls rooms, persists Regulation NuEvents for curation awareness
- **Regulation bridge** — communication events flow into the NuEvent store with `communication.message` and `communication.thread` algedonic categories
- **CAT engagement** — Communication Accommodation Theory gate: `convergence_bias` scalar decides speak/silent per agent

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `HKASK_MATRIX_URL` | Matrix homeserver URL | `http://localhost:8008` |
| `HKASK_MATRIX_REGISTRATION_TOKEN` | Registration token | `hkask-dev` |
| `HKASK_MATRIX_AGENT_USERNAME` | Agent Matrix username | (from keychain) |
| `HKASK_MATRIX_AGENT_PASSWORD` | Agent Matrix password | (from keychain) |

## Quick Start

```bash
./scripts/conduit/conduit-docker.sh start
./scripts/conduit/conduit-docker.sh register
HKASK_MATRIX_URL=http://localhost:8008 cargo run -- chat
# In REPL: /mcp start communication
```

## Deferred

- E2EE: deferred to v2 (SQLCipher/SQLite linking conflict with matrix-sdk-sqlite)
- Continuous sync: v1 uses on-demand polling via `get_messages()`
- MatrixTransport integration tests: require running Conduit (tests exist, marked `#[ignore]`)
