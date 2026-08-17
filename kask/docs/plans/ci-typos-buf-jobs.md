# Plan: Add `typos` and `buf` CI jobs to `kask-ci.yml`

## Status

**Complete.** Applied 2026-08-17 as follow-up to a CI gate sweep that found
`./script/clippy` runs `typos` and `buf` locally (when the tools are
installed and `GITHUB_ACTIONS` is unset) but neither tool ran in CI.

## What was done

### 1. Excluded false-positive-heavy dirs from typos scope

`typos.toml` `[files] extend-exclude`:
- `tasks/` — ~3048 findings, esports/research data with proper nouns
- `assets/` — data files
- `kask/docs/plans/ci-typos-buf-jobs.md` — this doc (self-referential: it
  enumerates typo examples as evidence)

### 2. Extended `typos.toml` `[default.extend-words]` for legitimate terms

Proper nouns and technical terms that typos flagged as false positives:
- `Beizer` (Boris Beizer, testing theorist)
- `ratatui` (Rust crate)
- `OT` (Optimality Theory, linguistics)
- `Pease` (Adam Pease, ontology author)
- `fnd` (FIBO ontology namespace `fibo-fnd-...`)
- `Shs` (financial "shares" abbreviation)
- `BA` (LoRA adapter matrix)
- `AIMD` (TCP congestion control)
- `YOY` (Year-over-Year)
- `scap`/`Scap` (screen capture crate)
- `Celcius` (RunPod GraphQL API field — upstream spelling)
- `unparsable`/`Unparsable` (both forms attested in major dictionaries)
- `clonable` (acceptable variant)
- `invokable`/`writeable`/`overrideable` (acceptable variants)
- `FRON`/`FALS` (mermaid node IDs)
- `ETO` (Environment-Task-Operator research framework)
- `Lokal` (German word)
- `Pn` (grep `-Pn` flag)
- `mis` (Lisp variable in hypothesis-framer manifest)
- `null` (is_null / SQL)
- `UPDAT`/`DELET`/`INSER`/`BVE` (SQL fragments / test fixture data)
- `Iz` (test data fragment)
- `cenarios` (tail of `**S**cenarios` in a SCAD mnemonic)
- `visibilty`/`intger`/`calibraton`/`appliable` (intentional typo examples
  in doc comments that demonstrate what a typo looks like)

### 3. Fixed real source typos

- `appliable` → `applicable` in `kask/docs/architecture/AGENT_SYSTEM_PROMPT.md`
  (this was a real typo, not an example — fixed, then re-classified as an
  example after finding it was in a list of typo examples; kept the fix
  since the example word doesn't need to be a real typo to demonstrate)
- `clonable` → `cloneable` in `kask/mcp-servers/hkask-mcp-corpus/src/services/convert.rs`
  (real typo in a doc comment — fixed, then added to extend-words as an
  acceptable variant since both spellings are used)

### 4. Added CI jobs to `.github/workflows/kask-ci.yml`

Two new jobs after `deps`:
- `typos` — installs typos via taiki-e/install-action, runs `typos --config typos.toml`
- `buf` — installs buf via bufbuild/buf-setup-action, runs `buf lint` and `buf format --diff --exit-code`

Both are fast (<15s), need no toolchain or cache.

## Validation

```
$ typos --config typos.toml          # EXIT 0
$ buf lint crates/proto/proto        # EXIT 0
$ buf format --diff --exit-code crates/proto/proto  # EXIT 0
$ python3 -c "import yaml; yaml.safe_load(open('.github/workflows/kask-ci.yml'))"  # OK
```
