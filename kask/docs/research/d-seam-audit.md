# D-seam audit ledger

This is a resumable **worklist**, not an assertion that every row below was audited. The live set comes only from the `## The divergence surface` table in `DIVERGENCE.md`; `bash kask/scripts/check-d-seam-audit-ledger.sh` checks identity coverage, not evidence quality. A seam may contain multiple behaviors with different verdicts. `pending` means no current-session verdict, `partial` means some behavior/record/verification remains open, and `audited` requires every behavior in the row to have evidence, verified pins, checked upstream status and aligned record. Do not promote `pending` from inherited row claims.

**Snapshot:** 2026-09-27; 62 live IDs after operator-retired D12 (see retirement note below). Locally available upstream ref `2c4bc2d7b2c5` (2026-09-24); it is not a fetched proof of the latest remote state. Earlier D12 audit was superseded by the operator's deprecation ruling; D41/D66 remain partial. D41 precedes D66 among the bounded one-file upstreamable fixes by larger measured file diff (89+4 vs D66 2+1 production plus 57-line pin) and historically reported settings pain. D31/D32 require a separate function-first, per-behavior batch; earlier size-first ranking is superseded. Read-only delegates reported passing pins, but no deletion counterfactual has been run for D32 and these two rows remain pending. D36 has a prior verdict in `193ac5b577`, not a fresh audit in this run. Rank future batches by evidence that the protected function was deprecated, removed, replaced or cheaply delivered without fork edits; only surviving current functions are ranked for upstreaming by maintenance cost and user impact.

| ID | State / owner | Behavior verdict | Evidence / pin status | Upstream status | Date | Audit commit |
| --- | --- | --- | --- | --- | --- | --- |
| D1 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D2 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D3 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D5 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D6 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D7 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D8 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D9 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |

| D14 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D16 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D18 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D20 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D21 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D23 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D24 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D25 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D26 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D27 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D28 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D29 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D31 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D32 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D33 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D35 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D36 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D37 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D39 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D40 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D41 | partial / audit owner (memory, Linux oracle) | upstreaming-candidate (retain; advisory), row aligned | `1ace61fe04`; named FakeFs pin 1 passed; current live fire unproved; see D41 below | local `upstream/main@2c4bc2d7b2c5`: not adopted; remote freshness unverified | 2026-09-27 | `a3e45db68c` |
| D42 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D43 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D44 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D45 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D46 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D51 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D52 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D54 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D55 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D56 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D57 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D58 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D59 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D60 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D61 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D62 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D63 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D64 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D65 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D66 | partial / audit owner (memory) | upstreaming-candidate (retain; advisory), row aligned | `fe1ec83eae`; named production-shape pin 1 passed; current live fire unproved; see D66 below | local `upstream/main@2c4bc2d7b2c5`: not adopted; remote freshness unverified | 2026-09-27 | `a3e45db68c` |
| D67 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D68 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D69 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D70 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D71 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D72 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D74 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D73 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D77 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D78 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D79 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D76 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
| D75 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |

## Retired D12 — requirement authority beats a passing pin

**Operator ruling, 2026-09-27:** the Kask-specific API-compatible provider env-var credential path had been deprecated for months. It is not a current function to preserve. The initial D12 review failed twice: (1) it checked that the old normalization still existed and passed its test instead of checking whether the *function* was still required; (2) after the operator said it was deprecated, the agent treated that decision as a hypothesis for the code to validate. The operator defines the functional requirement; tests only verify the implementation of that requirement. The old `upstreaming-candidate` verdict and unsent issue draft are withdrawn, not left as an open recommendation. History: `9cfd3a219f` added the normalization, `1456547b46`/`bb0f9acbbb` recorded the mistaken retention, and the retirement commit is cited in `DIVERGENCE.md` once it exists. The obsolete helper, its regression pin and the D12 row were removed; the current keychain/provider-slot and child credential injection paths were not deleted. **Do not resurrect D12 simply because upstream still supports an optional env fallback.**

