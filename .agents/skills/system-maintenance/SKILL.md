---
name: system-maintenance
description: Weekly system maintenance for this machine (Ubuntu 26.04, Framework laptop) over the ~/system-maintenance/ scripts — run the health check, interpret warnings, preview and classify cleanups with probe-before-delete, execute the disposable class only, surface manual-review items as exact commands, hand off sudo-gated work, verify with reconciled measurements, and report a ledger. The scripts are the executables; this skill is the orchestration and judgment layer.
---

# System Maintenance

## Reference models

- **PDCA** — Shewhart (1939)/Deming; `onto_anchor` → derived `pdca_cycle` (operator ruling 2026-09-24). The maintenance pass is one bounded PDCA loop; its Check measures the pass, never the agent running it.
- **Feedback loop** — Wiener (1948)/Ashby (1956); `onto_anchor` → derived `cybernetic_feedback_loop` (operator ruling 2026-09-18). The staleness warning is the instrument that made missed weekly runs visible; a silent leg is a broken loop.
- **Maintenance strategy** — scheduled (calendar) vs condition-based (measured state) maintenance, per the reliability-engineering literature (Moubray, *RCM II*; ISO 14224 taxonomy). Coarse `onto_anchor` (5W1H core rung) — ruling requested; never a private definition.
- **Probe-before-action** — audit discipline (assertion → evidence → action); the project's own probe-before-delete rulings.

## Initial and target condition

- **Initial condition:** a "run the weekly maintenance" request (or a staleness/health warning to triage), the five `~/system-maintenance/` scripts, their logs, and live system state.
- **Target condition:** a completed pass — health check interpreted, dry-run classified, disposable class executed, manual-review items surfaced as exact commands, sudo work handed off (not asked for), measurements reconciled, ledger reported. Observable: health-check exit code, review-script output, the handoff block, and a df/du delta that closes against the deletion ledger.

## Managed scripts

