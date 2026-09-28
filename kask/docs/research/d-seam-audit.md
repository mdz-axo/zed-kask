# D-seam audit ledger

This is a resumable **worklist**, not an assertion that every row below was audited. The live set comes only from the `## The divergence surface` table in `DIVERGENCE.md`; `bash kask/scripts/check-d-seam-audit-ledger.sh` checks identity coverage, not evidence quality. A seam may contain multiple behaviors with different verdicts. `pending` means no current-session verdict, `partial` means some behavior/record/verification remains open, and `audited` requires every behavior in the row to have evidence, verified pins, checked upstream status and aligned record. Do not promote `pending` from inherited row claims.

**Snapshot:** 2026-09-27; register `bfc60b318b` plus an in-flight D9 edit owned by another stream (not changed here). Locally available upstream ref `2c4bc2d7b2c5`; it is not a fetched proof of the latest upstream state. Initial batch: D12 (small upstreamable provider-neutral fix, one named pin); next prioritize D31/D32/D41/D66 (upstreamable candidates; ranking and pins not yet verified). D36 has a prior verdict in `193ac5b577`, not a fresh audit in this run. Rank later batches from evidence, not this candidate list: upstreamability → files × lines → pin health → live fire → operator pain.

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
| D12 | partial / audit owner (memory + live lookup) | upstreaming-candidate (retain; advisory); row aligned | `9cfd3a219f`; `api_compatible.rs:25-50`; named pin reran 1 passed; no live-fire proof; see D12 below | local `upstream/main@2c4bc2d7b2c5`: not adopted; remote freshness unverified | 2026-09-27 | `1456547b46` (audit), `6c55dd50e9` (citation), `bb0f9acbbb` (alignment) |
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
| D41 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
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
| D66 | pending / audit owner | not assessed | not run | unverified | — | uncommitted |
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

## D12 — API-compatible provider environment name

- **Spec and observation:** `9cfd3a219f` says the fork's alphanumeric uppercase form must align with Kask's credential env names; `DIVERGENCE.md` D12 is the claimed contract. At `crates/language_models/src/provider/api_compatible.rs:25-33`, the one public helper implements it; `:50` feeds it into `ApiKeyState::new` at `:63-67`. The local upstream snapshot at `2c4bc2d7b2c5` still uses `format!("{}_API_KEY", id).to_case(Case::UpperSnake)` at `:31-32`. `git log -S 'api_key_env_var_name_for' upstream/main -- crates/language_models/src/provider/api_compatible.rs` returned no adoption commit. **This is a snapshot, not proof of remote HEAD.** Counterexample: `OpenRouter` produces `OPENROUTER_API_KEY` in fork vs case-split `OPEN_ROUTER_API_KEY` upstream. The former is the Kask credential spelling. No environment key value was read.
- **Pins / live evidence:** the sole named row pin, `bash kask/scripts/cargo-test-nonzero.sh -p language_models test_api_key_env_var_name_kask_contract`, ran 1 test, 1 passed, 119 filtered out (2026-09-27). This is an oracle for the pure naming rule, **not** an integration test of the actual configured-key lookup. The available `Zed-Kask.log` and `.old` had zero D12 name/warn signature hits; the behavior has no dedicated fire counter, so live use is **not observable**, not disproven. `.rules` has no D12-specific trap; its general missing-credential rule applies downstream but does not prove this lookup ran.
- **G1 Exist:** removing the helper and restoring upstream's expression changes the env name for mixed-case IDs; the pure pin fails. **Evidence** for the semantic difference; **Hypothesis** that a particular live login would fail (no controlled live credential probe). Keep while upstream is different. **G2 Surface:** one public function, one production caller; no 7-function issue. **G3 Contract:** helper hides nontrivial stripping and case conversion; inlining would not reduce fork lines enough to eliminate the Zed-side behavior. No trait/config or new pass-through wrapper. Constraint: **Guardrail** for credential resolution, **Evidence** for fork/upstream output contrast; no prohibition-grade live outage claim. Security track found no credential exfiltration or access-control change in this behavior; other ten priority surfaces were not audited for this D12 batch. Architecture track: no dead adapter; UI track: not applicable (no UI renderer).
- **Advisory verdict:** `upstreaming-candidate` = retain + report upstream. No retirement or behavior change authorized. **Record alignment fixed:** after the D9 edit was committed, the D12 row and source/test comments were corrected together. Source `api_compatible.rs:22-24` no longer describes `.env` as active; test `:315-323` calls `fal.ai` a historical fixture rather than a configured provider. D12-F1/F2 are **fixed-verified** by source inspection and the rerun of the one named pin (1 passed, 119 filtered out); they were Guideline documentation findings, not behavior failures. This row stays `partial` because live credential lookup and grounded memory remain open.
- **Upstream issue draft (operator to review/send):** **Title:** Normalize API-compatible provider credential env names consistently. **Summary:** the API-compatible state derives `UpperSnake` from provider ID, turning a mixed-case ID such as `OpenRouter` into `OPEN_ROUTER_API_KEY`, while integrations using a concatenated uppercase ID expect `OPENROUTER_API_KEY`. Punctuation in IDs can also produce surprising env names. **Evidence:** upstream `api_compatible.rs:31-32` at `2c4bc2d7b2c5`; fork's `api_key_env_var_name_for` and `test_api_key_env_var_name_kask_contract` (one executed pass). **Discriminator:** run an integration test with an actual `OpenRouter` provider ID and a value set only under `OPENROUTER_API_KEY`, verify the key is discovered; a pure string test alone does not verify lookup. **Proposed fix:** decide and document the canonical normalization at the provider boundary, and keep existing `UPPER_SNAKE` consumers working or provide a deliberate migration for existing keys (no silent break). I can offer a PR with the controlled lookup regression once upstream maintainers agree on the public env-name contract. **Status:** draft only; no issue filed and no upstream adoption claimed.
- **Open items:** D12-F1/F2 row/comment alignment — `fixed-verified`, audit owner (row and comments now align; pin rerun); live lookup oracle and upstream issue submission — `operator-decision`, operator owns whether to invest in live controlled credentials and sending issue; stale remote ref — `delegated-tracked`, audit owner at next network-enabled refresh; `seam:D12` memory — `delegated-tracked`, audit owner when a matching episodic h_mem ID is available (recall returned none; `memory_insert` requires a cited evidence h_mem, and D36's ID does not support D12). No `.rules` edit made; no new rule is proposed for a one-off stale comment.
