<p align="center">
  <img src="kask/assets/zk-icon.svg" alt="Zed-Kask" width="128" height="128" />
</p>

# Zed-Kask

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)

**Zed-Kask** is a minimal-divergence fork of [Zed](https://zed.dev) with the hKask agent platform compiled in-process. Zed's editor, collaboration, and inference surfaces are joined to Kask's skills, MCP servers, regulation nervous system, and sovereign memory as native editor surfaces: one clone, one build, one CI.

Kask is a **local, single-user** agentic AI toolkit — not a cloud platform, not an agent framework. There is no autonomous agent loop by default; the human is in the loop, and skills escalate _to the user_. Everything Kask lives under [`kask/`](./kask/) (additive — `git merge upstream/main` never touches it). Everything outside `kask/` is upstream Zed except the named seam edits documented in [`DIVERGENCE.md`](./DIVERGENCE.md).

> **Linux x86_64 prebuilt binaries available.** Install `zed-kask` plus all `hkask-mcp-*` MCP server binaries:
>
> ```bash
> curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
> ```

---

## What You Get

Zed-Kask adds native editor surfaces that upstream Zed does not have. Each runs in-process — no separate daemon, no external service.

### Skills

**60 agent-facing skills** execute inside the agent panel. A skill is a _process_, not a prompt: its `SKILL.md` body is injected into the conversation, and the model self-iterates a Plan-Do-Act cycle using two built-in tools — `lisp_eval` for deterministic checks against `hkask-lisp` and `render_template` for structured prompt scaffolding from the seeded Jinja2 template registry. Skills and templates are seeded to disk at startup and editable at runtime; user edits are never overwritten. See [`kask/docs/reference/skills/`](./kask/docs/reference/skills/) for the registry and [`kask/docs/explanation/`](./kask/docs/explanation/) for the anatomy.

### MCP servers

**10 built-in MCP servers** (**259 `#[tool]` methods** fleet-wide) are launched as child processes over stdio and exposed as agent tools through `rmcp`:

| Server                 | Surface                                              |
| ---------------------- | ---------------------------------------------------- |
| `companies`            | Company valuation, forecasting, portfolio            |
| `corpus`               | Gather→process→output document pipeline              |
| `curator`              | Curator-scoped memory and regulation surfaces        |
| `kata-kanban`          | Kata-driven task kanban with idempotent creates      |
| `portfolio`            | Portfolio dashboard and positions                    |
| `prediction-markets`   | Polymarket/Kalshi calibration                        |
| `research`             | Web research and extraction                          |
| `scenarios`            | Schwartz/Tetlock scenario pipeline                   |
| `swarm`                | Agent Bestiary World swarm orchestration             |
| `training`             | LoRA/QLoRA training configuration and contracts      |

See [`kask/docs/reference/mcp-servers/README.md`](./kask/docs/reference/mcp-servers/README.md) for the full registry.

### Curator agent

The **Curator** is a native in-process agent — an `Agent::Curator` variant selectable alongside the Zed coding agent in the Agent Panel. It is the system's cybernetic regulator, not an autonomous agent: it runs regulation and metacognition loops against the user's sovereign pod and escalates _to the user_ rather than acting on its own. The Curator carries its own sovereign memory store and gets all coding capabilities _plus_ regulatory context and tools.

### Swarm and Kanban panels

Two native panels extend the steering surface. The **swarm panel** composes and steers local agent swarms (agent cards, hiring, PSO/ACO/flocking parameters); the **kanban panel** drives kata-kanban task boards. Both can take the steering seat of an agent conversation via a per-context system-prompt overlay scoped to exactly the MCP server whose tools the overlay advertises.

---

## Installation

### Prebuilt binary (Linux x86_64)

Downloads the latest release archive — `zed-kask` plus all `hkask-mcp-*` binaries — verifies it against the published `SHA256SUMS`, and installs to `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
```

Verify the install:

```bash
zed-kask --help
ls ~/.local/bin/hkask-mcp-*
```

If `~/.local/bin` is not on your `PATH`, start a new shell or:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Environment variables the installer honors:

| Variable               | Default            | Purpose                                               |
| ---------------------- | ------------------ | ----------------------------------------------------- |
| `HKASK_VERSION`        | latest release     | Pin a release tag (e.g. `v0.233.10`)                 |
| `INSTALL_DIR`          | `$HOME/.local`     | Install prefix; binaries land in `$INSTALL_DIR/bin`   |
| `HKASK_SYSTEM_INSTALL` | unset              | Set to `true` to symlink into `/usr/local/bin`        |
| `HKASK_REPO`           | `mdz-axo/zed-kask` | Override the GitHub owner/repo                        |
| `HKASK_REMOVE_CONFIG`  | unset              | Set to `true` to remove config and data on uninstall  |

An updater is installed alongside the binaries; run `update-zed-kask` (or `kask/scripts/build/update-zed-kask.sh`) to move to a newer release.

### Source build (Linux, requires Rust toolchain)

If you prefer to build from source, or as a fallback when the prebuilt binary download fails, the source-build installer clones the repo at the pinned tag, installs system dependencies via `script/linux`, and builds with `cargo`:

```bash
curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash
```

The binary installer (`install-binary.sh`) automatically falls back to this path if the prebuilt archive download fails, fetching the installer scripts from the release assets (tag-pinned, checksum-verified).

---

## Architecture

Zed-Kask keeps Kask's hexagonal port surface and implements every adapter in one bridge crate, `kask/crates/kask_bridge`, so that Kask crates never depend on Zed crates — Zed-Kask depends on Kask, never the reverse. This is the governing invariant, enforced in CI by `kask/scripts/check-hkask-no-zed-deps.sh`.

The seam between the two sides is small and documented: the named divergence points (D1–D32; five since removed) cover every edit to Zed's tree outside `kask/`. The full table and upstream-sync procedure live in [`DIVERGENCE.md`](./DIVERGENCE.md). The composition-root wiring is documented in [`kask/docs/architecture/zed-host-architecture-plan.md`](./kask/docs/architecture/zed-host-architecture-plan.md).

Tool authority lives at the boundaries whose list the caller does not choose: the per-request `tool_allowlist` on the inference IPC dispatch (fail-closed), each swarm agent card's `mcp_tools` allowlist, and per-server MCP env/credential allowlists. Interrupted tool calls (unknown delivery) are never auto-retried; the three kanban create tools carry server-side idempotency keys so a replay cannot double an effect.

Federation, multiplayer, and sign-in ride on Zed's existing collaboration and identity capabilities. Kask's own Matrix transport and separate identity crate are not carried over — the local Kask pod stays sovereign, and users sign in with their existing Zed account.

---

## Developing

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

The Kask side builds as part of the same workspace — the `kask/` crates are workspace members. Kask conforms to Zed's dependency versions where there are package conflicts; do not bump Zed's workspace deps to accommodate Kask.

For Kask-specific contribution (skills, MCP servers, the architecture), start at [`kask/docs/README.md`](./kask/docs/README.md). Per-crate documentation (tutorial, how-to, reference, explanation) lives in the [Diataxis set](./kask/docs/diataxis/INDEX.md) — 36 artifacts across 10 cross-cutting crate sets. See [CONTRIBUTING.md](./CONTRIBUTING.md) for general contribution guidelines.

---

## Releases

Releases are cut by pushing a `v*` tag (e.g. `v0.233.10`). The release workflow builds the Linux x86_64 archive, generates `SHA256SUMS`, and publishes a GitHub Release with auto-generated notes.

Release assets:

- `zed-kask-x86_64-unknown-linux-gnu.tar.gz` — `zed-kask` + all `hkask-mcp-*` binaries
- `install.sh`, `install-common.sh`, `mcp-servers.txt` — source-build fallback scripts (tag-pinned, checksum-verified)
- `SHA256SUMS` — checksums for all of the above

See the [releases page](https://github.com/mdz-axo/zed-kask/releases) for all published versions.

---

## License

Zed-Kask inherits Zed's licensing: GPL-3.0-or-later primarily, with Apache-2.0 components where marked. License information for third-party dependencies must be correctly provided for CI to pass; see [`script/licenses/zed-licenses.toml`](./script/licenses/zed-licenses.toml) and the [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) configuration for details.
