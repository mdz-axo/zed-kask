# hkask-git-cas

gix-backed Git content-addressable storage adapter — the backup/gitcas component for registries and artifacts.

Implements [`hkask_ports::git_cas::GitCASPort`] and pod-directory backup for files, YAML/templates, databases, and logs. It is the **only** crate in the workspace that depends on `gix`. Thin MCP servers depend on `hkask-mcp` (gix-free) and pay no git-engine compile cost; only components that actually instantiate backup/admin operations depend on this crate.

## Public API

- `GixCasAdapter` — the adapter implementing `GitCASPort` over a gix object store (BLAKE3-hashed).

## Why it is isolated

Confining the `gix` dependency here keeps the heavy git-engine compile cost off every other crate. The port trait (`GitCASPort`) is the only seam callers depend on; the gix implementation is an invisible detail behind it.

## Dependencies

- `gix` — the only workspace consumer of the git engine
- `hkask-ports` — `GitCASPort` trait
- `hkask-storage-core` — path sanitization helpers