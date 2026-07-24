# Token Efficiency: Context Injection & Compaction Optimization

## Overview

Goal: maximize semantic information content and saliency of injected context per billed token, modeled on Kilocode's proven patterns and reusing the local `hkask-condenser` where it fits.

The Zed agent already wires provider-side prompt caching (Anthropic `cache_control` automatic mode, OpenAI `prompt_cache_key`), so the cost problem is NOT "caching is off." It is four orthogonal inefficiencies, ordered here by leverage-to-effort ratio:

1. **Compaction trigger counts cache reads as full tokens** — forces premature summarization of nearly-free cached context. (1-line fix, highest leverage)
2. **System prompt re-rendered every turn with no baseline discipline** — any tool/skill/rules change busts the entire prefix cache. (Medium effort, high leverage)
3. **Single cache breakpoint on the conversation tail** — misses the latest-user-message breakpoint that keeps intra-turn tool loops cheap. (Small fix, medium leverage)
4. **No tool-output spillover or per-result truncation** — large tool outputs persist verbatim across every subsequent request. (Medium effort, medium leverage)
5. **Compaction summary regenerated from scratch, not iteratively refined** — loses hard-won context on each compaction. (Medium effort, high semantic leverage)

## Architecture Decisions

### AD-1: Compaction trigger uses billed tokens, not gross tokens
Introduce `billed_input_tokens(usage) = input_tokens + cache_creation_input_tokens` (excludes `cache_read_input_tokens`). Use it for the compaction threshold. Keep `total_input_tokens` for the context-window-overflow warning (the model still consumes the full window). Two distinct concerns, two distinct functions.

### AD-2: System-prompt baseline digest (lightweight Kilocode SystemContext adaptation)
Full Kilocode `SystemContext` with DB-persisted epoch/snapshot/revision is too large a change for a first pass. Instead: compute a digest (hash) of the rendered system prompt + tools list; store it on the `Thread`; only re-render when the digest changes. This gives byte-stability within a turn (already true) AND across turns when nothing changed, and makes cache-bust events observable. A future iteration can add the full epoch/snapshot system if digest-based detection proves insufficient.

### AD-3: Second cache breakpoint at the latest user message
Mirror Kilocode's three-breakpoint default (tools / system / latest-user-message). Zed already places the long-TTL breakpoint on tools+system (Anthropic automatic mode) and the short-TTL top-level breakpoint on the conversation tail. Add an explicit `cache: true` on the latest user message so the `any_message_wants_cache` gate is satisfied by the right message, and the conversation-tail breakpoint lands on the user message rather than whichever message happens to be last (which during a tool loop is a tool result).

### AD-4: Tool-output truncation via the local `hkask-condenser` `rtk_style` algorithm
The kask condenser's `RtkStyleAlgorithm` already does head/tail preservation with an ontology-aware split ratio — it's a Rust-native equivalent of Kilocode's `ToolOutputStore.boundedPreview`. Reuse it for terminal output and large file reads that exceed a byte/line budget. Add an on-disk spillover path (Kilocode pattern) so the model can re-read the full output by path. This keeps the condenser as a complementary tool rather than replacing Zed's compaction.

### AD-5: Iterative compaction summary refinement
Mirror Kilocode's `buildPrompt`: when a prior `CompactionInfo::Summary` exists, feed it back wrapped in `<previous-summary>` with the instruction "Preserve still-true details, remove stale details, and merge in the new facts." Use a structured Markdown template (Goal / Constraints / Progress / Key Decisions / Next Steps / Critical Context / Relevant Files) instead of the current free-form `COMPACTION_PROMPT`. This is a prompt-only change — no new types.

## Phased Task List

### Phase 1 — Foundation (cheapest, highest leverage)

- [ ] **T1: Billed-token compaction trigger** — Introduce `billed_input_tokens`, use it in `compaction_message_target_ix`. Keep `total_input_tokens` for the overflow warning.
- [ ] **T2: Second cache breakpoint at latest user message** — In `build_request_messages_until`, set `cache: true` on the latest user message in addition to the last message.

**Checkpoint 1**: `./script/clippy` clean; `test_prompt_caching` and compaction tests pass; new test asserts high `cache_read_input_tokens` does NOT trigger compaction.

