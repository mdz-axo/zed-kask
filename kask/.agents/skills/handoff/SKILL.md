---
name: handoff
visibility: public
description: "Session context transfer skill. Compacts the current conversation into a structured handoff document that a fresh agent can use to pick up work cleanly — without duplicating file content. Includes artifact referencing, sensitive data redaction, and skill suggestion for the receiving agent."
---


# Handoff

Session context transfer skill. Compacts the current conversation into a structured handoff document that a fresh agent can use to pick up work cleanly — without duplicating file content. Includes artifact referencing, sensitive data redaction, skill suggestion for the receiving agent, and a convergence check to verify the handoff packet is complete and redaction-safe.

## When to Use

- A session is ending and the conversation context needs to be transferred to a fresh agent that has zero prior context
- Conversation history has become verbose and must be distilled into essential facts before handoff
- Artifacts discussed during the session need to be cataloged by path or URL — never by content — so the next agent can locate them
- Sensitive data (API keys, tokens, PEM blocks, connection strings, PII) must be detected and redacted before the handoff document is written
- The next agent needs recommendations for which skills to invoke and awareness of open questions and risks
- The completed handoff packet needs a convergence check to verify completeness, actionability, and redaction safety

## Instructions

### 1. Compact the session (handoff-compact)

1. State the session purpose in one sentence — what was this session trying to accomplish?
2. Record progress as accomplishments with status (`complete`, `in_progress`, `blocked`), not as a step-by-step process narrative.
3. Document every significant decision with its rationale and the alternative options that were considered.
4. Describe the current state precisely — where exactly did things leave off, and what is unfinished or in-progress?
5. If the user provided a next-session focus, tailor the summary to emphasize information relevant to that focus.
6. Reference file content by path only — never reproduce file contents in the summary.

### 2. Catalog artifacts and detect sensitive data (handoff-artifacts)

1. Identify every relevant artifact from the session and reference it by path or URL — never by content.
2. Classify each artifact by type: `source`, `doc`, `adr`, `prd`, `plan`, `issue`, `commit`, `diff`, `config`, `test`.
3. For each artifact, explain why the next agent needs to know about it.
4. Scan the session context for sensitive data patterns: API keys and tokens (`API_KEY=...`, `token: ...`), PEM private key blocks, database connection strings (`postgres://user:pass@host`), email addresses, and phone numbers.
5. Catalog every field containing sensitive data with the field name and redaction reason (`api_key`, `private_key`, `connection_string`, `pii_email`, `pii_phone`).
6. Prioritize artifacts by relevance to the session's purpose and decisions.

### 3. Suggest skills and extract open questions (handoff-skills-suggest)

1. Analyze the session's domain, remaining work, and artifact types to recommend skills the next agent should invoke.
2. Match skills by domain and task type — debugging → diagnostic/tdd skills, architecture → improve-codebase-architecture, code quality → coding-guidelines/self-critique-revision.
3. Match skills by project stage — pre-implementation → grill-me, hypothesis-framer, mcda; mid-implementation → tdd, diagnose, deep-module; post-implementation → self-critique-revision, caveman.
4. Use hKask naming conventions (lowercase, hyphenated) for skill IDs. Recommend by describing what the next agent needs to do.
5. Assign priority levels: `critical` (must invoke), `recommended` (should invoke), `optional` (may invoke). Maximum 5 suggestions.
6. Scan for unresolved decisions, blockers, ambiguous requirements, and assumptions needing validation. Classify risk level as `high`, `medium`, or `low`. Maximum 7 open questions.

### 4. Compose the handoff document (handoff-compose)

1. Assemble the document with sections in this exact order: header with metadata (session purpose, timestamp, document version), next session purpose, progress summary (table), key decisions and rationale (numbered list), current state (prose), artifact references (table with type, path/URL, description, relevance), suggested skills (table with skill ID, reason, priority), open questions and risks (table with question, risk level, context).
2. Apply all flagged redactions — replace sensitive data with `[REDACTED]` markers. Include a redaction summary at the end.
3. Ensure the document is self-contained and understandable by an agent with zero prior context.
4. Target under 8000 tokens. Every word must earn its place.
5. Make the document actionable — the next agent should be able to start working immediately.
6. Write the document to the OS temp directory at `/tmp/handoff-<timestamp>.md`.

### 5. Check convergence (handoff-convergence-check)

1. Compute a convergence metric on [0, 1] where 0 means the handoff packet is complete, actionable, and redaction-safe, and 1 means not converged.
2. Scan the composed handoff document for unredacted sensitive patterns: API key patterns (`sk-...`, `API_KEY=`, `token:`, `Bearer ...`), PEM blocks, connection strings (`postgres://user:pass@`, `mongodb+srv://`), and email addresses.
3. Verify that every flagged redaction from the artifacts step appears as `[REDACTED]` in the final document.
4. If any sensitive pattern is found unredacted, add it to `blockers` with a description.
5. If any flagged field is missing its `[REDACTED]` marker, add it to `blockers`.
6. Return the convergence metric, rationale, and blockers list.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `handoff-compact.j2` | `KnowAct` | Compress session context into a structured summary with purpose, progress, decisions, and current state. Reduces verbose conversation history to essential facts. |
| `handoff-artifacts.j2` | `KnowAct` | Identify and catalog artifacts by path or URL reference. Detect and flag sensitive data (API keys, tokens, PII) for redaction. |
| `handoff-skills-suggest.j2` | `KnowAct` | Analyze session content and suggest relevant skills the next agent should invoke. Extract open questions and risks for the next session. |
| `handoff-compose.j2` | `WordAct` | Assemble the final handoff document from all gathered sections. Apply redaction, format as structured markdown, and write to the OS temp directory. |
| `handoff-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for handoff PDCA cycles. |

## Constraints

- All templates use `Public` visibility.
- Energy caps: handoff-compact 6144, handoff-artifacts 3072, handoff-skills-suggest 2048, handoff-compose 8192, handoff-convergence-check 2048.
- Jinja2 expressions are sandboxed — no arbitrary Python code execution.
- In safety mode: no file system access, no network calls, no environment variable access, strict Jinja2 sandbox enforcement.
- Maximum output tokens per template: compact 4096, artifacts 2048, skills-suggest 2048, compose 8192, convergence-check 1000.
- Maximum 5 skill suggestions and 7 open questions in the skills-suggest step.
- All artifact references must be by path or URL — zero content duplication across every step.
- All flagged sensitive data must be redacted with `[REDACTED]` markers in the final document.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
