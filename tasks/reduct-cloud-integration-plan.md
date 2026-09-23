---
title: "Reduct cloud integration — evidence-gated implementation plan"
creator: "Z-K technical program manager"
date: "2026-09-22"
type: "bibo:Document"
status: "Credential affordance implemented in working tree; provider API workflows unverified"
---

# Reduct cloud integration — evidence-gated implementation plan

## Direction and measurement (Improvement Kata)

Users should be able to enter a Reduct API key through Settings → Kask → Data Services, invoke explicitly named cloud media/transcript operations in the Media panel, receive cloud results or classified, visible errors, and continue using local `educt_*` tools without Reduct. Measure with a credential-to-child-MCP test, one contract/response test per documented operation, local educt regressions, and an authorized end-to-end live run. An API key by itself is **not** a working integration.

## Current condition (observed 2026-09-22)

- `tasks/reduct-video-analysis-scaffold.md` explicitly labels itself research, not implementation; its API operations beyond one third-party project-read example are speculative. Its proposed fallback and credential spelling are not binding specifications.
- `kask/mcp-servers/hkask-mcp-media/src/tools/educt.rs`, the server README and `kask/docs/reference/mcp-servers/media.md` identify local transcript and edit operations as a local implementation, not Reduct cloud. `crates/media_panel/src/media_panel.rs` advertises generated MCP tool names and an `educt_` group; there is no `reduct_` group/tool today. This inventory is code inspection, not an executed end-to-end educt test.
- At intake, Data Services had no Reduct descriptor or media allowlist. This working-tree change adds both and `reduct_connection_status` to verify media-child key delivery without disclosing the key or claiming provider validation. `crates/settings_ui/src/pages/kask_page/media.rs` concerns media model overrides, so **Data Services** is the key-entry location.
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
| 1 (blocking for workflows) | Authenticated API reference unavailable | First enter the key in Data Services; investigate access with the running media child and public sources. If a logged-in web session is still required, operator may authorize a secure local docs export. No key pasted in chat. |
| 2 | Per-operation coverage may differ from product UI | Map every requested action to a documented API contract. For gaps, ask operator which visible cloud subset is acceptable; never synthesize parity. |
| 3 | Safe test account/media and external-data consent | Operator enters key in UI and authorizes a disposable sample recording; no production media or account changes in probes without authorization. |
| 4 | Child credential isolation and refresh | Test descriptor → keychain URL → filtered media env → server credential read, and rotate/remove key while observing the child refresh. |

## Small vertical slices (task-breakdown)

1. **Credential affordance (S; implemented in working tree).** Add Reduct to Data Services and the media-only allowlist. Show only key-delivery status, not provider validity; test missing key, secrecy and credential-to-MCP routing. Checkpoint: scoped tests/build and operator entry of the key through Settings after a safe editor update.
2. **Documented capability gate (S).** With the entered key and authorized account, investigate API access and obtain authenticated reference contracts when accessible; produce a method/path/auth/payload/response/error table. A key may not authenticate the docs website; if so, report the precise blocker rather than claim coverage. **Waiting for an operator-entered key and runtime access.**
3. **Cloud connection and one read operation (S).** Implement one documented read-only MCP operation through a small media-local client; test 401/403, transport errors, provider errors and successful identity. Depends on task 2. Checkpoint: scoped tests/build and inspect the actual panel result before mutations.
4. **Cloud media/transcript operation (S per contract).** Wire one documented upload/import + transcript/status or retrieval path with explicit data-transfer consent and robust in-progress/error states; use disposable sample content. Depends on tasks 2–3. Checkpoint: fixture and authorized live check; local educt still functions without Reduct key.
5. **Cloud composition/edit/export (S per contract).** Add only documented highlights/reel/block or other editing and export operations; tests prove server-side state/result identity and fail-closed handling for unsupported operations. Depends on tasks 2–4. Checkpoint: build affected MCP/widgets and run an authorized full panel flow after a safe restart, otherwise hand off exact remaining steps.

## Architecture/essentialist challenge

Keep `educt_*` local and introduce explicit `reduct_*` cloud operations (no mode-routing heuristic or auto-fallback). Use existing `DATA_SERVICES`/keychain/child-env path and generated tool advertisement; do not create a second credential store, cloud `*_enabled` setting, speculative shared local/cloud trait, or matching local aliases with no documented provider implementation. The initial cloud client earns existence only if it encapsulates auth, classified HTTP errors and provider response parsing; if it is merely a one-line forwarding wrapper, inline it. Panel changes should be labels/explanatory copy tied to actual tools, not a second bespoke editor. Upload semantics and consent must be settled against authenticated docs before making a mutation reachable.

## Kata experiment log and next experiment

**Baseline observation:** unauthenticated extraction and browser visit of the endpoint reference each resolved to login; public help article gave capability categories but no endpoint contracts. Thus the documented-contract count for requested cloud workflow calls remains **zero**. No prior prediction was recorded for these observations, so do not retroactively score them as a successful prediction.

**Experiment 1 (observed in working tree):** add Reduct to the existing Data Services registry and expose a media-child credential-delivery status without any provider request. Prediction: this gives the operator a safe key-entry surface without implying successful API access. The Reduct-specific bridge test failed first (missing descriptor), then passed after the change; the status tool's missing/configured-state tests passed. A real key write and live child refresh have not yet been observed.

**Next experiment (after operator enters key in the rebuilt Settings UI):** call `reduct_connection_status` to check key delivery, then seek provider documentation or probe only evidenced, read-only endpoint contracts through the child. Prediction: key delivery will succeed, but the API reference may still require a separate browser login; do not conflate these. If endpoint contracts remain inaccessible, report that precise blocker, not an invented operation. Do not treat the plan as delivery evidence.

## Risk and ownership

Until verified endpoints are implemented, **cloud workflow unavailable** (technical owner: implementation agent). The key-entry UI now explicitly says provider access is unverified and cloud operations are not connected. The next operator action is entry of the key through Settings; the agent then investigates available authenticated API evidence without requesting the secret in chat. Reduct plan limits, billing and consent apply to uploads; exact API costs/rate limits are unknown until authenticated docs/account data are reviewed. Local educt is not removed or altered. No code or runtime verification is claimed by this planning artifact.
