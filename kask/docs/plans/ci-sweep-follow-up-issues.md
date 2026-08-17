# Follow-up Issues from CI Gate Sweep

Generated 2026-08-17 after the CI gate sweep and remediation. These are
open questions and improvements a human should triage. They are not
blocking — the sweep passed and the three regressions are fixed — but
they represent residual gaps worth resolving.

**Status (updated 2026-08-17):** All 7 issues resolved. See per-issue notes.

## 1. Pin `typos` and `buf` versions in CI jobs

**Where:** `.github/workflows/kask-ci.yml` (the `typos` and `buf` jobs added in the sweep)

**Problem:** The `typos` job uses `tool: typos` (unpinned) and the `buf` job uses `bufbuild/buf-setup-action@v1` (unpinned major). A new release of either tool could surface new findings and break CI without a code change. The `deps` job already follows the pinning discipline — its comment says *"taiki-e/install-action@v2 fetches the latest release tag by default; bump this explicitly when upgrading."*

**Fix:** Pin `typos` to a specific version (e.g. `tool: typos@v1.49.0`) and pin `buf-setup-action` to a specific commit SHA. Bump explicitly when upgrading, same as `cargo-machete@0.9.2`.

**Resolved (2026-08-17):** `typos` pinned to `typos@v1.49.0`; `buf-setup-action` pinned to commit `a47c93e0b1648d5651a065437926377d060baa99` (v1.50.0). Both in `.github/workflows/kask-ci.yml`.

## 2. Consider removing the `GITHUB_ACTIONS` guard from `script/clippy`

**Where:** `script/clippy:18-30`

**Problem:** The local-only branch runs `cargo machete` + `typos` + `buf` when `GITHUB_ACTIONS` is unset. Now that `typos` and `buf` have dedicated CI jobs (and `cargo machete` is gate 15), the guard is arguably dead surface: the local-only checks are all covered by standalone CI jobs. The guard was a workaround for missing CI jobs; now that the jobs exist, the workaround is redundant.

**Fix (essentialist — take away):** Remove the `GITHUB_ACTIONS` guard and the local-only block from `script/clippy`. Let `script/clippy` be clippy-only. The other checks run as dedicated CI jobs. This is ~12 lines deleted, zero behavior lost.

**Counter-argument:** The local-only branch lets developers run all checks with one command (`./script/clippy`) without waiting for CI. If that workflow is valued, keep the guard but add a comment explaining that the CI jobs are the source of truth and the local branch is a convenience aggregator.

**Resolved (2026-08-17):** Essentialist option applied. The `GITHUB_ACTIONS` guard and the local-only `cargo machete` + `typos` + `buf` block were removed from `script/clippy`. The script is now clippy-only; the other checks run as dedicated CI jobs (`deps`, `typos`, `buf`).

## 3. The mcp-tool-tests ratchet quota prevents growth but doesn't force shrinkage

**Where:** `kask/scripts/check-mcp-tool-tests.sh` (the `ALLOWLIST_MAX=9` quota added in the sweep)

**Problem:** My `ALLOWLIST_MAX=9` quota prevents the allowlist from *growing* beyond 9, but it doesn't force it to *shrink*. A ratchet that prevents growth but doesn't force shrinkage is still a one-way clutch — just in the opposite direction. The allowlist has been at 9/9 since inception and the quota alone won't move it.

**Fix options (ranked):**
1. **Stale-entry check:** Add a date-stamp to each allowlist entry (`# added YYYY-MM-DD`) and emit a warning (not a failure) for entries older than 90 days. This makes the stall *visible* in CI output.
2. **Time-decaying cap:** Lower `ALLOWLIST_MAX` by 1 every quarter (e.g., via a date check in the script). This forces progress: if no entry is removed by the deadline, the gate fails.
3. **Quota + ownership:** Require each allowlist entry to name an owner (team or person) who is responsible for either adding the test or justifying the deferral.

Option 1 is the smallest change and the most honest — it makes the stall visible without forcing a deadline that may not be resourced.

**Resolved (2026-08-17):** Option 1 applied. Each allowlist entry now carries an `added YYYY-MM-DD` date field (real data, not a comment — bash strips inline comments from array values). Entries older than `STALE_DAYS` (default 90, env-overridable) emit a `::warning::` in CI output. The gate still exits 0 (warnings, not failures) — visibility without a forced deadline. The 9 entries dated 2026-07-17 will start emitting warnings on 2026-10-15.

## 4. Add a security audit log entry for the `check-convergence-weights.sh` deletion

**Where:** `kask/security/audit-log/2026-07-22-baseline.md:107`

**Problem:** The baseline audit log lists `check-convergence-weights.sh ✓` as a verified CI gate. I deleted the gate (its target template was replaced by `compute.rs` Kata primitives) but didn't add a new audit entry noting the deletion and rationale. A future auditor reading the baseline will look for the gate and be confused.

**Fix:** Add a new audit log entry (e.g., `kask/security/audit-log/2026-08-17-convergence-weights-deletion.md`) noting: the gate was deleted because `convergence-check.j2` templates were replaced by deterministic Kata primitives in `compute.rs:540-550`; the weight-sum invariant survived the migration and is now enforced by `hkask-templates/tests/evaluate_weight_sums.rs`; the workflow step was removed from `kask-ci.yml`.

