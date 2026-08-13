# Zed-Kask

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)
[![CI](https://github.com/mdz-axo/zed-kask/actions/workflows/run_tests.yml/badge.svg)](https://github.com/mdz-axo/zed-kask/actions/workflows/run_tests.yml)
[![Release](https://github.com/mdz-axo/zed-kask/actions/workflows/kask-release.yml/badge.svg)](https://github.com/mdz-axo/zed-kask/actions/workflows/kask-release.yml)

Welcome to **Zed-Kask** — a fork of [Zed](https://zed.dev), the high-performance, multiplayer code editor from the creators of [Atom](https://github.com/atom/atom) and [Tree-sitter](https://github.com/tree-sitter/tree-sitter), with the [Kask](#what-kask-is) agentic AI tools integrated in-process. The focus is the integration itself: Zed's editor, collaboration, and inference surfaces joined to Kask's skills, MCP servers, regulation nervous system, and sovereign memory as native editor surfaces — one clone, one build, one CI. Multiplayer and federation ride on Zed's existing collaboration capabilities; Kask's own Matrix transport is not carried over.

Zed-Kask is one clone, one build, one CI. Everything Kask lives under [`kask/`](./kask/) (additive — `git merge upstream/main` never touches it). Everything outside `kask/` is upstream Zed except the small set of seam edits documented in [`DIVERGENCE.md`](./DIVERGENCE.md).

> **Prebuilt binaries are available** for Linux x86_64. Install `zed-kask` plus all `hkask-mcp-*` MCP server binaries with one command:
>
> ```bash
> curl -fsSL https://raw.githubusercontent.com/mdz-axo/zed-kask/main/kask/scripts/build/install-binary.sh | bash
> ```
>
> The script downloads the latest release archive, verifies it against the published `SHA256SUMS`, and installs to `~/.local/bin`. See [Installation](#installation) for channel selection, pinning, and source-build fallback.

---

## Key Features

Zed-Kask adds three native editor surfaces that upstream Zed does not have. Each is wired through a small, documented seam ([`DIVERGENCE.md`](./DIVERGENCE.md)) and runs in-process — no separate daemon, no external service.

### Skills

**61 agent-facing skills** (PDCA loops), each composed of a [FlowDef manifest](./kask/registry/manifests/) plus a [template crate](./kask/registry/templates/), execute inside the agent panel via the manifest cascade (D1). A skill is a _process_, not a prompt: it composes Jinja2 templates into Plan-Do-Check-Act cycles with convergence thresholds, gas budgets, and escalation to the user — there is no autonomous agent loop by default. The shipped skills, manifests, and templates are seeded to disk at startup (seed-if-missing; user edits are never overwritten) and are editable at runtime. The 61 `SKILL.md` companions in [`.agents/skills/`](./.agents/skills/) are discovery-only (the catalog description the agent reads to pick a skill); the manifest is the source of truth. See [`kask/docs/reference/skills/README.md`](./kask/docs/reference/skills/README.md) for the registry and [`kask/docs/explanation/skills-and-composition.md`](./kask/docs/explanation/skills-and-composition.md) for the anatomy.

### MCP servers

**13 built-in MCP servers** (305 tools fleet-wide) are launched as child processes over stdio by Zed's `context_server` host (D3) and exposed as agent tools through `rmcp`:

| Server | Surface |
| --- | --- |
| `codegraph` | Code-graph query, traversal, and context assembly |
| `companies` | Company valuation, forecasting, portfolio |
| `condenser` | Thread/session compression algorithms |
| `corpus` | Gather→process→output document pipeline (folded `docproc` + `replica`) |
| `curator` | Curator-scoped memory and regulation surfaces |
| `kata-kanban` | Kata-driven task kanban |
| `media` | Image/video/audio gallery and generation |
| `portfolio` | Portfolio dashboard and positions |
| `prediction-markets` | Polymarket/Kalshi calibration |
| `research` | Web research and extraction |
| `scenarios` | Schwartz/Tetlock scenario pipeline |
| `swarm` | Agent Bestiary World swarm orchestration |
| `training` | LoRA/QLoRA training configuration and contracts |

Two parallel launch paths serve different consumers: the app-global `McpRuntime` (governed dispatch with capability-match gate, gas budgeting, and `reg.tool.*` spans — serves the skill cascade) and the per-project `ContextServerStore` (serves the agent tool picker). Both are by design. See [`kask/docs/reference/mcp-servers/README.md`](./kask/docs/reference/mcp-servers/README.md) for the full registry.

### Curator agent

The **Curator** (D2) is a native in-process agent — an `Agent::Curator` variant backed by hKask's Regulation and metacognition loops, selectable alongside the Zed coding agent in the Agent Panel. It is the system's cybernetic regulator, not an autonomous agent: it runs the CyberneticsLoop (variety engineering, algedonic alerts), MetacognitionLoop, and ConsolidationService against the user's sovereign pod, and escalates _to the user_ rather than acting on its own. The Curator carries its own sovereign memory store (`agents/curator/curator.db`, D6) — Curator turns are ingested as curator-perspective episodic + semantic records, and the curator's context injector (D8) recalls from its own DB so it builds its own memory automatically. Its `CuratorStatusTool` and regulatory static context are appended to the Zed Agent prompt (not an override — the coding instructions stay intact), so the Curator gets all coding capabilities _plus_ regulatory context and tools. See [`kask/docs/architecture/zed-host-architecture-plan.md`](./kask/docs/architecture/zed-host-architecture-plan.md) for the composition root wiring.

---

## What Kask Is

Kask is a set of **agentic AI tools** — skills, MCP servers, regulation, and sovereign memory — designed to run inside a host editor rather than as a standalone platform. In Zed-Kask it runs as a **local, single-user install**: one user, one sovereign pod, on the user's own machine. It is not an agent framework — there is no autonomous agent loop by default; the human is in the loop and skills escalate _to the user_, not away from them. The Curator is the system's cybernetic regulator, not an autonomous agent.

Three things sit between the user and a model:

1. **Skills** — PDCA loops that compose Jinja2 templates into Plan-Do-Check-Act cycles with convergence thresholds, gas budgets, and escalation. Where other systems give you a prompt, Kask gives you a _process_.
2. **MCP servers** — built-in Model Context Protocol servers (research, memory, codegraph, media, filesystem, regulation, …) exposed as tools through `rmcp`.
3. **Inference routing** — one router across multiple providers, with circuit breakers and per-call gas accounting.

Everything else in Kask — the pod, wallet, ledger, regulation, keystore — exists to keep the user's local session **sovereign**: per-pod encrypted storage, OCAP dual gate, visibility gating.

### Federation is Zed's job

Kask's own Matrix/7R7 transport is **not** carried into Zed-Kask. Federation and multi-user communication ride on **Zed's existing collaboration capabilities** (channels, rooms, voice, contact sharing). The local Kask pod stays sovereign; reaching other users happens through Zed's comms layer, not through Kask's federation plumbing.

### Sign-in is Zed's job too

There is no cloud server, no Kubernetes, no Kask OAuth, and no Admin/Member roles or invite flow. Users sign in with their **existing Zed account** — that single login gates the Zed-based features (communication, collaboration, voice). The local pod is bound to the signed-in account at startup. Kask's separate identity crate is removed entirely — identity is the Zed account, and pod identity lives in the pod runtime (`hkask-pods`) and the primitives in `hkask-types` (`PodID`, `WebID`, `UserID`, `WalletId`).

### What Kask Is Not

- Not an agent framework. No autonomous agent loop by default; skills escalate _to the user_.
- Not a multi-tenant cloud server. Zed-Kask is a local single-user install — sovereignty is the local pod, not row-level isolation across a group.
- Not its own transport. Federation and multiplayer go through Zed's collab/voip, not Kask's Matrix stack.

The full Kask documentation index (architecture, reference, explanation, research, plans, QA, and status) lives at [`kask/docs/README.md`](./kask/docs/README.md). The fork's divergence manifest and upstream-sync procedure live at [`DIVERGENCE.md`](./DIVERGENCE.md).

---

## How the Two Fit Together

Zed-Kask keeps Kask's hexagonal port surface and implements every adapter in one bridge crate, `kask/crates/kask_bridge`, so that Kask crates never depend on Zed crates — Zed-Kask depends on Kask, never the reverse. This is the governing invariant, enforced in CI by `kask/scripts/check-hkask-no-zed-deps.sh`.

The seam between the two sides is small and documented: twenty-three named divergence points (D1–D23, one of which — D10, the Kask panel — has since been removed) cover every edit to Zed's tree outside `kask/`. The first ten (D1–D10) wire the core integration — skill execution, the Curator agent, in-process MCP tools, the guard layer, keychain access, thread→memory ingestion, app-identity rename, the bridge, settings/credentials, and the (since-removed) Kask panel. The next ten (D11–D20) are targeted upstream fixes that zed-kask carries until upstream lands them: a `time` deprecation allow (D11), an env-var-name fix for OpenAI-compatible providers (D12), an OpenRouter output-budget fix (D13), a streaming-reveal timer interval (D14), bounded cursor-blink timers (D15), an app-menu rename + update item (D16), a GitHub-backed zed-kask update feed (D17), media/graph/kanban/portfolio/scenarios block rendering in markdown (D18), an update-progress popup (D19), and observed per-call USD cost in `TokenUsage` (D20). The remaining three (D21–D23) are kask-extension seams: the widget→agent compose-back injector (D21), block-reachability pins in `main.rs` (D22), and the `AgentPanelSiblingHost` visibility + worktree spawn wiring (D23). See [`DIVERGENCE.md`](./DIVERGENCE.md) for the full table and [`kask/docs/architecture/zed-host-architecture-plan.md`](./kask/docs/architecture/zed-host-architecture-plan.md) for the composition-root wiring.

---

## Installation

### Prebuilt binary (Linux x86_64)

The fastest path. Downloads the latest release archive — `zed-kask` plus all `hkask-mcp-*` binaries — verifies it against the published `SHA256SUMS`, and installs to `~/.local/bin`:

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
| `HKASK_VERSION`          | latest release     | Pin a release tag (e.g. `v0.32.0`) or `weekly`        |
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

### Developing Zed-Kask

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

The Kask side builds as part of the same workspace — the `kask/` crates are workspace members. Kask conforms to Zed's dependency versions where there are package conflicts; do not bump Zed's workspace deps to accommodate Kask.

### Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for ways you can contribute to Zed. For Kask-specific contribution (skills, MCP servers, the architecture), start at [`kask/docs/README.md`](./kask/docs/README.md) and [`kask/docs/`](./kask/docs/).

Per-crate documentation (tutorial, how-to, reference, explanation for each major crate) lives in the [Diataxis set](./kask/docs/diataxis/INDEX.md) — 43 artifacts across 11 cross-cutting crate sets.

---

## Releases

Releases are cut by pushing a `v*` tag (e.g. `v0.32.0`). The [`kask-release`](./.github/workflows/kask-release.yml) workflow builds the Linux x86_64 archive, generates `SHA256SUMS`, and publishes a GitHub Release with auto-generated notes. A weekly build runs on schedule at 08:00 UTC each Monday, force-moving the `weekly` tag.

Release assets:

- `zed-kask-x86_64-unknown-linux-gnu.tar.gz` — `zed-kask` + all `hkask-mcp-*` binaries
- `install.sh`, `install-common.sh`, `mcp-servers.txt` — source-build fallback scripts (tag-pinned, checksum-verified)
- `SHA256SUMS` — checksums for all of the above

See the [releases page](https://github.com/mdz-axo/zed-kask/releases) for all published versions.

---

## Licensing

Zed source code is licensed primarily under GPL-3.0-or-later, with Apache-2.0 components where marked.

License information for third party dependencies must be correctly provided for CI to pass.

We use [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) to automatically comply with open source licenses. If CI is failing, check the following:

- Is it showing a `no license specified` error for a crate you've created? If so, add `publish = false` under `[package]` in your crate's Cargo.toml.
- Is the error `failed to satisfy license requirements` for a dependency? If so, first determine what license the project has and whether this system is sufficient to comply with this license's requirements. If you're unsure, ask a lawyer. Once you've verified that this system is acceptable add the license's SPDX identifier to the `accepted` array in `script/licenses/zed-licenses.toml`.
- Is `cargo-about` unable to find the license for a dependency? If so, add a clarification field at the end of `script/licenses/zed-licenses.toml`, as specified in the [cargo-about book](https://embarkstudios.github.io/cargo-about/cli/generate/config.html#crate-configuration).

## Sponsorship

Zed is developed by **Zed Industries, Inc.**, a for-profit company.

If you’d like to financially support the project, you can do so via GitHub Sponsors.
Sponsorships go directly to Zed Industries and are used as general company revenue.
There are no perks or entitlements associated with sponsorship.
