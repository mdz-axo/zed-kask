# Plan: Add `typos` and `buf` CI jobs to `kask-ci.yml`

## Status

Proposed — not yet applied. Generated 2026-08-17 during a CI gate sweep that
found `./script/clippy` runs `typos` and `buf` locally (when the tools are
installed and `GITHUB_ACTIONS` is unset) but neither tool runs in CI.

## Findings (ground truth from local run)

### `buf` — ready to add now

```
$ buf lint crates/proto/proto     # EXIT 0
$ buf format --diff --exit-code crates/proto/proto  # EXIT 0
```

Both pass clean. `crates/proto/proto/buf.yaml` exists and is maintained.

### `typos` — NOT ready to add now

```
$ typos --config typos.toml   # 3376 findings, EXIT != 0
```

Breakdown by top-level directory:

| Dir       | Findings | Nature |
|-----------|----------|--------|
| `tasks/`  | 3048     | Esports/research data — proper nouns (team names, player handles, locales). ~90% false positives. |
| `kask/`   | 240      | Mix: real typos (`unparseable`, `Celcius`, `appliable`, `clonable`) + proper nouns (`Beizer` = Boris Beizer, testing theorist) |
| `crates/` | 86       | Mostly upstream Zed code (not our scope) |
| `.agents/`| 19       | `Beizer` (correct — refers to the person) |
| `assets/` | 16       | Data files |
| `docs/`   | 4        | Real typos |

**Most common real source typo:** `unparseable` → `unparsable` (65 + 12 occurrences across `hkask-mcp-companies`, `hkask-mcp-swarm`, `hkask-mcp-codegraph`, `hkask-mcp-prediction-markets`, `hkask-inference`, `hkask-mcp-training`, `hkask-mcp-research`).

**Legitimate proper nouns to allowlist in `typos.toml`:**
- `Beizer` (Boris Beizer, software testing theorist — author of *Software Testing Techniques*)
- `ratatui` (a real Rust crate name)
- `OT` (Optimality Theory, linguistics — used in `pragmatic-semantics` skill)
- `strat` (a real variable name in `hkask-mcp-research`)

## Remediation sequence (do in this order, separate PRs)

### PR 1: Exclude `tasks/` and data dirs from typos scope

`tasks/` contains research artifacts with esports team names, player handles,
and locale strings that are not typos. Add to `typos.toml` `[files] extend-exclude`:

```toml
    # Research artifacts contain proper nouns (team names, player handles,
    # locales) that typos flags as false positives. These are data, not prose.
    "tasks/",
    "assets/",
```

This alone drops ~3064 findings.

### PR 2: Fix real source typos

The highest-frequency real typo is `unparseable` → `unparsable` across these
files (non-exhaustive — run `typos` after PR 1 to get the precise list):

- `kask/mcp-servers/hkask-mcp-companies/src/financial_model.rs`
- `kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs`
- `kask/mcp-servers/hkask-mcp-codegraph/src/codegraph/types.rs`
- `kask/mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/schema.rs`
- `kask/mcp-servers/hkask-mcp-prediction-markets/src/types.rs`
- `kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_portfolio.rs`
- `kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs`
- `kask/mcp-servers/hkask-mcp-training/src/dataset.rs`
- `kask/mcp-servers/hkask-mcp-research/src/research/types/ranking.rs`
- `kask/crates/hkask-inference/src/chat_protocol.rs`

Other real typos to fix:
- `Celcius` → `Celsius` (`kask/scripts/monitor-runpod-training.sh`)
- `appliable` → `applicable` (`kask/docs/architecture/AGENT_SYSTEM_PROMPT.md`)
- `clonable` → `cloneable` (`kask/mcp-servers/hkask-mcp-corpus/src/services/convert.rs`)
- `occured` → `occurred`
- `indicies` → `indices`
- `fradulent` → `fraudulent`
- `consitution` → `constitution`
- `companys` → `companies`
- `parlimentary` → `parliamentary`
- `massachussetts` → `massachusetts`
- `Isreali` → `Israeli`
- `Governer` → `Governor`
- `Grammer` → `Grammar`
- `calibraton` → `calibration`
- `intger` → `integer`
- `visibilty` → `visibility`
- `invokable` → `invokable` (check — may be correct as-is)

### PR 3: Extend `typos.toml` for legitimate proper nouns

```toml
[default.extend-words]
# Boris Beizer — software testing theorist (Software Testing Techniques, 1990).
# Used across bug-hunt skill and test-harness docs.
Beizer = "Beizer"
beizer = "beizer"

# ratatui is a real Rust crate (terminal UI framework).
ratatui = "ratatui"

# OT = Optimality Theory (linguistics), used in pragmatic-semantics skill.
OT = "OT"
```

### PR 4: Add the CI jobs

Only after PRs 1-3 land and `typos --config typos.toml` exits 0 locally, add
to `.github/workflows/kask-ci.yml`:

```yaml
  typos:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@v2
        with:
          tool: typos
      - run: typos --config typos.toml

  buf:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: bufbuild/buf-setup-action@v1
      - run: buf lint crates/proto/proto
      - run: buf format --diff --exit-code crates/proto/proto
```

`buf` can be added independently of `typos` (it passes clean today). The
`typos` job must wait for PRs 1-3.

## Why not add `typos` to CI now?

Adding a gate that is known to fail with 3376 findings violates the
"advertised invariants must point to the enforcement line" rule in reverse:
a gate that always fails is as broken as a gate that always passes — both
suppress the signal. The remediation sequence above is the disciplined path.
