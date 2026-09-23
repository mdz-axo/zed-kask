---
title: "Reduct cloud integration — evidence-gated implementation plan"
creator: "Z-K technical program manager"
date: "2026-09-22"
type: "bibo:Document"
status: "Key loaded and read-only project endpoint verified; cloud media and editing blocked on endpoint reference"
---

# Reduct cloud integration — evidence-gated implementation plan

## Direction and measurement (Improvement Kata)

Users should be able to enter a Reduct API key through Settings → Kask → Data Services, invoke explicitly named cloud media/transcript operations in the Media panel, receive cloud results or classified, visible errors, and continue using local `educt_*` tools without Reduct. Measure with a credential-to-child-MCP test, one contract/response test per documented operation, local educt regressions, and an authorized end-to-end live run. An API key by itself is **not** a working integration.

## Current condition (observed 2026-09-22)

- `tasks/reduct-video-analysis-scaffold.md` explicitly labels itself research, not implementation; its API operations beyond one third-party project-read example are speculative. Its proposed fallback and credential spelling are not binding specifications.
- At intake, `educt_*` was entirely local and there were no `reduct_*` tools. `crates/media_panel/src/media_panel.rs` now advertises a distinct Reduct group from generated MCP tool names. Scoped local educt regressions were run serially and passed (34 tests); no live local video render was performed in this continuation.
- At intake, Data Services had no Reduct descriptor or media allowlist. This working-tree change adds both and `reduct_connection_status` to verify media-child key delivery without disclosing the key or claiming provider validation. `crates/settings_ui/src/pages/kask_page/media.rs` concerns media model overrides, so **Data Services** is the key-entry location.
- Official public API overview, https://help.reduct.video/en/articles/api-access (read 2026-09-22), states REST v3; create/edit/retrieve Projects, Recordings, Media, Redactions, Highlights, Comments, Reels and reel blocks; retrieve transcripts/statuses; upload/import media; publish/unpublish reels. It says the API requires Professional or Enterprise and directs users to log in for the full endpoint documentation. This overview does not specify operation methods, URLs, auth-header contract, pagination, payload/response schemas, upload semantics, failure schemas, or edit transaction boundaries.
- Unauthenticated web access to https://app.reduct.video/backstage/api/ returned login. After the operator entered the key and restarted, `reduct_connection_status` returned `configured` with `provider_connection: not_checked`. An explicit read-only test loaded the key from the OS keychain (never printed), authenticated `GET https://app.reduct.video/api/v3/project` via `x-auth-key` with HTTP 200, and verified the live response has a `project` map keyed by ID with `title` fields. A live request to the API reference with the same key returned HTTP 302, still login-gated. `reduct_projects_snapshot` read path passed an explicit live test; no cloud video/audio/transcript/edit operation was verified.

## Target condition and proposed horizon

**Proposed engineering horizon (not an operator deadline):** one to three weeks after authenticated endpoint docs and a safe test workspace become available. Actual duration depends on supported contracts and upload/render latency.

1. Key creation/reset in Data Services uses one OS-keychain slot and refreshes only the media MCP child; missing key and failed authorization return distinct, explicit errors. No key is logged or checked into the tree.
2. Each cloud upload/import, transcript fetch/status, highlight/reel composition and supported edit or export operation has a documented endpoint mapping, a typed boundary test with sanitized fixture data, and a visible Media-panel entry point; operations not documented by the available API explicitly report unsupported rather than mirroring `educt_*` names without behavior.
3. An authorized sample recording completes at least one documented cloud composition/edit path with observed cloud identity and output; provider failures do not trigger implicit local fallback. The local educt regression path remains independently usable without Reduct credentials.
4. Builds/checks pass for `hkask-mcp-media`, `kask_bridge`, `settings_ui`, `media_panel`, and editor integration where affected; managed child processes and user-facing panel are rebuilt and tested, with restart/live checks reported separately.

## Obstacles and sequencing

| Priority | Obstacle | Evidence needed / closure |
|---|---|---|
| 1 (blocking for media/edit workflows) | API reference remains login-gated even with API key (HTTP 302) | Requires a separately logged-in browser session or a secure local export of non-secret endpoint reference; API key alone cannot expose endpoint contracts. No key pasted in chat. |
| 2 | Per-operation coverage may differ from product UI | Map every requested action to a documented API contract. For gaps, ask operator which visible cloud subset is acceptable; never synthesize parity. |
| 3 | Safe test account/media and external-data consent | Key is loaded; no media upload has been authorized or attempted. Use a disposable sample recording only after docs identify the upload contract and explicit consent. |
| 4 | Child credential isolation and refresh | Test descriptor → keychain URL → filtered media env → server credential read, and rotate/remove key while observing the child refresh. |

