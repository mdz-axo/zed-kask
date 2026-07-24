# hkask-mcp-condenser

Context condensation MCP server — compress tool outputs, manage profiles, classify categories, summarize thread history.

Part of hKask's Episodic loop (L2). The condenser operates on the active conversation window, compressing tool outputs to fit within token budgets.

## Tools (8)

| Tool | Description | Requires Config |
|------|-------------|-----------------|
| `condenser_ping` | Liveness check, profile info, algorithm listing, suggested profile, compression history stats | — |
| `condenser_compress` | Compress tool output using context-aware algorithms. Auto-selects the best-performing algorithm per category when sufficient compression history exists (learning). | — |
| `condenser_classify` | Classify tool name to context category | — |
| `condenser_set_profile` | Set compression profile (heavy/normal/soft/light) | — |
| `condenser_stats` | Cumulative compression statistics | — |
| `condenser_score_saliency` | Score text relevance against persona keywords (word-overlap) or memory stores (semantic/episodic search). Returns 0.0–1.0. The `against` parameter accepts `"persona"` (default) or `"memory"`. When `against="persona"`, the optional `persona_keywords` parameter overrides the server's default keyword set. When `against="memory"`, the tool checks for a semantic memory store first, then falls back to episodic memory. If neither is available, returns 0.5 (neutral). | Persona keywords configurable via `HKASK_CONDENSER_PERSONA_KEYWORDS` or per-request override. Memory stores require `HKASK_DB_PATH` |
| `condenser_persist` | Persist compressed output to episodic memory | `HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE` |
| `condenser_thread_summary` | LLM-powered conversation summarization via centralized inference router. Disables model thinking/reasoning mode to ensure output tokens are used for the summary, not internal reasoning. Returns `original_tokens_approx` and `summary_tokens_approx` for context-window budgeting. | — |

## Compression Profiles

| Profile | Retention | Max Lines | Use Case |
|---------|-----------|-----------|----------|
| heavy | 10% | 30 | Aggressive compression |
| normal | 20% | 80 | Default balance |
| soft | 60% | 200 | Light compression |
| light | 95% | ∞ | Near-passthrough |

## Algorithms

| Algorithm | Categories | Strategy |
|-----------|-----------|----------|
| `rtk_style` | ShellCommand, TestOutput, BuildOutput | Head/tail preservation with ellipsis |
| `word_rank` | ConversationHistory, LogOutput | TF-IDF bag-of-words compression + structural bonus + ontology anchoring |
| `flashrank` | FileContents, StructuredData, Unknown | Greedy marginal-utility selection (relevance + novelty + brevity) |

## Configuration

Environment variables (all optional):

| Variable | Description | Default |
|----------|-------------|---------|
| `HKASK_DB_PATH` | SQLite database path for episodic persistence | In-memory (no persistence) |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase | Required if `HKASK_DB_PATH` is set |
| `HKASK_DEFAULT_MODEL` | Model for thread summarization (inherits system default) | `google/gemma-4-26B-A4B-it` (configure via `INFERENCE_MODEL` env var) |
| `HKASK_CONDENSE_SALIENCY_WINDOW` | Saliency window multiplier for default `max_tokens` in thread summarization (saliency × 100, clamped to [150, 2000]) | `5` (→ 500 tokens) |
| `HKASK_CONDENSER_PERSONA_KEYWORDS` | Comma-separated persona keywords for saliency scoring. Overrides the default generic condensation terms. | Generic condensation terms (condense, compress, summarize, context, token, budget, saliency, relevance, retention, profile, ontology, category, persist) |

Without `HKASK_DB_PATH`, `condenser_persist` returns a permission-denied error and `condenser_score_saliency` with `against="memory"` returns 0.5 (neutral). All other tools work without configuration (graceful degradation). Thread summarization uses the centralized hKask inference router (configured via standard `DI_API_KEY`, `FA_API_KEY`, `TG_API_KEY`, `OR_API_KEY`, `KC_API_KEY` environment variables). The default model is configured via the `INFERENCE_MODEL` env var (defaults to `google/gemma-4-26B-A4B-it`).

When `HKASK_DB_PATH` is set, both episodic and semantic memory stores are initialized from the same SQLite database. Semantic memory includes an `EmbeddingStore` (1024-dimensional vectors) for KNN similarity search. The `condenser_score_saliency` tool queries semantic memory first (shared knowledge), falling back to episodic memory (first-person experience) if semantic is unavailable.

## Context Categories