**Resolved (2026-08-17):** Audit entry created at `kask/security/audit-log/2026-08-17-convergence-weights-deletion.md`. The baseline entry at `2026-07-22-baseline.md:107` now carries a `(DELETED 2026-08-17 — see ...)` superseded marker cross-referencing the new entry.

## 5. Is there a telemetry consumer filtering on `reg.skill.manifest.unparseable`?

**Where:** `kask/crates/hkask-templates/src/skill_loader.rs:319`, `kask/crates/hkask-types/src/event.rs:305`

**Problem:** The codebase standard is `unparsable` (45 instances in prose) but 2 telemetry span operation names use `unparseable` (`manifest_unparseable`, `reg.skill.manifest.unparseable`). I excluded these 2 files from typos to keep the prose standard enforceable without breaking the telemetry contract. But if no telemetry consumer actually filters on these operation names, renaming to `unparsable` would let me drop the exclusion entirely.

**Research question:** Is there a dashboard, alert, or log query that filters on `reg.skill.manifest.unparseable`? If not, rename both to `unparsable` and remove the `extend-exclude` entries from `typos.toml`. If yes, the exclusion is the correct trade-off and should stay.

**Resolved (2026-08-17):** Research answered: no consumer filters on these names. A repo-wide grep for `manifest_unparseable` / `reg.skill.manifest.unparseable` found only the two emit sites (`skill_loader.rs:319`, `event.rs:305`) and the `typos.toml` exclusion comments — no dashboard query, alert rule, log filter, or test assertion keys on them. Both renamed to `unparsable` (`manifest_unparsable`, `reg.skill.manifest.unparsable`); the two `extend-exclude` entries in `typos.toml` were dropped. The `unparseable`/`Unparseable` allowlist entries remain (still used by the codegraph `Complexity::Unparseable` enum variant and upstream Zed code).

## 6. Audit every gate's "0 violations" semantics for silent disconnection

**Where:** All `kask/scripts/check-*.sh` gates

**Problem:** I found `check-convergence-weights.sh` was dead because its output was `0 checked, 0 skipped — no weights found` — a success message that actually meant "sensor disconnected." Are there other gates with the same failure mode? Specifically:
- `check-reg-creep.sh` — output was `all reg.* targets registered (exact match)`. Does it check against a real registry, or does it verify internal consistency? If the registry is empty, "exact match" is trivially true.
- `check-forecast-conformance.sh` — output was `21 primitives, 21 contract references`. Is 21 the right number, or has it drifted? If the primitive count and the contract count are both 0, "21 = 21" is trivially true.
- `check-convergence-weights.sh` — confirmed dead, deleted, replaced by `evaluate_weight_sums.rs`.

**Fix:** For each gate, add a self-test that injects a synthetic violation (like `check-kali-regressions-selftest.sh` does) and confirms the gate catches it. A gate that can't fail is a gate that doesn't enforce.

**Resolved (2026-08-17):** Self-tests added for the two gates flagged in the issue:
- `check-reg-creep-selftest.sh` — injects a synthetic unregistered `reg.selftest.fake` target into a temp source tree and asserts the gate exits 1 with `UNREGISTERED`. Also pins the empty-scan path (exit 0 with `no reg.* targets found`) so a future change can't invert it. The gate was made parameterizable (`SCAN_DIRS`, `REGISTRY` env vars) so the self-test can redirect it at a temp tree without touching real source.
- `check-forecast-conformance-selftest.sh` — injects synthetic orphan (primitive not in contract), dangle (contract ref not in lib), and empty-parse (no `#[must_use]` pub fns) violations, asserting the gate exits 1 with the expected keyword for each. The gate was made parameterizable (`LIB`, `CONTRACT`, `SECTION` env vars).

Both self-tests are wired into CI as dedicated jobs (`reg-creep-selftest`, `forecast-conformance-selftest`) alongside the existing `kali-regressions-selftest`. The remaining gates (`check-string-errors.sh`, `check-mcp-servers.sh`, `check-version-sync.sh`, `check-hkask-no-zed-deps.sh`, `check-unsafe-forbid.sh`, `check-skill-span-namespace.sh`, `check-reg-canonical.sh`, `check-lora-training-regressions.sh`, `check-mcp-tool-tests.sh`) do not yet have self-tests — they are lower-priority because their failure modes are grep-based (a broken regex would surface immediately on a real violation) rather than count-based (the silent-disconnection class). Adding self-tests for them is future work.

## 7. Periodic typos-allowlist review

**Where:** `typos.toml` `[default.extend-words]` and `[files] extend-exclude`

**Problem:** I added ~30 `extend-words` entries and 5 `extend-exclude` paths. Each exclusion is a potential future false-negative. There's no mechanism to review these — they'll accumulate. A new typo that matches an allowlisted word would be silently missed.

**Fix:** Add a periodic review (e.g., quarterly) that re-runs typos without the allowlist and reviews new findings. Or add a CI job that runs typos without the allowlist on a schedule (not per-PR) and reports new findings as warnings, not failures.

**Resolved (2026-08-17):** Weekly scheduled CI job `typos-allowlist-audit` added to `.github/workflows/kask-ci.yml`. Runs on cron `0 9 * * 0` (Sundays 09:00 UTC), gated on `github.event_name == 'schedule'` so it never fires on push/PR. The job runs typos and surfaces findings as job output with `continue-on-error: true` (non-blocking). The value is the delta between runs — new findings may indicate a real typo that matches an allowlisted word. The job header explains the triage protocol.
