---
title: "Reduct cloud integration — evidence-gated implementation plan"
creator: "Z-K technical program manager"
date: "2026-09-22"
type: "bibo:Document"
status: "Blocked at authenticated API reference; no cloud implementation claimed"
---

# Reduct cloud integration — evidence-gated implementation plan

## Direction and measurement (Improvement Kata)

Users should be able to enter a Reduct API key through Settings → Kask → Data Services, invoke explicitly named cloud media/transcript operations in the Media panel, receive cloud results or classified, visible errors, and continue using local `educt_*` tools without Reduct. Measure with a credential-to-child-MCP test, one contract/response test per documented operation, local educt regressions, and an authorized end-to-end live run. An API key by itself is **not** a working integration.

## Current condition (observed 2026-09-22)

- `tasks/reduct-video-analysis-scaffold.md` explicitly labels itself research, not implementation; its API operations beyond one third-party project-read example are speculative. Its proposed fallback and credential spelling are not binding specifications.
- `kask/mcp-servers/hkask-mcp-media/src/tools/educt.rs`, the server README and `kask/docs/reference/mcp-servers/media.md` identify local transcript and edit operations as a local implementation, not Reduct cloud. `crates/media_panel/src/media_panel.rs` advertises generated MCP tool names and an `educt_` group; there is no `reduct_` group/tool today. This inventory is code inspection, not an executed end-to-end educt test.
- `crates/settings_ui/src/pages/kask_page/data_services.rs` uses `DATA_SERVICES` and keychain-backed write/delete, whose `nudge_mcp_servers` refreshes child credentials. `kask/crates/kask_bridge/src/inference_providers.rs` has no Reduct descriptor; the media allowlist in `kask/crates/kask_bridge/src/mcp_servers.rs` has no Reduct key. `crates/settings_ui/src/pages/kask_page/media.rs` concerns media model overrides, so **Data Services** is the existing key-entry location.
- Official public API overview, https://help.reduct.video/en/articles/api-access (read 2026-09-22), states REST v3; create/edit/retrieve Projects, Recordings, Media, Redactions, Highlights, Comments, Reels and reel blocks; retrieve transcripts/statuses; upload/import media; publish/unpublish reels. It says the API requires Professional or Enterprise and directs users to log in for the full endpoint documentation. This overview does not specify operation methods, URLs, auth-header contract, pagination, payload/response schemas, upload semantics, failure schemas, or edit transaction boundaries.
- Both a web extraction and unauthenticated browser visit to https://app.reduct.video/backstage/api/ returned the **login page**, not the reference. No API key is supplied to this agent environment, and a key alone is not proof of documentation/session access. No live Reduct call or editor restart has been run.

## Target condition and proposed horizon

**Proposed engineering horizon (not an operator deadline):** one to three weeks after authenticated endpoint docs and a safe test workspace become available. Actual duration depends on supported contracts and upload/render latency.

1. Key creation/reset in Data Services uses one OS-keychain slot and refreshes only the media MCP child; missing key and failed authorization return distinct, explicit errors. No key is logged or checked into the tree.
2. Each cloud upload/import, transcript fetch/status, highlight/reel composition and supported edit or export operation has a documented endpoint mapping, a typed boundary test with sanitized fixture data, and a visible Media-panel entry point; operations not documented by the available API explicitly report unsupported rather than mirroring `educt_*` names without behavior.
3. An authorized sample recording completes at least one documented cloud composition/edit path with observed cloud identity and output; provider failures do not trigger implicit local fallback. The local educt regression path remains independently usable without Reduct credentials.
4. Builds/checks pass for `hkask-mcp-media`, `kask_bridge`, `settings_ui`, `media_panel`, and editor integration where affected; managed child processes and user-facing panel are rebuilt and tested, with restart/live checks reported separately.

## Obstacles and sequencing

