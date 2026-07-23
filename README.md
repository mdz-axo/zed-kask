# Zed-Kask

[![Zed](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/zed-industries/zed/main/assets/badge/v0.json)](https://zed.dev)
[![CI](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml/badge.svg)](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml)

Welcome to **Zed-Kask** — a fork of [Zed](https://zed.dev), the high-performance, multiplayer code editor from the creators of [Atom](https://github.com/atom/atom) and [Tree-sitter](https://github.com/tree-sitter/tree-sitter), with [hKask](#what-hkask-is) — a minimal viable container for users and AI tools — running as a local single-user install inside the editor. Multiplayer and federation ride on Zed's existing collaboration capabilities; hKask's own Matrix transport is not carried over.

Zed-Kask is one clone, one build, one CI. Everything hKask lives under [`kask/`](./kask/) (additive — `git merge upstream/main` never touches it). Everything outside `kask/` is upstream Zed except the small set of seam edits documented in [`DIVERGENCE.md`](./DIVERGENCE.md).

---

## What hKask Is

hKask is a **minimal viable container for users and AI tools**. In Zed-Kask it runs as a **local, single-user install**: one user, one sovereign userpod, on the user's own machine. It is not an agent framework — there is no autonomous agent loop by default; the human is in the loop and skills escalate *to the user*, not away from them. The Curator is the system's cybernetic regulator, not an autonomous agent.

Three things sit between the user and a model:

1. **Skills** — PDCA loops that compose Jinja2 templates into Plan-Do-Check-Act cycles with convergence thresholds, gas budgets, and escalation. Where other systems give you a prompt, hKask gives you a *process*.
2. **MCP servers** — built-in Model Context Protocol servers (research, memory, codegraph, media, filesystem, regulation, …) exposed as tools through `rmcp`.
3. **Inference routing** — one router across multiple providers, with fusion, circuit breakers, and per-call gas accounting.

Everything else in hKask — the userpod, wallet, ledger, regulation, keystore — exists to keep the user's local session **sovereign**: per-userpod encrypted storage, OCAP dual gate, visibility gating.

### Federation is Zed's job

hKask's own Matrix/7R7 transport is **not** carried into Zed-Kask. Federation and multi-user communication ride on **Zed's existing collaboration capabilities** (channels, rooms, voice, contact sharing). The local hKask userpod stays sovereign; reaching other users happens through Zed's comms layer, not through hKask's federation plumbing.

### Sign-in is Zed's job too

There is no cloud server, no Kubernetes, no hKask OAuth, and no Admin/Member roles or invite flow. Users sign in with their **existing Zed account** — that single login gates the Zed-based features (communication, collaboration, voice). The local userpod is bound to the signed-in account at startup. hKask's identity layer is trimmed to the userpod/pod data structures only.

### What hKask Is Not

- Not an agent framework. No autonomous agent loop by default; skills escalate *to the user*.
- Not a multi-tenant cloud server. Zed-Kask is a local single-user install — sovereignty is the local userpod, not row-level isolation across a group.
- Not its own transport. Federation and multiplayer go through Zed's collab/voip, not hKask's Matrix stack.

The full hKask README (architecture diagrams, the four essential patterns, crate structure, current metrics, design philosophy) lives at [`kask/README.md`](./kask/README.md). The fork's divergence manifest and upstream-sync procedure live at [`DIVERGENCE.md`](./DIVERGENCE.md).

---

## How the Two Fit Together

Zed-Kask keeps hKask's hexagonal port surface and implements every adapter in one bridge crate, `kask/crates/kask_bridge`, so that hKask crates never depend on Zed crates — Zed-Kask depends on hKask, never the reverse. This is the governing invariant, enforced in CI by `kask/scripts/check-hkask-no-zed-deps.sh`.

The seam between the two sides is small and documented: ten divergence points (D1–D10) cover every edit to Zed's tree outside `kask/` — skill execution, the Curator agent, in-process MCP tools, the guard layer, sovereignty keys, thread→memory ingestion, app-identity rename, the bridge, settings/credentials, and the Kask panel. See [`DIVERGENCE.md`](./DIVERGENCE.md) for the table and [`kask/docs/specs/`](./kask/docs/specs/) for the per-seam specifications.

---

## Installation

On macOS, Linux, and Windows you can [download Zed directly](https://zed.dev/download) or install Zed via your local package manager ([macOS](https://zed.dev/docs/installation#macos)/[Linux](https://zed.dev/docs/linux#installing-via-a-package-manager)/[Windows](https://zed.dev/docs/windows#package-managers)).

> Zed-Kask is a fork under active development and does not yet publish its own installers. Build from source using the instructions below.

### Developing Zed-Kask

- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

The hKask side builds as part of the same workspace — the `kask/` crates are workspace members. hKask conforms to Zed's dependency versions where there are package conflicts; do not bump Zed's workspace deps to accommodate hKask.

### Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for ways you can contribute to Zed. For hKask-specific contribution (skills, MCP servers, the architecture), start at [`kask/README.md`](./kask/README.md) and [`kask/docs/`](./kask/docs/).

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