## Small vertical slices (task-breakdown)

1. **Credential affordance (S; implemented in working tree).** Add Reduct to Data Services and the media-only allowlist. Show only key-delivery status, not provider validity; test missing key, secrecy and credential-to-MCP routing. Checkpoint: scoped tests/build and operator entry of the key through Settings after a safe editor update.
2. **Observed project read (S; code and live contract check implemented).** A documented third-party project-read URL returned HTTP 200 with the OS-keychain key. Bound the response, project ID/title projection and classified errors; do not assert pagination. The newly added MCP tool and panel must still be exercised after rebuilding/restarting the updated binary.
3. **Documented workflow gate (S; blocked).** Obtain authenticated Reduct endpoint contracts for upload, transcription, reel/edit/export; produce a method/path/auth/payload/response/error table. The key does not unlock the docs URL (observed HTTP 302). No media/edit tool is added until the contracts are available.
4. **Cloud media/transcript operation (S per contract).** Wire one documented upload/import + transcript/status or retrieval path with explicit data-transfer consent and robust in-progress/error states; use disposable sample content. Depends on task 3. Checkpoint: fixture and authorized live check; local educt still functions without Reduct key.
5. **Cloud composition/edit/export (S per contract).** Add only documented highlights/reel/block or other editing and export operations; tests prove server-side state/result identity and fail-closed handling for unsupported operations. Depends on tasks 3–4. Checkpoint: build affected MCP/widgets and run an authorized full panel flow after a safe restart, otherwise hand off exact remaining steps.

## Architecture/essentialist challenge

Keep `educt_*` local and introduce explicit `reduct_*` cloud operations (no mode-routing heuristic or auto-fallback). Use existing `DATA_SERVICES`/keychain/child-env path and generated tool advertisement; do not create a second credential store, cloud `*_enabled` setting, speculative shared local/cloud trait, or matching local aliases with no documented provider implementation. The initial cloud client earns existence only if it encapsulates auth, classified HTTP errors and provider response parsing; if it is merely a one-line forwarding wrapper, inline it. Panel changes should be labels/explanatory copy tied to actual tools, not a second bespoke editor. Upload semantics and consent must be settled against authenticated docs before making a mutation reachable.

## Kata experiment log and next experiment

**Baseline observation:** unauthenticated extraction and browser visit of the endpoint reference each resolved to login; public help article gave capability categories but no endpoint contracts. Thus the documented-contract count for requested cloud workflow calls remains **zero**. No prior prediction was recorded for these observations, so do not retroactively score them as a successful prediction.

**Experiment 1 (observed in working tree):** add Reduct to the existing Data Services registry and expose a media-child credential-delivery status without any provider request. Prediction: this gives the operator a safe key-entry surface without implying successful API access. The Reduct-specific bridge test failed first (missing descriptor), then passed after the change; the status tool's missing/configured-state tests passed. A real key write and live child refresh have not yet been observed.

**Experiment 2 (observed):** the operator entered the key and restarted the editor. The media-child credential-status tool reported configured. An explicitly gated OS-keychain test made a read-only authenticated project request: HTTP 200; the key was never printed or returned. Prediction that API-key access might not unlock web docs was confirmed: the docs URL returned HTTP 302. **Experiment 3 (observed):** inspect only the returned response structure, then add a bounded ID/title-only project snapshot. A live test of the new snapshot path passed, without exposing project names/IDs in test output. **Next obstacle:** obtain endpoint documentation using a logged-in browser session; no unsupported cloud media action should be inferred from the project read.

## Risk and ownership

Until media/edit endpoint contracts are accessible, **cloud composition/editing unavailable** (technical owner: implementation agent, external documentation gate). The key-entry UI and read-only project snapshot are now implemented; only the project-read path has live API evidence. The next operator action, if cloud editing remains the goal, is to use a logged-in Reduct account to make the non-secret API reference accessible locally; this is not a request to share the secret. Reduct plan limits, billing and consent apply to uploads; exact API costs/rate limits are unknown until authenticated docs/account data are reviewed. Local educt is not removed or altered. No code or runtime verification is claimed by this planning artifact.