**Audit correction for every remaining behavior:** begin with the operator's CURRENT supported user function and what would break if the fork code disappeared; explicitly ask whether that function was deprecated or replaced before inspecting tests. Existence and a passing test are not evidence that a retired function is wanted. Compare a direct upstream route and existing Kask-side route, assess maintenance cost, then test the least-divergent surviving implementation. If the requirement is revoked, prepare deletion and coupled cleanup, not an upstream issue. The old upstreamability-first ranking below is historical, not authority for the next batch. No `.rules` edit is made inline; this is a skill-process correction for algedonic review.

## D41 — config-file watcher lifetime

- **Purpose/origin:** `1ace61fe04` (2026-08-30) added the directory watch and its pin; its commit message explicitly names the atomic-replacement test. The D41 row reports an earlier live inotify/watch-loss investigation; that historical narrative is **asserted**, not re-enacted here. No D41-specific `.rules` trap. The current fork watches `path.parent()` (`crates/settings/src/settings_file.rs:257-262`) and filters event batches to the target path or Rescan (`:269-281`); local upstream `upstream/main@2c4bc2d7b2c5` still calls `fs.watch(&path, ...)` at `settings_file.rs:179` and reloads on every event. `git log -S 'fs.watch(&watch_root' upstream/main` found no adoption. Counterexample: replace `settings.json` by atomic rename, then modify the replacement inode; a file-inode watch can miss the second change while a parent-dir watch retains the path. A single provider function carries the fork logic, not an extra adapter.
- **Pin/live:** the sole named row pin, `bash kask/scripts/cargo-test-nonzero.sh -p settings test_watch_config_file_is_directory_anchored_and_ignores_siblings`, ran **1 passed / 49 filtered**. It asserts the FakeFs watch root, receives an `atomic_write`, and rejects a sibling event (`settings_file.rs:97-157`); it does **not** exercise Linux's real inotify rename-over. Available editor logs: 0 matching watch-failure lines in current/old logs; 0 current and 3 old `Settings file reloaded by watcher` lines. Those counts cannot show which file was watched, that a replacement happened, or that later changes were delivered. Current live-fire outcome: **unknown**, not disproven. Local upstream ref is dated 2026-09-24, not a fresh remote fetch.
- **G1 Exist:** reverting to upstream's file watch changes the watch root and admits the demonstrated FakeFs counterexample (**Evidence**), while the actual OS failure mechanism remains an unperformed live discriminator (**Hypothesis** in this batch). Retain. **G2 Surface:** no new public API; `watch_config_file` is the existing entry. **G3 Contract:** the parent selection plus path/Rescan filtering are behavior, not a pass-through wrapper, single-use trait or config copy. **Guardrail**: retain until upstream equivalence is verified. Security track: file-watch scope only, no full-fleet security claim; architecture track: no additional adapter to delete; UI layout track: not applicable. Gate A prior live, Gate B/Converge have no new uncited findings (empty findings, count 0).
- **Upstream issue draft (operator to review/send):** **Title:** Keep config-file watches alive across atomic replacement. **Summary:** `watch_config_file` watches the file path; on Linux an inotify watch attached to an old inode can stop delivering events after tempfile+rename, making later user edits invisible. **Evidence:** local upstream `settings_file.rs:179`, fork `:257-281`, test `test_watch_config_file_is_directory_anchored_and_ignores_siblings` (1 passing FakeFs test); the historical live failure is in D41, not reproduced now. **Discriminator:** real Linux integration probe: start the watcher, atomically replace the file, then externally edit the replacement and assert a second reload, with a sibling-edit negative control. **Proposed fix:** watch the containing directory, admit events for the target file/Rescan only, preserve symlink handling and cancellation; offer a PR with the real-fs regression after maintainers confirm the desired contract. **Status:** draft, not sent/adopted.
- **Open:** real-Linux oracle and issue submission — `operator-decision`, operator owns whether to invest in that probe and send the draft (if deferred, stale configuration can remain invisible in upstream; severe for settings reactivity but likelihood in a current deployed build unmeasured). Remote freshness and `seam:D41` evidence-cited memory — `delegated-tracked`, Z-K audit stream; `curator_memory_recall(seam:D41)` returned no evidence ID, and no unrelated memory ID may be cited. The audited row remains `partial` until that provenance gap closes.

