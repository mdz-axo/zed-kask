# hkask-memory

Semantic and episodic memory pipelines for hKask.

Implements the memory consolidation pipeline (L2 in the loop architecture):
episodic → consolidation → semantic.

## Forgetting Curve

Wozniak & Gorzelanczyk (1995), equation (3): **R(t) = exp(-t/S)**

Where S is memory life in days (configurable, default 180 = 6 months × 30).
After S days without recall, confidence decays to exp(-1) ≈ 36.8%.

At recall (when a memory is pulled into a prompt as context), the decay clock
resets — t goes back to 0, R = 1.0.

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `HKASK_MEMORY_LIFE_DAYS` | Memory life S in days | 180 |
| `HKASK_DB_PROVIDER` | Database provider (`sqlite` or `postgres`) | — |
| `HKASK_DB_PATH` | SQLite database path | — |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase | — |

`ServiceConfig.memory_life_days` also accepts the value programmatically.

## Template Pipeline

Two separate Jinja2 templates for hMem extraction from agent operations.

### Algo / No-Judge Merge

Classification runs the fusion panel in parallel — every panelist receives
the same few-shot prompt, and their extractions are merged algorithmically via
the fusion orchestrator's `algo_merge()` (union, case-insensitive dedup,
diverging fields annotated `[A:... B:...]`). This is the **algo / no-judge**
path — a family of deterministic merge strategies with zero LLM judge calls.
The current method is a recursive JSON merge; the architecture anticipates
additional methods (e.g., set intersection, vote/tally) as future sub-selectors
on the `algo` judge value. No separate merge function — the fusion system
handles it. See `docs/how-to/fusion-mode.md`.

| Setting | Env Var | Default |
|---|---|---|
| Panel models | `HKASK_FUSION_PANEL_MODELS` env var or `fusion:` block in corpus.yaml | `KC/qwen/qwen3-235b-a22b-2507`, `DI/google/gemma-4-E4B-it` |

### Content Safety

All LLM boundaries are protected by `hkask-guard` — mandatory input/output
scanning aligned with OWASP LLM Top 10. Prompt injection, role override, and
secret leakage detection are always active at every classification call.

### Memory Templates

Templates invoked via `memory_remember.yaml` FlowDef manifest with `fusion: true`
on each step and manifest-level `fusion: { judge: algo, panel: [...] }`. The
fusion orchestrator dispatches both panel models in parallel and merges JSON
outputs via `merge_json_values` (recursive union, case-insensitive dedup,
diverging strings annotated `[A:... B:...]`). This is the algo / no-judge path —
a deterministic, zero-cost merge with no LLM judge call, and the current method
in a family of extensible algorithmic merge strategies.

| Template | Memory Type | Perspective | Extraction Focus |
|----------|-------------|-------------|-----------------|
| `remember-episodic.j2` | Episodic | First-person | Process, actions, observations |
| `remember-semantic.j2` | Semantic | Third-person | Facts, relationships, knowledge |

### Content Safety
The FlowDef selector (`operation-selector.j2`) classifies the request and
routes to the appropriate template.

## Consolidation

Episodic → Semantic is a one-way bridge. Consolidation:

1. Selects oldest, lowest-confidence episodic hMems
2. Decays confidence at point of use: `memory_decay(days_since, memory_life_days)`
3. Strips perspective, sets Public visibility
4. Bayesian combines with existing semantic hMems (log-odds pooling)
5. Expires episodic source (soft-delete via valid_to)

No token or authorization required — consolidation is always permitted.
