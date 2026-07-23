# hkask-pods

UserPods, ACP, and pod management for hKask. Each user gets exactly one userpod (1:1) - their sovereign identity, memory, capabilities, and consent boundary within a shared install.

## Key Concepts

| Concept | Description |
|---------|-------------|
| **Pods** | Agent containers — CuratorPod, TeamPod, UserPodPod |
| **ACP** | Agent Communication Protocol — agent-to-agent messaging |
| **Bots** | Subsystem-curator bots (auto-spawn at startup) |
| **UserPods** | Authorial style replicas — embed, compose, compare |
| **Curator** | Human governance agent — metacognition, oversight |
| **OCAP** | Object-capability security — delegation with attenuation |

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PROVIDER` | Database provider (`sqlite` or `postgres`) |
| `HKASK_DB_PATH` | SQLite database path |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase |

## Pod Lifecycle