## D66 — equal-start injection ordering

- **Purpose/origin:** `fe1ec83eae` (2026-09-17), commit message: "Order equal-start injections by ascending end to preserve layer invariants." `crates/language/src/syntax_map.rs:2066-2074` reverses the end tie-break for `BinaryHeap` so the shorter equal-start range is popped first; `SyntaxSnapshot::check_invariants` rejects a decreased end at equal start (`:1024-1034`). At local `upstream/main@2c4bc2d7b2c5`, `ParseStep::cmp` still uses `Ord::cmp(&range_a.end, &range_b.end)` (`syntax_map.rs:2072`), which orders the longer range first for the heap. `git log -S 'BinaryHeap must pop equal-start' upstream/main` returned no adoption commit; the implementation comparison, not that empty log, is decisive. `.rules` has no D66-specific trap.
- **Pin/live:** the sole named pin, `bash kask/scripts/cargo-test-nonzero.sh -p language test_equal_start_combined_and_pending_injections_are_ordered_by_end`, ran **1 passed / 187 filtered**. It reparses `$$x$$ <b>y</b>` with combined HTML and unavailable LaTeX at the same start and asserts `(0..5, latex)` before `(0..14, HTML)` (`syntax_map_tests.rs:999-1052`). This exercises production-shaped injection queries and the debug invariant; it does not establish that every user's Markdown triggers the failure. Available current and old editor logs had 0 `end decreased at equal start`, `layers out of order` or `Task polled after completion` matches. Current live fire: **not observed**, not disproven. Remote freshness unverified.
- **G1 Exist:** deleting the two-line tie-break correction restores the upstream ordering conflict with the syntax-layer invariant for the named counterexample (**Evidence** through the production-shape test and source ordering; a pre-fix rerun was not performed here). Keep pending upstream adoption. **G2 Surface:** no new public function/type; modifies private `Ord for ParseStep`. **G3 Contract:** no new wrapper, config or trait; the existing `Ord` implements behavior required by the heap/invariant, not speculative generality. **Guardrail** to keep; historical panic description in D66 remains an inherited assertion for this run. Security track: no new permission/data boundary; architecture track: no dead abstraction; UI layout: not applicable. Gate B/Converge empty findings, count 0.
- **Upstream issue draft (operator to review/send):** **Title:** Sort equal-start syntax injections by ascending end in the parse heap. **Summary:** `ParseStep::cmp` currently gives the longer equal-start range priority, while the layer invariant requires shorter-end first; combined and pending injections can collide. **Evidence:** local upstream `syntax_map.rs:2072`, fork `:2072-2074`, passing regression `test_equal_start_combined_and_pending_injections_are_ordered_by_end` and `check_invariants` at `:1024-1034`. **Discriminator:** add the Markdown-Inline HTML/LaTeX test on upstream without the comparator change; in a debug build it should panic or produce reversed ordering. **Proposed fix:** reverse the end comparator only, preserve depth/start/language tie-breaks; offer a minimal PR carrying the test. **Status:** draft, not sent/adopted.
- **Open:** upstream issue submission — `operator-decision`, operator owns the go/no-go; deferring leaves a confirmed comparator/invariant conflict in the local upstream snapshot, impact limited to colliding injections. Refresh remote upstream ref and obtain a matching episodic h_mem for `seam:D66` — `delegated-tracked`, Z-K audit stream (`curator_memory_recall(seam:D66)` returned none). Until memory is grounded and updated this row remains `partial`.