| Priority | Obstacle | Evidence needed / closure |
|---|---|---|
| 1 (blocking) | Authenticated API reference unavailable | Operator exports or grants locally accessible **non-secret** endpoint reference including request/response schemas, auth, pagination, errors, media upload, transcript, highlights/reels, edits, exports; record source/revision. No key pasted in chat. |
| 2 | Per-operation coverage may differ from product UI | Map every requested action to a documented API contract. For gaps, ask operator which visible cloud subset is acceptable; never synthesize parity. |
| 3 | Safe test account/media and external-data consent | Operator enters key in UI and authorizes a disposable sample recording; no production media or account changes in probes without authorization. |
| 4 | Child credential isolation and refresh | Test descriptor → keychain URL → filtered media env → server credential read, and rotate/remove key while observing the child refresh. |

## Small vertical slices (task-breakdown)

1. **Documented capability gate (S; no source edits).** From authenticated docs produce a table of method, path, auth, payload, response/error, mutation and consent classification for each requested workflow. Verification: every proposed tool has an official reference; unavailable operations are listed as gaps. **Blocked by obstacle 1.**
2. **Cloud connection and one read operation (S).** Add a Reduct descriptor to Data Services, media-only allowlist, and one documented, read-only MCP operation through a small client localized to media MCP. Test keychain-to-child path; missing key, 401/403, transport errors, provider errors and successful identity; panel advertises it using generated `TOOL_NAMES`. Depends on task 1. Checkpoint: run scoped tests/build and inspect the actual panel result before mutations.
3. **Cloud media/transcript operation (S per contract).** Wire one documented upload/import + transcript/status or retrieval path with explicit data-transfer consent and robust in-progress/error states; use disposable sample content. Depends on tasks 1–2. Checkpoint: fixture and authorized live check; local educt still functions without Reduct key.
4. **Cloud composition/edit/export (S per contract).** Add only documented highlights/reel/block or other editing and export operations; tests prove server-side state/result identity and fail-closed handling for unsupported operations. Depends on tasks 1–3. Checkpoint: build affected MCP/widgets and run an authorized full panel flow after a safe restart, otherwise hand off exact remaining steps.

## Architecture/essentialist challenge

Keep `educt_*` local and introduce explicit `reduct_*` cloud operations (no mode-routing heuristic or auto-fallback). Use existing `DATA_SERVICES`/keychain/child-env path and generated tool advertisement; do not create a second credential store, cloud `*_enabled` setting, speculative shared local/cloud trait, or matching local aliases with no documented provider implementation. The initial cloud client earns existence only if it encapsulates auth, classified HTTP errors and provider response parsing; if it is merely a one-line forwarding wrapper, inline it. Panel changes should be labels/explanatory copy tied to actual tools, not a second bespoke editor. Upload semantics and consent must be settled against authenticated docs before making a mutation reachable.

## Kata experiment log and next experiment

**Baseline observation:** unauthenticated extraction and browser visit of the endpoint reference each resolved to login; public help article gave capability categories but no endpoint contracts. Thus the documented-contract count for requested cloud workflow calls remains **zero**. No prior prediction was recorded for these observations, so do not retroactively score them as a successful prediction.

**Next experiment (pending external input):** operator obtains the logged-in endpoint reference *without sharing the key*, as a sanitized HTML/PDF/OpenAPI export placed in a project-readable location or by another approved secure route. Prediction: it will document at least one read operation and one upload/transcript operation, but may not expose every UI edit/export operation; confidence 0.65 (hypothesis, not a verified claim). Check by extracting method/path/request/response/error details and counting coverage of the target workflow. If present, implement slice 2 and test it, then select the first documented mutating path. If absent, report the specific unsupported actions and ask the operator which subset to expose. Do not treat the plan as delivery evidence.

## Risk and ownership

Until the gate opens, **cloud workflow unavailable** (operator decision/input required to provide docs; technical owner: implementation agent after that). A key-only UI change would misleadingly suggest users can work in Reduct, so it is deliberately deferred. Reduct plan limits, billing and consent apply to uploads; exact API costs/rate limits are unknown until authenticated docs/account data are reviewed. Local educt is not removed or altered. No code or runtime verification is claimed by this planning artifact.
