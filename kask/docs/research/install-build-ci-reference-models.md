---
title: "Install, Build, and CI Scripts — Reference Models from Published Practice"
audience: [architects, developers, agents]
last_updated: 2026-10-11
version: "1.0.0"
status: "Active"
domain: "Build and distribution"
mds_categories: [build, trust]
---

# Install, Build, and CI Scripts — Reference Models from Published Practice

This document records the reference models for zed-kask's **install, build,
and CI script surface** — what a Linux source-build installer for a Rust
workspace must do, drawn from published practice — and audits the tree
against them. It exists because the operator reported the installer
re-downloading the multi-GB `zk-ref-open` corpus a machine already held and
asked for the class-level treatment (2026-10-11): anchor the scripts to
best practice, and **check for, test for, and pin to** that practice, so
waste of that class cannot silently return. The instance fix (the
up-to-date gate) had already landed in v0.40.2 (`5b33f59c70`); this
document is the anchor, the conformance gate
(`kask/scripts/build/check-install-best-practices.sh`, wired into
`kask-invariants.yml`) is the enforcement, and its selftest
(`kask/scripts/check-install-best-practices-selftest.sh`, enumerated by
the "Gate selftests" CI step) proves the gate fails on violations.

> **Ontology note.** "Idempotence" anchors coarsely on the 5W1H
> interrogative ground (`onto_anchor`, 2026-10-11: no published or derived
> ontology concept matched; a ruling is available on request). The
> operative definitions below come from the cited practice documents,
> which are the published vocabulary of this domain.

## §1 Reference models

### 1.1 Installer design (Linux, Rust)

