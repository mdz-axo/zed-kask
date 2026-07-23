# kask/ — hKask integration into zed-kask

This directory holds everything hKask that is **merged into the zed-kask fork**.
It is additive to upstream zed — `git merge upstream/main` never touches here.

## Deployment model
zed-kask is a **local single-user install**: one user, one sovereign UserPod, on the user's own machine. Identity/sign-on is the **Zed account** (the user signs into their existing Zed account; the `*.zed.dev` account/collab endpoints are kept). There is no cloud server, no Kubernetes/K3s, no hKask OAuth, no Admin/Member roles, no invite flow. Multiplayer/collaboration/voice/federation ride on **Zed's comms/voip/CRDT**; hKask's Matrix/7R7 transport is dropped. `hkask-identity` is **deleted entirely** (identity is the Zed account; pod identity lives in `hkask-pods`/`hkask-types`).

## Layout
- `crates/` — hKask keep-crates (`hkask-*`) + the bridge (`kask_bridge`, D8) + the panel (`kask_panel`, D10)
- `mcp-servers/` — the 15 hKask MCP server crates (12 loaded by default)
- `skills/` — the skills registry (`manifest.yaml` + `*.j2` templates; Pattern A source of truth)
- `scripts/` — hKask admin/build/CI scripts (including `check-hkask-no-zed-deps.sh`)
- `docs/` — architecture, specs, plans (the documentation home)

## References
- `DIVERGENCE.md` (repo root) — the fork's divergence manifest + upstream-sync procedure
- `kask/docs/architecture/zed-host-architecture-plan.md` — the full architecture + migration plan
- `kask/docs/specs/` — D1–D10 seam specifications
