# Zed-Kask

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)
[![CI](https://github.com/mdz-axo/zed-kask/actions/workflows/run_tests.yml/badge.svg)](https://github.com/mdz-axo/zed-kask/actions/workflows/run_tests.yml)
[![Release](https://github.com/mdz-axo/zed-kask/actions/workflows/kask-release.yml/badge.svg)](https://github.com/mdz-axo/zed-kask/actions/workflows/kask-release.yml)

**Zed-Kask** is a fork of [Zed](https://zed.dev) — the high-performance code editor from the creators of Atom and Tree-sitter — with [Kask](./kask/) agentic AI tools integrated in-process. Zed's editor, collaboration, and inference surfaces are joined to Kask's skills, MCP servers, regulation nervous system, and sovereign memory as native editor surfaces: one clone, one build, one CI.

Kask is a **local, single-user** agentic AI toolkit — not a cloud platform, not an agent framework. There is no autonomous agent loop by default; the human is in the loop, and skills escalate _to the user_. Everything Kask lives under [`kask/`](./kask/) (additive — `git merge upstream/main` never touches it). Everything outside `kask/` is upstream Zed except the small set of seam edits documented in [`DIVERGENCE.md`](./DIVERGENCE.md).

> **Linux x86_64 prebuilt binaries available.** Install `zed-kask` plus all `hkask-mcp-*` MCP server binaries:
>
> ```bash
> curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
> ```

---

## What You Get

Zed-Kask adds three native editor surfaces that upstream Zed does not have. Each runs in-process — no separate daemon, no external service.

### Skills

**69 agent-facing skills** (PDCA loops) execute inside the agent panel via the manifest cascade. A skill is a _process_, not a prompt: it composes Jinja2 templates into Plan-Do-Check-Act cycles with convergence thresholds, gas budgets, and escalation to the user. Skills, manifests, and templates are seeded to disk at startup and editable at runtime; user edits are never overwritten. See [`kask/docs/reference/skills/README.md`](./kask/docs/reference/skills/README.md) for the registry and [`kask/docs/explanation/skills-and-composition.md`](./kask/docs/explanation/skills-and-composition.md) for the anatomy.

### MCP servers

**13 built-in MCP servers** (311 tools fleet-wide) are launched as child processes over stdio and exposed as agent tools through `rmcp`:

| Server | Surface |
| --- | --- |
| `codegraph` | Code-graph query, traversal, and context assembly |
| `companies` | Company valuation, forecasting, portfolio |
| `condenser` | Thread/session compression algorithms |
| `corpus` | Gather→process→output document pipeline |
| `curator` | Curator-scoped memory and regulation surfaces |
| `kata-kanban` | Kata-driven task kanban |
| `media` | Image/video/audio gallery and generation |
| `portfolio` | Portfolio dashboard and positions |
| `prediction-markets` | Polymarket/Kalshi calibration |
| `research` | Web research and extraction |
| `scenarios` | Schwartz/Tetlock scenario pipeline |
| `swarm` | Agent Bestiary World swarm orchestration |
| `training` | LoRA/QLoRA training configuration and contracts |

See [`kask/docs/reference/mcp-servers/README.md`](./kask/docs/reference/mcp-servers/README.md) for the full registry.

### Curator agent

The **Curator** is a native in-process agent — an `Agent::Curator` variant selectable alongside the Zed coding agent in the Agent Panel. It is the system's cybernetic regulator, not an autonomous agent: it runs regulation and metacognition loops against the user's sovereign pod and escalates _to the user_ rather than acting on its own. The Curator carries its own sovereign memory store and gets all coding capabilities _plus_ regulatory context and tools. See [`kask/docs/architecture/zed-host-architecture-plan.md`](./kask/docs/architecture/zed-host-architecture-plan.md) for the composition root wiring.

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

| Variable                 | Default            | Purpose                                               |
| ------------------------ | ------------------ | ----------------------------------------------------- |
| `HKASK_VERSION`          | latest release     | Pin a release tag (e.g. `v0.233.10`) or `weekly`      |
| `HKASK_CHANNEL`          | `stable`           | Set to `weekly` for the weekly build                  |
| `INSTALL_DIR`            | `$HOME/.local`     | Install prefix; binaries land in `$INSTALL_DIR/bin`   |
| `HKASK_SYSTEM_INSTALL`   | unset              | Set to `true` to symlink into `/usr/local/bin`        |
| `HKASK_REPO`             | `mdz-axo/zed-kask` | Override the GitHub owner/repo                        |
| `HKASK_NO_FALLBACK`      | unset              | Set to `true` to skip the source-build fallback       |
| `HKASK_ALLOW_UNVERIFIED` | unset              | Set to `true` to proceed when `SHA256SUMS` is missing |

### Weekly

The `weekly` tag is force-moved once a week by the release workflow (08:00 UTC each Monday). Install the current weekly build:

```bash
HKASK_CHANNEL=weekly curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
```

### Source build (Linux, requires Rust toolchain)

If you prefer to build from source, or as a fallback when the prebuilt binary download fails, the source-build installer clones the repo at the pinned tag, installs system dependencies via `script/linux`, and builds with `cargo`:

```bash
curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install.sh | bash
```

The binary installer (`install-binary.sh`) automatically falls back to this path if the prebuilt archive download fails, fetching the installer scripts from the release assets (tag-pinned, checksum-verified).

---

## Architecture

Zed-Kask keeps Kask's hexagonal port surface and implements every adapter in one bridge crate, `kask/crates/kask_bridge`, so that Kask crates never depend on Zed crates — Zed-Kask depends on Kask, never the reverse. This is the governing invariant, enforced in CI by `kask/scripts/check-hkask-no-zed-deps.sh`.

The seam between the two sides is small and documented: thirty named divergence points (D1–D30; D4 and D10 removed) cover every edit to Zed's tree outside `kask/`. The full table and upstream-sync procedure live in [`DIVERGENCE.md`](./DIVERGENCE.md). The composition-root wiring is documented in [`kask/docs/architecture/zed-host-architecture-plan.md`](./kask/docs/architecture/zed-host-architecture-plan.md).

Federation, multiplayer, and sign-in ride on Zed's existing collaboration and identity capabilities. Kask's own Matrix transport and separate identity crate are not carried over — the local Kask pod stays sovereign, and users sign in with their existing Zed account.

---

## Developing

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

The Kask side builds as part of the same workspace — the `kask/` crates are workspace members. Kask conforms to Zed's dependency versions where there are package conflicts; do not bump Zed's workspace deps to accommodate Kask.

For Kask-specific contribution (skills, MCP servers, the architecture), start at [`kask/docs/README.md`](./kask/docs/README.md). Per-crate documentation (tutorial, how-to, reference, explanation) lives in the [Diataxis set](./kask/docs/diataxis/INDEX.md) — 41 artifacts across 11 cross-cutting crate sets. See [CONTRIBUTING.md](./CONTRIBUTING.md) for general contribution guidelines.

---

## Releases

Releases are cut by pushing a `v*` tag (e.g. `v0.233.10`). The [`kask-release`](./.github/workflows/kask-release.yml) workflow builds the Linux x86_64 archive, generates `SHA256SUMS`, and publishes a GitHub Release with auto-generated notes. A weekly build runs on schedule at 08:00 UTC each Monday, force-moving the `weekly` tag.

Release assets:

- `zed-kask-x86_64-unknown-linux-gnu.tar.gz` — `zed-kask` + all `hkask-mcp-*` binaries
- `install.sh`, `install-common.sh`, `mcp-servers.txt` — source-build fallback scripts (tag-pinned, checksum-verified)
- `SHA256SUMS` — checksums for all of the above

See the [releases page](https://github.com/mdz-axo/zed-kask/releases) for all published versions.

---

## License

Zed-Kask inherits Zed's licensing: GPL-3.0-or-later primarily, with Apache-2.0 components where marked. License information for third-party dependencies must be correctly provided for CI to pass; see [`script/licenses/zed-licenses.toml`](./script/licenses/zed-licenses.toml) and the [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) configuration for details.