| Category | Matched Substrings | Algorithm |
|----------|-------------------|-----------|
| `shell_command` | git, docker, cargo, npm, shell, exec, run, bash | rtk_style |
| `test_output` | test, pytest, spec | rtk_style |
| `build_output` | build, compile, make | rtk_style |
| `file_contents` | file, read, cat | flashrank |
| `conversation_history` | chat, conversation, message | word_rank |
| `structured_data` | json, api, query | flashrank |
| `log_output` | log, journal, trace | word_rank |
| `unknown` | (fallback) | flashrank |

More-specific categories are checked first — `test` matches before `run`, so `pytest_run` classifies as `test_output`.

## Token Estimation

`approx_token_count` uses the standard ~4 characters per token heuristic (same rule of thumb used by OpenAI's tiktoken and Anthropic's Claude). This provides fast, dependency-free estimates for context-window planning.

`ThreadSummaryOutput` includes both `original_tokens_approx` (before summarization) and `summary_tokens_approx` (after), enabling callers to budget context windows. The `ChatService` auto-condense trigger uses this same heuristic at 87.5% of the model's context window.

## Thinking Mode

For models with reasoning/thinking mode (e.g., qwen3, gemma4, deepseek-r1), `condenser_thread_summary` and `ChatService::condense_history` set `disable_thinking: true` in `LLMParameters`. This maps to `enable_thinking: false` in the OpenAI-compatible chat request, instructing the model to skip internal reasoning and produce output directly. Without this, reasoning-mode models can consume all `max_tokens` on internal thought, producing empty visible output.

The `enable_thinking` field is only serialized when `false` — backends that don't support it are unaffected.

**Known behavior:** Some backends may not honor `enable_thinking: false` for reasoning-mode models (qwen3.5, gemma4, deepseek-r1). The condenser gracefully degrades: when a thinking model returns an empty summary, the tool responds with `"Inference engine returned an empty summary"`. **Workaround:** use a non-thinking model for summarization. The default condenser model is configured via the `INFERENCE_MODEL` env var (defaults to `google/gemma-4-26B-A4B-it`), which produces clean structured summaries without thinking interference.

## Running

```bash
# As part of kask (auto-started with other MCP servers)
kask chat

# Standalone stdio MCP server
hkask-mcp-condenser

# With persistence
HKASK_DB_PATH=/path/to/db HKASK_DB_PASSPHRASE=secret hkask-mcp-condenser
```

## Tool Surface Justification

The condenser exposes 8 tools, exceeding the 7-function guideline. Each tool beyond 7 is justified:

| Tool | Why It Cannot Be Merged |
|------|------------------------|
| `condenser_classify` | Preview operation — lets clients check classification without paying the compression cost. Merging into `compress` would force clients to compress just to see the category. |
| `condenser_stats` | Cumulative state across all compressions (counts, byte totals, algorithm usage). `condenser_ping` returns instantaneous state (current profile, health). Different time horizons, different data shapes. |

## Learning

The condenser learns which compression algorithm performs best per category. After 10 compressions for a given category, `CondenserEngine::recommend_algorithm()` returns the algorithm with the highest historical compression ratio. When sufficient data exists, `compress()` auto-selects the recommended algorithm instead of the static `default_for()` mapping.

The engine stores up to 200 `CompressionRecord` observations in a bounded ring buffer. The `condenser_ping` response includes:
- `suggested_profile` — recommends a more aggressive profile when health checks flag degradation
- `history_records` — number of stored compression observations
- `history_stats` — per-algorithm and per-category compression ratio summaries

## Regulation Spans

The `reg.condenser` tracing spans (compress, compression_ratio, health) are **diagnostic logging** for human inspection via log output — NOT cybernetic feedback signals. They are not consumed by any regulation policy or feedback loop.

The actual feedback channel is the daemon's `store_experience` call, enriched with compression quality data (algorithm, category, profile, compression_ratio, health_signal_count). This data is available to the Regulation runtime for observability and analysis.

## Two-Phase Condensation

The ChatService's auto-condense pipeline supports two-phase condensation:
1. **Phase 1 (CPU):** Pre-compress the old half of conversation history with `CondenserEngine` (Profile::Heavy, ConversationHistory category). Reduces token count before the expensive LLM call.
2. **Phase 2 (LLM):** Summarize the pre-compressed old half via the centralized inference router.

Phase 1 is controlled by the `pre_compress` setting (default: true). When disabled, the raw old half is fed directly to the LLM summarizer.