### Phase 2 — System prompt stability

- [ ] **T3: System-prompt digest on Thread** — Hash the rendered system prompt + sorted available_tools; store digest on `Thread`; skip re-render (reuse cached string) when digest matches.
- [ ] **T4: Cache-bust telemetry** — Emit a telemetry event when the system-prompt digest changes across turns, with the changed field (tools/skills/rules/date) if derivable.

**Checkpoint 2**: `./script/clippy` clean; system prompt tests pass; new test asserts digest stability across turns with no changes, and bust on tool add.

### Phase 3 — Compaction quality

- [ ] **T5: Structured compaction template** — Replace `COMPACTION_PROMPT` with a Kilocode-style fixed-section Markdown template (Goal / Constraints / Progress / Key Decisions / Next Steps / Critical Context / Relevant Files).
- [ ] **T6: Iterative summary refinement** — When a prior `CompactionInfo::Summary` exists, feed it back in `<previous-summary>` with the "preserve still-true, remove stale, merge new" instruction. Carry the prior `recent` tail into the new compaction context.

**Checkpoint 3**: `./script/clippy` clean; compaction tests updated to assert the new template and refinement path; `test_compaction_usage_counts_toward_cumulative_usage` still passes.

### Phase 4 — Tool-output bounding (complementary condenser)

- [ ] **T7: Tool-output truncation hook** — Add a bounded-preview step for terminal tool results exceeding a byte/line budget, using `hkask-condenser`'s `RtkStyleAlgorithm` or a local equivalent. Write full output to a temp file; replace in-message content with head/tail preview + marker path.
- [ ] **T8: Spillover cleanup** — Add retention/cleanup for the spillover files (hourly sweep, 7-day TTL) or tie to thread lifetime.

**Checkpoint 4**: `./script/clippy` clean; new test asserts a 100KB terminal output is truncated to the budget with a valid spillover path; full output recoverable by reading the path.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Excluding cache reads from compaction trigger could let the context window genuinely overflow (model rejects) | High — request failure | Two-threshold approach: compact on billed tokens (cost), hard-warn/hard-cap on total tokens (window). T1 includes the overflow-warning path. |
| System-prompt digest skips a needed re-render (e.g., date rollover) | Medium — stale date in prompt | Include `date` in the digest; date change correctly busts. Digest is over the full rendered string, not a subset. |
| Second breakpoint increases Anthropic cache-write cost (1.25x per breakpoint) | Low — Anthropic billing | Kilocode's own analysis: "a single reuse within 5 minutes already wins." The latest-user-message breakpoint pays for itself on the first intra-turn tool round-trip. |
| `hkask-condenser` dependency adds weight to the agent crate | Medium — build time / coupling | T7 uses the algorithm logic directly (it's pure functions) or vendored; do not add a workspace dependency from `crates/agent` to `kask/crates/hkask-condenser` without checking the workspace boundary. |
| Iterative summary refinement could grow unbounded if the summary itself isn't bounded | Medium — summary bloat | The structured template has fixed sections; the model is instructed to "remove stale." Add a soft token cap on the summary output (Kilocode uses 4096). |

## Open Questions

1. **Workspace boundary**: Can `crates/agent` depend on `kask/crates/hkask-condenser`, or does the kask tree live behind a feature flag / separate workspace? Need to check `Cargo.toml` workspace membership. If not reachable, vendor the `rtk_style` head/tail logic (it's ~30 lines).
2. **Spillover file location**: Where should tool-output spillover files live? Kilocode uses a global `tool-output/` dir. Zed has per-thread temp dirs (`$TMPDIR` under sandboxing). Prefer the per-thread temp dir so cleanup is automatic on thread drop.
3. **Compaction template localization**: The current `COMPACTION_PROMPT` is English. The new structured template should match. Confirm there's no i18n layer for agent prompts (there isn't — templates are `.hbs` files).
4. **Digest algorithm**: Which hash? `std::hash::DefaultHasher` is not stable across Rust versions. Use `sha2::Sha256` (already a workspace dep via `git`/`http`) or a simple `FxHash` if available. Confirm before T3.