The skill manages the script fleet at `~/system-maintenance/` (this machine's convention) — inventory, verification, execution, and updates. The fleet is versioned in place (`~/system-maintenance/.git`, `logs/` ignored — runtime data, read live by the staleness instrument); a script change without its commit is unmanaged. Fleet docs: `README.md` (machine manifest, scripts table, process).

| Script | Role | Runs | sudo? |
|---|---|---|---|
| `system-health-check.sh` | read-only health check; warns on disk, services, staleness | daily 08:30 timer + on demand | no |
| `maintenance-clean.sh` | cache cleanup; `--dry-run` default, `--run` deletes reviewed disposables only | weekly on demand | no |
| `maintenance-review.sh` | manual-review surface — every held-back item as an exact command | weekly on demand | no |
| `maintenance-update.sh` | package update (apt, snap, flatpak, SMART, rustup, uv) | weekly, the operator's terminal | yes |
| `maintenance-reminder.sh` | desktop reminder to run the update | Sat 09:00 timer | no |
| `setup-maintenance-timers.sh` | installer for the two timers | on demand | no |

## When to Use

- "Run the weekly/maintenance" requests, or the Saturday 09:00 reminder firing.
- Triage of health-check warnings (disk, services, staleness, failed units).
- Disk-space pressure on `/` or `~/.cache`.
- Script updates: a new trap, warning class, or review category surfaced by a pass or requested by the operator.

## When NOT to Use

- Code bugs or performance regressions — `diagnose`.
- Repo docs, corpus, media, portfolio data dirs — their own skills/servers.
- Executing sudo work — this skill hands it off, never asks for or handles the password.

## Instructions — the maintenance pass (one bounded PDCA loop, max 2 re-entries)

1. **Baseline (D).** Run `bash ~/system-maintenance/system-health-check.sh`; capture exit code, warnings, and the `~/.cache` size line. Record `df -h /` and `du -sh ~/.cache` baselines. Read `ls -t ~/system-maintenance/logs/update-*.log | head` — the staleness warning means *successful* runs (grep the log for completion), not log-file age.
2. **Triage (P).** Classify every warning and dry-run item into exactly one class:
   - **disposable** — `maintenance-clean.sh --run` territory (reviewed caches).
   - **manual-review** — held back by the scripts (HF models, datasets, podman volumes, toolchains, snap revisions, journals). Probe before any deletion (step 4).
   - **sudo-gated** — package updates, SMART, failed-unit resets, `/var/crash`. Handoff only (step 7).
   - **protected** — false positives and live infrastructure. Never delete without an operator ruling.
3. **Preview (D).** Run `bash ~/system-maintenance/maintenance-clean.sh --dry-run`. Read the full output — never a truncated display.
4. **Probe-before-delete (P, D gates).** For each manual-review item the operator has asked to delete:
   - **Usage:** `find <dir> -type f -atime -30 | wc -l` (0 = unused recently); open handles via `/proc/*/fd` scan; running services (`systemctl --user`, `ps`).
   - **Recoverability:** public assets re-download anonymously; private ones need the stored token; local-only artifacts (fine-tunes, volumes, audit trails) are **unrecoverable** — say so.
   - **References:** grep configs, clones, and pins before removing any toolchain or volume.
   - **False positives:** verify what the string actually is before deleting on a name match (e.g. `lrmoo`'s *"Dido and Aeneas"* is Purcell's opera, not the verification program).
   - **Live infra:** `elan` serves `kask/lean`'s pinned toolchain — it is not residue.
5. **Execute (D).** Run `maintenance-clean.sh --run` for the disposable class; delete probed manual-review items the operator named. Nothing else.
6. **Review surface (D).** Run `bash ~/system-maintenance/maintenance-review.sh` — every held-back item as an exact command, one place.
7. **Handoff (P).** Deliver the sudo block verbatim (reset-failed + crash-file removal + `maintenance-update.sh`). Never ask for the password; never run sudo from the agent terminal (it cannot authenticate — the Sep 5/12/19 logs are this failure).
8. **Verify (D).** Re-run the health check. Reconcile: `(df_before − df_after)` vs the deletion ledger's bytes, and `du` deltas per item. Any unexplained gap → re-enter step 2 (triage) with the gap named. **Trap:** `du` mid-churn (an app restarting) reads transient — re-measure via the health check, not a mid-batch `du`. Hardlink-shared caches (uv) free fewer blocks than `du` reports.
9. **Report (P→D).** Ledger table (item → evidence gate → bytes freed), the handoff block, protected items named, learnings.

10. **Improve the scripts (P→D) — the manager step.** The skill owns the scripts' lifecycle. When a pass surfaces a new trap, warning class, or review category — or the operator requests one — update the fleet through the script-update path:
    1. Edit the script (or fleet doc) in `~/system-maintenance/`.
    2. Verify: `bash -n <script>`; run the read-safe ones (health check, review, `--dry-run`) to confirm the change behaves.
    3. Commit in the scripts repo: `git -C ~/system-maintenance commit -m "<change> — <session evidence>"`.
    4. Update this SKILL.md in the same pass if the process, traps, or inventory changed; commit in zed-kask pathspec-limited.

    Triggers: a trap hit live (the transient `du` was one); a new warning worth instrumenting (the staleness warning was one); a new manual-review category (add it to `maintenance-review.sh` — the top-`~/.cache`-consumers section was one); an operator request.

### Convergence

After step 8, check in `lisp_eval`:
- form: `(if (and (= exit 0) (<= unexplained_gb 1)) (quote done) (if (>= iteration 3) (quote stop-and-report) (quote reenter-triage)))`
- env: `{ "exit": <health-check exit>, "unexplained_gb": < |df delta − ledger bytes| in GB >, "iteration": <1-based> }`

`done` → report (step 9). `reenter-triage` → step 2 with the named gap. `stop-and-report` → deliver with open items listed. The counts come from tools, never the model's own score.

## D/P labelling

| Step | Type | Oracle / critique |
|---|---|---|
| 1 Baseline, 3 Preview, 5 Execute, 6 Review, 8 Verify | D | script exit codes, `du`/`df`, `lisp_eval` reconciliation |
| 2 Triage, 4 Probe, 7 Handoff, 9 Report, 10 Improve | P | the operator vetoes classifications and trigger decisions; D gates (exit codes, byte arithmetic, `bash -n`, git commits) critique every claim |

## Constraints

- **Sudo is handed off, never asked for.** The agent terminal cannot authenticate; the scripts say "run manually" by design.
- **Probe before every deletion** — atime, open handles, recoverability, references, false positives. No deletion on a name match alone.
- **Protected classes** stand until an operator ruling: false positives, live infrastructure, unrecoverable local-only artifacts without explicit instruction.
- **Read full outputs** — truncated sweep/du displays hide results; count matches (`grep -c`) and consume the list.
- **The scripts stay the executables; this skill is their manager** — the inventory above, verification, execution, and the step-10 update path. It duplicates no script logic. Script changes commit in the scripts repo and update this skill in the same pass — a stale skill or stale fleet doc is active misinformation.
- **Timers are reminders, not executors** — the weekly timer reminds; the update runs in your terminal.

## Regression case

- The convergence form: `{exit 0, unexplained_gb 0.5, iteration 1}` → `done`; `{exit 1, unexplained_gb 0, iteration 1}` → `reenter-triage`; `{exit 0, unexplained_gb 4, iteration 3}` → `stop-and-report`.
- The class assignment is total: every dry-run line lands in exactly one of disposable/manual-review/sudo-gated/protected — a line in two classes is a triage defect.