| # | Practice | Published anchor |
|---|----------|------------------|
| I1 | **Idempotence** — re-running a completed install is a no-op, never a re-fetch | rustup (`rust-lang/rustup`): re-running `rustup-init` over an existing install updates or no-ops; Debian maintainer scripts are required to be idempotent |
| I2 | **Consult machine state before expensive work** — presence *and* content status, before any multi-GB transfer | rustup's channel manifests; Homebrew's "already installed" checks; the operator's 2026-10-11 requirement states it directly |
| I3 | **Content-addressed identity** — a run identity binding the content digest is the "status" a presence check compares | rustup channel manifests carry per-component SHA-256; generated release installers embed "pinned version, channel, archive names, and digests" (1667 `release-install-script-body.ts`); our `run_id` + sha256-pinned `bundle.json` |
| I4 | **Resumable transfers** — an interrupted multi-hundred-MB download continues from the staged bytes, not from zero | `curl -C -` (curl(1): continue at the documented offset; `--retry` covers transient errors but restarts); apt's partial-download design |
| I5 | **Integrity verification, fail closed** — every fetched byte is hash-checked against a committed pin before use | sha256 pins in `bundle.json`; supply-chain baseline practice |
| I6 | **Atomic replacement** — a failed install preserves the working install; artifacts are staged then renamed | Determinate Systems `nix-installer` (plan/execute, revertible installs); our own `publish_binaries` staged-then-rename contract (pinned by `check-publish-paths.sh`); the `.new` + `mv` ETXTBSY pattern in `.rules` |
| I7 | **Loud failures, no silent fallback** — the last message before an abort tells the truth; degradation is surfaced | Google Shell Style Guide error handling; the project's own failure-signals rules (`.rules`) |
| I8 | **Pinned versions** — tags not floating branches; `--locked` builds; pinned toolchains | `HKASK_ALLOW_FALLBACK=false` default; `cargo --locked` (the Cargo Book's reproducibility guidance); `rust-toolchain.toml` |

### 1.2 Shell hygiene

The Google Shell Style Guide (bash for executables; quoting; error
handling; cleanup) with **ShellCheck** as the published enforcement
instrument — the gate runs `shellcheck -S warning` over every
`kask/scripts/build/*.sh`. Fail-fast stances are required of every build
script, with two tree-derived exceptions the gate recognizes structurally
(not by filename): a commented `set -uo pipefail` deviation (the build
sampler must survive bad samples) and the `HKASK_INSTALL_COMMON_LOADED`
double-source guard (a sourced library is governed by its sourcer's
errexit).

### 1.3 Build efficiency

sccache wired when present with a loud warning when absent (Depot /
WarpBuild sccache-in-CI guidance); the two-profile split (release for the
zed binary, release-mcp for the I/O-daemon MCP servers — D46); the
`min(nproc,16)` job cap; CI caching via `Swatinem/rust-cache` (which sets
`CARGO_INCREMENTAL=0` and keys on Cargo.lock / rust-toolchain content —
the published CI caching practice); the Lean toolchain cache keyed on
`hashFiles`.

### 1.4 CI process

The Cargo Book's continuous-integration guidance; content-keyed caches;
concurrency cancel for rapid agent push bursts; and the house rule that
**every gate has a selftest proving it fails on a violation** (the
`kask-invariants.yml` "Gate selftests" step enumerates
`kask/scripts/check-*-selftest.sh`).

## §2 The audit (IS — 2026-10-11, pre-fix snapshot)

Findings — every one fixed in the same change (§3):

| ID | Finding | Evidence (pre-fix `file:line`) | Severity / practice violated |
|----|---------|-------------------------------|------------------------------|
| F1 | Non-atomic database assembly: `: > "$database"` truncated a possibly-registered database *before* part concatenation — a failed assembly left a corrupt DB at the registered path while the registration still pointed there | `install.sh:636-642` | high — destroys a working install on failure (I6) |
| F2 | No physical resume: a partial part was `rm -f`'d and re-fetched from zero; `curl --retry` restarts transfers, it does not resume them | `install.sh:616-624` | high — the operator's exact waste class (I4) |
| F3 | Corpus failure was fatal to the whole install (bare call under `set -e`), contradicting the stage's documented "loud but not fatal to the product install" contract | `install.sh` (2026-10 lines 1029–1033; the script has since reorganized) vs the contract at `install.sh:545-547` | medium (I7 — code contradicted its own spec) |
| F4 | The CI rust branch logged "skipping Rust installation" and then returned 1 — the last message before an abort said "skipping" | `install.sh:169-172` | low (I7) |
| F5 | `cargo build` without `--locked` — a stale Cargo.lock silently re-resolves instead of failing loudly | `install.sh:196,260,272` | medium (I8) |
| F6 | `verify_installation` did not report corpus registration presence — the installer's own multi-GB artifact was unverified after install | `install.sh:727-825` | low (I2) |
| F7 | The hardcoded `HKASK_VERSION` fallback (the version the `curl \| bash` path pins) had no sync check — it drifts silently every release | `install.sh:87` | medium (I8) |

Conformances measured the same day (no action needed): `set -euo pipefail`
(`install.sh:27`); shellcheck clean at warning severity over all
`kask/scripts/build/*.sh`; pinned-tag default with opt-in-only fallback
(`install.sh:96-118`); `rust-toolchain.toml` as the canonical toolchain
pin (`install.sh:154-156`); sccache wired with a loud absence warning
(`install.sh:217-229`); the job cap (`install.sh:213-215`); staged atomic
binary publication (`install-common.sh` `publish_binaries`, pinned by
`check-publish-paths.sh`); sha256-verified downloads (`install.sh:614-632`);
the up-to-date gate itself (`install.sh:582-606`, from `5b33f59c70`);
honest `[installed]/[runtime-tested]/[skipped]` verification classes
(`install.sh:727-815`); idempotent system deps via `script/linux`
(`install.sh:135-137`); CI rust-cache + `CARGO_INCREMENTAL=0`, the Lean
cache keyed on `hashFiles`, concurrency cancel, and the gate-selftest
enumeration (`kask-invariants.yml:147-149, 231-243, 250-254, 264-267`).

## §3 What landed (2026-10-11)

- **The corpus stage moved from `install.sh` into `install-common.sh`**
  (the `uninstall_hkask` testability precedent: `install.sh` runs
  `main "$@"` and is not sourceable). Landed state:
  `install-common.sh:917-1140` — constants at `:917`, the
  `corpus_registered_field` lookup helper at `:929` (one home for the
  federated-registration read, shared by the gate and verification),
  `install_corpus_bundle` at `:940` with the up-to-date skip at `:982`,
  physical resume at `:1002-1010` (`curl -fL -C -`, partial kept on
  transport failure, the hash as arbiter, one clean re-download on
  corrupt bytes), and atomic assembly at `:1047-1055` (assemble to
  `$db.assembling`, rename into place; an injected `cat` failure
  preserves the registered database).
- **`install.sh`**: the pointer comment at `:535`; the truthful CI rust
  failure at `:174`; `--locked` on all three cargo builds at
  `:200,264,276`; corpus registration presence reported by
  `verify_installation` at `:646-648`; the non-fatal corpus dispatch at
  `:882` matching the documented contract.
- **The conformance gate** (`build/check-install-best-practices.sh`):
  a static section (shellcheck, fail-fast stances, `--locked`, version
  sync, fallback default, non-fatal dispatch, registry integrity walk)
  and a behavioral corpus suite — seven cases against a `file://`
  fixture exercising the real `install_corpus_bundle`: fresh install,
  idempotent same-run skip, different-run fetch, observed `-C -` resume
  (through a recording curl wrapper), corrupt-partial recovery, transport
  failure preserving the working registration, and atomic assembly under
  an injected `cat` failure.
- **The selftest** (`check-install-best-practices-selftest.sh`): nine
  probes — seven static mutations (each caught by name), one behavioral
  aliveness probe (neutralizing the up-to-date gate fails the suite), and
  a pristine-fixture pass.
- **CI**: the gate runs in the `kask-invariants.yml` check job (after the
  duplication-ratchet step installs jq); the selftest is enumerated by
  the "Gate selftests" glob.

## §4 Residual risks (named, not enforced)

- The `curl | bash` usage line fetches the script from `main` while the
  script clones a pinned tag — the running gate logic and the cloned
  bundle data can skew across a release boundary. The version-sync check
  (A4) bounds the most damaging case (a stale default pin), but the skew
  itself is not enforced.
- The behavioral suite's `file://` fixture exercises curl's file-scheme
  failure mode (exit 37), not HTTP 416/404. The code treats curl 33/22
  as "let the hash decide" and everything else as transport failure,
  which covers the HTTP cases by design, but the pin is file://-shaped.
- Docs count: this document was added when `kask/docs` held 83 files (cap 85,
  working target 75) — condensation direction per
  `DOCUMENTATION_STANDARDS.md` §3 (measured count lives in the
  [`docs README`](../README.md) corpus-size line).
