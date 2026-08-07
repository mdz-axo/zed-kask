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

| Variable                 | Description                            | Default |
| ------------------------ | -------------------------------------- | ------- |
| `HKASK_MEMORY_LIFE_DAYS` | Memory life S in days                  | 180     |
| `HKASK_DB_PATH`          | SQLite database path                   | —       |
| `HKASK_DB_PASSPHRASE`    | SQLite SQLCipher encryption passphrase | —       |
| `HKASK_EMBEDDING_DIM`    | Embedding vector dimension             | 1024    |

`ServiceConfig.memory_life_days` also accepts the value programmatically.

## Template Pipeline

Two separate Jinja2 templates for hMem extraction from agent operations.

### Single-Model Extraction

Classification runs the configured classifier model — each passage receives
the same few-shot prompt, and extractions are merged into the corpus. The
classifier model is configured via `HkaskSettings::classifier_model()` (env
var `HKASK_CLASSIFIER_MODEL`), falling back to the registry's default
classifier config.

| Setting          | Env Var                  | Default                                        |
| ---------------- | ------------------------ | ---------------------------------------------- |
| Classifier model | `HKASK_CLASSIFIER_MODEL` | `DeepInfra/Qwen/Qwen3-235B-A22B-Instruct-2507` |

### Content Safety

All LLM boundaries are protected by `hkask-guard` — mandatory input/output
scanning aligned with OWASP LLM Top 10. Prompt injection, role override, and
secret leakage detection are always active at every classification call.

### Memory Templates

Templates invoked via `memory_remember.yaml` FlowDef manifest. Each step
runs single-model extraction through the configured classifier model.

| Template               | Memory Type | Perspective  | Extraction Focus                |
| ---------------------- | ----------- | ------------ | ------------------------------- |
| `remember-episodic.j2` | Episodic    | First-person | Process, actions, observations  |
| `remember-semantic.j2` | Semantic    | Third-person | Facts, relationships, knowledge |

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
