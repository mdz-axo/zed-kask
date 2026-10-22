# ABW Removal Plan — Collapse the Swarm Server and Panel to Local-Only

Status: **reviewed, awaiting operator go** (no destructive edit has been made)
Date: 2026-10-22 · Census basis: commit `4deebd6840` + the concurrent uncommitted trust-verdict workstream (see §8)
Author: Z-K agent (technical program manager) · Operator: principal investigator (go/veto)

Goal (operator's words, restated): the swarm capability becomes exactly two things —
local swarms and local swarm agents. The Agent Bestiary World (ABW) cloud integration
is removed end-to-end: no ABW tools in the swarm MCP server, no ABW surfaces in the
swarm panel, no ABW settings or credentials in Settings > Kask, no skill or doc that
instructs use of a removed tool. Every local capability that exists today still works.

---

## 1. Census inventory (file:line anchors)

Tool partition (from registration sites, verified against `build.rs` generation):
- **ABW cloud router — 48 tools, all removed.** Registered in `src/cloud_swarm_tools.rs`
  (2,535 lines, handler map at :234–:2502) + `src/cloud_swarm/curator.rs` (Xaman, 345)
  + `src/cloud_swarm/helpers.rs` (314), module wrapper `src/cloud_swarm.rs` (22).
  Names: `swarm_{list_agents, get_swarm, get_agent, update_agent, list_apps,
  ontology_templates, execute_agent, hire_cost, request_consent, authorize_session,
  hire, delegate, delegate_and_wait, run_status, generate_prompt, generate_ontology,
  create_agent, create_swarm, xaman, create_app, fanout, fire, delete_agent,
  delete_swarm, search_knowledge, publish_checks, publish_agent, fork_agent, get_app,
  create_app_direct, update_app, publish_app, archive_app, spawn_app_workspace,
  list_app_workspaces, get_app_schema, fork_workspace_to_app, workspace_* (12),
  authorize_session}`.
- **Cloud-local bridges — 4 tools, removed** (handlers in `src/local_tools.rs`):
  `swarm_clone_to_local` :1582, `swarm_push_to_cloud` :1877, `swarm_push_local_swarm`
  :2526, `swarm_pull_swarm_to_local` :2591.
- **Local tools — 37, all kept**: 27 in `local_tools.rs` (incl. `swarm_ai_assist` :2661,
  reworked), 3 thread-scoped (`swarm_thread_local`, `swarm_list_local_threads`,
  `swarm_delegate_in_thread_local`), 4 in `knowledge_tools.rs` (all `*_local`), 3 A2A
  in `a2a_tools.rs` (`swarm_a2a_{card,send,broadcast}`).
- Enumeration source: `build.rs` :20–48 generates `TOOL_NAMES` + `CLOUD_SWARM_TOOL_NAMES`
  (cloud partition = the `#[tool_router(router = cloud_swarm_router)]` file); pinned by
  `cloud_swarm_tool_names_match_live_cloud_router` (`hkask_mcp_swarm.rs` :965–979).

Server (`kask/mcp-servers/hkask-mcp-swarm/`):
- Whole-file ABW-only deletions: `src/abw_client.rs` (413; `require_auth` :51),
  `src/abw_util.rs` (352; straddler — see §3), `src/consent.rs` (743),
  `src/spend_gate.rs` (1,082), `src/cloud_swarm_tools.rs` (2,535), `src/cloud_swarm.rs`
  (22), `src/cloud_swarm/curator.rs` (345), `src/cloud_swarm/helpers.rs` (314).
- `src/hkask_mcp_swarm.rs` (980): mod decls :87–96; `use crate::abw_client::SwarmClient`
  :151; `use crate::consent::ConsentStore` :153; `SwarmServer` fields `client`/`consent`
  :160–161; `combined_router` :173–178; `resolve_consent_store_path` :192–205;
  `run()` wiring :212 (api_key), :229–234 (catalogue-only warn), :383–409 (consent store),
  :411–434 (client + `CredentialRequirement::optional("HKASK_ABW_API_KEY")` :431–434);
  smoke test `cloud_tool_without_key…` :876–892; tool-surface pin :965–979; doc header
  :3–79.
- `src/config.rs` (371): `SwarmMode` :30–59, `api_base_url`/`api_key`/`max_credits_per_dispatch`/
  `curator_consent_default`/`default_agent_model` :71–84; env reads :226–236;
  abw-mode warning :290.
- `src/error.rs` (198): ABW variants (`AbwAuth` :15 …), reqwest conversion, `require_auth`.
- `src/request_types.rs` (1,556): ABW request types (~61 hits), `FanoutAbwEntry` :867–872,
  `AiAssistRequest` :1085 + `mode` doc :1091.
- `src/contract.rs` (542): mode-aware slug rule :78, :404–:477 (local-only rule kept).
- `src/sanitize.rs` (312): ABW-response family :146–274 (`sanitize_abw_response[_plain]`,
  `sanitize_abw_text`, `unwrap_abw_envelope`); local keepers `sanitize_agent_id` :17,
  `strip_leading_mentions` :32 (consumers: agent_stats.rs:26, task_board.rs:234,
  thread_store.rs:39, local_registry.rs:599, local_runtime.rs:21).
- `src/local_registry.rs`: `cloud_swarm_id` field :97 (`serde(default, rename="cloud_id")`,
  no `deny_unknown_fields`) + docs :69–71.
- `src/local_swarms.rs`: `cloud_workspace_id` :53, `set_cloud_workspace_id` :376–384,
  pinning tests :536–568, `abw_util::make_swarm_slug` :219.
- `src/local_tools.rs` (4,141): bridge imports :10 (`create_abw_workspace`,
  `url_encode_segment`, `validate_agent_name`), sanitize imports :18–19, clone/push
  cloud-link code :1827–:2124, ABW doc references :5, :1083, :1574, :1707.
- Constructions `cloud_swarm_id: None` to update: `agent_executor.rs:837`, `a2a.rs:205`,
  `fleet_digest.rs:264`; doc refs in `local_knowledge.rs:1–114`, `local_runtime.rs`,
  `ontology.rs` (ABW path), `port_registry.rs`, `test_support.rs` (reqwest), `config.rs`.
- `Cargo.toml`: description names ABW/Xaman; `reqwest` dep (users: abw_client,
  cloud_swarm_tools, spend_gate, local_tools bridges, test_support, error, lib root —
  all removed → orphan candidate); `uuid` to verify.

Panel (`crates/swarm_panel/src/`, 10,734 lines total; panel touches the server crate ONLY
via `hkask_mcp_swarm::{TOOL_NAMES, CLOUD_SWARM_TOOL_NAMES}` — swarm_panel.rs:154,158,2192,
2668,2726,2732; parse.rs:204):
- Whole-file deletions: `hire.rs` (405), `app_edit.rs` (800).
- Rework: `swarm_panel.rs` (3,031; 92 hits): Steer prompt ABW/local split + consent gate +
  credit ceiling :145–:245, mode state/observer, consent banner :1293, publish banner
  :1384, run-status strip :1127, prompt-token tests :2668–2732 (pointer-equality test
  :2181–2197 keeps — `TOOL_NAMES` stays); `compose.rs` (934; cloud compose + curator
  credits :653–693, :831–837); `author.rs` (1,009; cloud/local toggle); `fetch.rs`
  (1,014; ABW fetch + wallet); `parse.rs` (530; ABW parsing, wallet, run-status,
  cloud_id); `agent_edit.rs` (792; cloud paths → local-only edit);
  `card.rs` (521; Hire/Push/Publish actions → local actions); `ai_assist.rs` (650; mode);
  `swarm_ops.rs` (328; run-status/ABW delete out, local ops stay).
- Clean/keep: `member_turns.rs` (690, local), `card.rs` rendering core, `panel_button.rs`.

Settings/credentials chain:
- `crates/settings_content/src/settings_content.rs`: `KaskSwarmSettingsContent` :1842–1858
  (`mode`, `api_url`, `max_credits_per_dispatch`, `curator_consent_default` out;
  `default_agent_model`, `a2a_http_enabled`, `embedding_dim` stay); `SwarmModeContent`
  enum :1866–1873 (delete).
- `kask/crates/kask_bridge/src/settings.rs`: `KaskSwarmSettings` :396–430 (mirror),
  `SwarmModeConfig` :435–465 (delete), Default-sync comment :466–475.
- `kask/crates/kask_bridge/src/mcp_env.rs`: ABW emissions :243–273, `HKASK_SWARM_MODE`
  + consent-store entries in KASK env list :325–350.
- `kask/crates/kask_bridge/src/mcp_servers.rs`: swarm `BuiltinMcpServer` :286–315
  (credentials `["HKASK_ABW_API_KEY","HKASK_DB_PASSPHRASE"]` :293 → keep passphrase only;
  config_env :295–298 prunes 4 ABW names + `HKASK_SWARM_MODE` + `HKASK_SWARM_CONSENT_STORE`);
  allowlist tests :1197–1240.
- `kask/crates/kask_bridge/src/inference_providers.rs`: `DATA_SERVICES` ABW entry
  :235–240 (credential_key `hkask_abw_api_key`).
- `kask/crates/kask_bridge/src/{credentials.rs:5, context_injector.rs:56}`: doc comments.
- `crates/settings_ui/src/pages/kask_page/swarm.rs` (318): ABW key field + reset card
  :34–89, consent toggle :102–122, API URL :124–143, max credits :145–160, default-model
  help text :162–181; page header :1–2, :222–248; keeps a2a toggle :183–203, embedding
  dim :205–220, default_agent_model input (env renamed).
- `crates/settings_ui/src/pages/kask_page.rs`: search aliases :709–713, prefetch/nudge
  test :847.
- Keychain slot: `kask://credentials/hkask_abw_api_key` (writers: settings UI; reader:
  server env injection; profile `abw.yaml`).

Provider profile: `kask/registry/providers/abw.yaml` (37 lines, delete);
`kask/scripts/check-provider-profiles.sh` derives the adopted set from tree anchors —
case arm `hkask_abw_api_key) echo abw` :65 deletes with the DATA_SERVICES anchor.
Script stays coherent by construction (checks 1–3 iterate the derived set).

Skills/templates/docs:
- `.agents/skills/swarm-intelligence/SKILL.md` (~40 ABW/Xaman/consent/mode hits:
  :4, :10–15, :57, :64, :74, :78–89, :98, :123, :139, :150, :207, :225–231, :345–356,
  :392–396, :408) — rework to local-only substrate.
- `kask/registry/templates/swarm-intelligence/`: act 14, check 1, compose-guide 10,
  decide 15, orient 3, sense 6 abw/xaman lines — rework all six.
- `.agents/skills/local-research-swarm/SKILL.md` :30, :102 — drop ABW tool names/instructions.
- `kask/registry/templates/kask-seam-audit/audit-ui.j2` :27–28 — ABW-delete gate text → local.
- Docs: `kask/docs/reference/mcp-servers/swarm.md` (64 hits — major rework; tool tables,
  auth/consent/credit/mode sections, counts), `kask/docs/reference/kask-settings.md`
  (:313–330 settings rows), `kask/docs/reference/mcp-servers/README.md` :44,
  `kask/docs/README.md` :62, `kask/docs/diataxis/swarm_system/{explanation,reference}.md`
  (6 / 10 hits), `kask/docs/diagrams/swarm.md` (15 hits — ABW backend subgraph),
  `kask/docs/diataxis/kask_bridge/reference.md` :121, `kask/docs/loop-register.md` :521,
  `kask/crates/hkask-memory/README.md` :9 (design-provenance mention — flagged, §4-G).
- `DIVERGENCE.md`: D2 :30 (Steer split sentence), D5 :32 (keychain list + purge record),
  D33 :50 (swarm panel seam — rewrite to local-only panel), :626 (D33 local extension —
  verify), :218 (retired-seam mention — verify).
- `crates/zed/src/main.rs` :2397–2400 (log text naming ABW routing).

False positives (excluded from scope, recorded): `crates/{language,prettier,markdown,
settings,settings_content/language}` "abw" = `tabWidth`/base64 noise; pinned third-party
corpora `kask/crates/hkask-bridge-ontology/sources/{sumo,skillware}` contain "abw"
substrings (WorldAirports etc.) — not fork-owned surface; binary `.gif` noise.

## 2. Consumer census (name-visibility, all forms)

- `hkask_mcp_swarm` external consumers: `hkask-mcp-kata-kanban` (Cargo.toml:18; imports
  all LOCAL types — `LocalDelegateResult`, `TaskSuccessVerdict`, `LocalAgentCapabilities`,
  `LocalAgentCard`, `LocalAgentRegistry`; `cloud_swarm_id: None` construction
  `hkask_mcp_kata_kanban.rs:241`; comment `idempotency.rs:48` mirrors consent store shape);
  `crates/swarm_panel` (consts only, above); `kask_bridge` goals/algedonic tests
  (`LocalAgentRegistry` — local, unaffected).
- `abw_util` consumers: cloud files (deleted) + `local_swarms.rs:219` (`make_swarm_slug`),
  `local_tools.rs:10` (`validate_agent_name` for local creation — verify in-phase;
  `create_abw_workspace`/`url_encode_segment` only in bridge/cloud paths).
- `sanitize` consumers: local keepers listed in §1; ABW family consumers all cloud-side
  (cloud_swarm_tools.rs:14–15,44,889–1259; local_tools.rs:18–19,:1707 — bridge paths).
- `consent`/`spend_gate` consumers: curator.rs, cloud_swarm_tools.rs, test_support.rs,
  abw_client.rs tests, spend_gate↔consent, lib-root wiring — all removed with the cloud
  family; remaining mentions in local files are doc comments (swept).
- `CLOUD_SWARM_TOOL_NAMES` consumers: panel only (listed above) + build.rs + pin test.
- `hkask_abw_api_key` consumers: settings UI (writer), inference_providers (descriptor),
  kask_page test, server env injection (mcp_servers allowlist), abw.yaml, script, docs.
- `cloud_swarm_id`/`cloud_workspace_id` data: no repo card carries the field
  (`grep agents/` clean); stored data-dir cards parse post-removal via
  `serde(default)` + no `deny_unknown_fields`; stale `mcp/swarm/consent.db` in the data
  dir becomes orphaned operator data (reported, not deleted).

## 3. Deletion-test results (load-bearing items)

Complexity that MOVES, not vanishes: `make_swarm_slug` → inline into `local_swarms.rs`;
`validate_agent_name` → move to its local consumer (`local_tools.rs`/`local_registry.rs`);
`sanitize_agent_id` + `strip_leading_mentions` stay in `sanitize.rs` (file shrinks);
`default_agent_model` stays (local runtime consumer, `local_runtime.rs:69–237`); contract
checks stay (mode param dropped); `TOOL_NAMES` generation stays (cloud partition goes);
`swarm_ai_assist` stays (mode param + abw branch go); panel browse/compose/author/edit
stay (local-only). `filter_mcp_tools`/`filter_declared_skills` exist for third-party ABW
card sanitization — verify their only local_tools uses are bridge paths, then delete
(in-phase check). `allowed_tool_servers`/`HKASK_MCP_SERVER_IDS`: verify local create
validates declared `mcp_tools` against it; keep if local-serving, else remove.

## 4. Explicit decisions

- **A. Keychain key `hkask_abw_api_key`: PURGE** (one-time, logged, at kask_bridge
  startup — the zed-side credentials provider is reachable there; recorded in D5).
  Driver: done criterion 1 — "no ABW settings field or credential remains." A dormant
  live third-party secret with no management surface is worse than a logged one-time
  delete. Counter-precedent noted: the 2026-10-17 training removal left dormant
  HuggingFace/Nebius entries operator-owned — operator may veto.
- **B. `cloud_swarm_id` (card, serde "cloud_id") + `cloud_workspace_id` (swarm) fields:
  REMOVE** with setters, badges, and pinning tests; stored JSON parses unchanged
  (serde default, unknown-field tolerance). No data migration.
- **C. `default_agent_model`: KEEP**, env rename `HKASK_ABW_DEFAULT_AGENT_MODEL` →
  `HKASK_SWARM_DEFAULT_AGENT_MODEL` (bridge emission, server read, allowlist, UI help,
  docs). Settings key `kask.swarm.default_agent_model` unchanged — no user migration.
- **D. Mode concept: REMOVE everywhere** (settings content enum, bridge enum, server
  `SwarmMode`, `HKASK_SWARM_MODE`, panel mode state/observer/Steer split, `ai_assist`
  mode param, contract slug-rule mode, skills/templates mode branching, docs). One
  backend means mode is a compatibility state, which done criterion 5 forbids.
- **E. `swarm_ai_assist`: KEEP, local-only** — drop `mode` from request/handler/schema;
  update panel form and skill text. Mid-flight callers passing `mode` get a visible
  validation error (accepted — no compat states).
- **F. Sweep scope**: fork-owned surface only (kask/, fork-owned crates, .agents/,
  DIVERGENCE.md); pinned third-party corpora + upstream zed files excluded and recorded.
  `.rules` is NOT edited inline — proposals in the change report (§7).
- **G. `hkask-memory/README.md:9`**: rewrite the design-provenance sentence to cite
  OpenClaw only (drops the ABW token) — flagged for operator veto (provenance edit).
- **H. Phase order deviation**: panel before server. Reason: the panel consumes
  `CLOUD_SWARM_TOOL_NAMES`; deleting it server-first leaves the panel compiling against
  an empty const with wrong UI, and panel-first keeps every phase green. All other
  order follows the operator's workflow (server+kata → settings/credentials → profile →
  skills/docs → sweep).

## 5. Phases (dependency-ordered; build + verify after each)

- **Phase 0 — Baseline (no edits):** snapshot `git status`; extract the generated
  `TOOL_NAMES` (89 tools: 48 cloud + 41 non-cloud, current tree incl. trust-verdict
  stream) as the before-enumeration.
- **Phase 1 — Panel** (`crates/swarm_panel`): delete `hire.rs`, `app_edit.rs`; rework
  the nine files in §1 to local-only (Steer prompt, banners, mode, wallet, split);
  update prompt-token tests; `zed/src/main.rs:2399` log text. Verify: `cargo check -p
  swarm_panel && cargo test -p swarm_panel` (panel builds against unchanged consts).
- **Phase 2 — Server + kata-kanban:** delete the eight ABW-only files; split `abw_util`;
  shrink `sanitize.rs`; prune config/error/request_types/contract/local_* per §1–§3;
  rewire `run()` (no client/consent/ABW credential requirement — mirror the local-only
  server pattern); build.rs drops the cloud partition; delete the two cloud-pinning
  tests; update kata-kanban :241 + idempotency comment; Cargo orphan check (`reqwest`,
  `uuid`). Verify: `cargo check -p hkask-mcp-swarm && cargo test -p hkask-mcp-swarm`;
  `cargo check -p hkask-mcp-kata-kanban && cargo test -p hkask-mcp-kata-kanban`;
  `cargo check -p swarm_panel` (const seam).
- **Phase 3 — Settings + credentials:** settings_content, bridge settings/mcp_env/
  mcp_servers (+ allowlist tests), inference_providers DATA_SERVICES entry, settings_ui
  swarm page + kask_page, doc comments, keychain purge (Decision A). Verify:
  `cargo check -p settings_content -p kask_bridge -p settings_ui` (+ tests pinning
  settings fields updated in-phase).
- **Phase 4 — Provider profile:** delete `abw.yaml`; remove script case arm :65.
  Verify: `bash kask/scripts/check-provider-profiles.sh` exits 0 (19 profiles).
- **Phase 5 — Skills + docs + DIVERGENCE:** swarm-intelligence SKILL.md + 6 templates,
  local-research-swarm, audit-ui.j2, the ten docs files, DIVERGENCE D2/D5/D33 (:626,
  :218 verified), hkask-memory README (G). Protocol for shared files: re-diff each of
  the 7 concurrently-modified files immediately before editing; never touch the other
  stream's hunks. Verify: scoped residue grep over skills/docs.
- **Phase 6 — Final sweep + verification:** full-repo symbol sweep (code AND docs) for
  every removed identifier (`swarm_hire`, `swarm_delegate` exact, `swarm_xaman`,
  `CLOUD_SWARM_TOOL_NAMES`, `cloud_swarm_id`, `cloud_workspace_id`, `HKASK_ABW`,
  `hkask_abw_api_key`, `abw_client`, `abw_util`, `spend_gate`, `FanoutAbwEntry`,
  `sanitize_abw`, `SwarmMode`, "Agent Bestiary", "Xaman", …) over the §4-F scope;
  after-enumeration = 37 tools, set-diff = 48 cloud + 4 bridges; `./script/clippy
  --deny warnings` over touched scope; full workspace build; local-flow tests; net
  lines measured; orphan sweep complete; `.rules` proposals listed.

## 6. Net lines projection

Method: whole-file `wc -l` (measured) + estimated block removals from hit maps.
Server whole files −5,454; server partial ≈ −1,580; panel ≈ −2,505; settings/bridge
≈ −362 (purge +25); provider ≈ −40; skills/docs ≈ −980. **Projected: ≈ −10,900 net lines
(range −9,500…−11,500).** The measured count at execution is the deliverable.

## 7. `.rules` proposals (listed, never edited inline)

- `.rules:127` cites `hkask-mcp-swarm/src/abw_client.rs` `require_auth` as the canonical
  `permission_denied` reference — the file is deleted; repoint to a live canonical site
  (operator to choose, e.g. `hkask-mcp-research` credential handling).
- Optional new rule (candidate): "capability removals sweep their credential slot,
  provider profile, settings knob, and generated-const seam in the same change" — meets
  the non-obvious/repeated/specific bar only if the operator judges it so.

## 8. Risks

1. **Concurrent stream** (highest): the trust-verdict removal (commit `d3d5cf6bfe` +
   uncommitted continuation, dated today) edits seven files this plan also edits
   (swarm-intelligence SKILL.md, local-research-swarm SKILL.md, swarm-orient.j2,
   diagrams/swarm.md, diataxis/swarm_system/explanation.md, reference/mcp-servers/
   README.md, three agent cards). Hunks are disjoint today (gate/verdict text vs.
   ABW/mode/consent text) but adjacent in two paragraphs (tool counts). Mitigation:
   §5 Phase 5 protocol; recommendation: land or pause the other stream before go.
2. `request_types.rs` ABW-type boundary needs a full read before pruning (61 hits over
   ~1,556 lines) — bounded, in-phase.
3. `uuid`/`allowed_tool_servers` local-purpose verification (§3) — in-phase checks with
   build + dep sweep as the oracle.
4. Running zed session keeps the old server binary until restart — surface in report.

---

## 9. Plan gate — per-skill verdicts (step 3)

### metacognition — confidence per removal step
| Step | Confidence | Basis | What lowers it |
|---|---|---|---|
| P1 panel | 0.80 | file heads + hit maps + const-seam census complete | full reads of compose/fetch/author reveal more ABW-coupled state |
| P2 server+kata | 0.85 | handler map, consumer census, wiring read (`run()`, build.rs) | request_types boundary; a local consumer of consent/spend missed |
| P3 settings | 0.85 | chain fully mapped end-to-end incl. tests | additional field-pinning tests |
| P4 profile | 0.90 | script mechanics read; closed under its derivation | none expected |
| P5 skills/docs | 0.75 | token hits mapped; semantic rework + concurrent stream | stream collision; prose paraphrases without tokens |
| P6 sweep | 0.90 | mechanical | under-matching patterns (mitigated: exact-name set) |
Verdict: **PASS** — every step carries a named basis and a named falsifier; no step
proceeds above 0.75 without an in-phase verification gate.

### hypothesis-framer — load-bearing assumptions as falsifiable hypotheses
P = the swarm surfaces; I = end-to-end ABW removal per §5; C = pre-removal tree;
O = green builds + zero residue + 37-tool enumeration. FINER: Feasible 8, Interesting 7,
Novel n/a (operator-mandated removal — novelty is not the aim; recorded honestly, not
scored away), Ethical 8 (operator's own data; purge decision explicit), Relevant 9.
H1 kata imports only local types & one cloud_swarm_id site — census + `cargo check -p
hkask-mcp-kata-kanban` (P2). H2 panel touches server crate only via the two consts —
census + `cargo check -p swarm_panel` (P1). H3 reqwest orphaned — grep + build (P2).
H4 provider script coherent minus abw — run script (P4). H5 stored cards parse after
field removal — serde attrs read (no deny_unknown_fields) + existing reload tests (P2).
H6 no local consumer of consent/spend — consumer census + build (P2). H7 enumeration
invariant 89→37 with diff = 52 — generated-const extraction (P0/P6). H8 concurrent
stream hunks stay disjoint — per-file re-diff (P5). Verdict: **PASS** — each hypothesis
has a discriminating test bound to a phase; none is unfalsifiable.

### grill-me — Recall / Mechanism / Rationale / Edge cases
Recall: phases 0–6, keep-rule, 37 tools, decisions A–H stated cold — consistent.
Mechanism: cloud router deletion flows through build.rs partition (cloud_names set
empties → const goes; panel-first order prevents the empty-const window — verified by
reading build.rs :20–48); keychain purge mechanism placed zed-side where the provider
is reachable; settings removal degrades old settings.json keys to inert unknowns
(serde ignores). Rationale challenged: panel-before-server (const seam — accepted,
documented as Decision H); mode removal (compat-state prohibition); default_agent_model
retention (live local consumer); skill retention (its live purpose is local composition
— local-research-swarm invokes it in local mode). Edge cases surfaced: ai_assist schema
break for in-flight callers (accepted); consent.db orphan data; stale keychain key
(Decision A); .rules:127 reference rot (§7); running-binary staleness (§8.4); pointer-
equality panel test survives (TOOL_NAMES stays). Verdict: **PASS** — no challenge
survived unanswered; two plan revisions adopted during the gate (H order; A purge).

### essentialist — Exist / Surface / Contract
Exist: every phase traces to a done criterion (P1→c1/c2, P2→c1/c2/3, P3→c1, P4→c1,
P5→c1/c5, P6→c2–5, P0→c2); the keychain purge exists because criterion 1 demands "no
… credential remains". No gold-plating found — the plan adds only the purge (+~25 lines)
and the env rename. Surface: surgical diffs; no unrelated refactors; straddlers reworked
not deleted. Contract: no compatibility states survive (mode param removed, not
defaulted; bridges deleted, not stubbed; DIVERGENCE D33 rewritten to the real surface).
Deletion test applied to every candidate (§3): load-bearing survivors named and their
complexity relocation planned. Verdict: **PASS**.

## 10. Bounded code-capabilities review (single additions list)

- **ADD `code-review`** — execution phase, one convergent review of the final removal
  diff against this spec before Phase 6 verification; justification: a ~11k-line
  six-phase removal across three surfaces warrants one spec-diff review pass.
- NOT added: `kask-seam-audit` — its dead-surface track duplicates Phase 6's orphan
  sweep and DIVERGENCE reconciliation, its security track has no new attack surface
  (removal only), and its GPUI-layout loop applies to added elements (none).
- NOT added: `deep-module` — no new modules or interfaces are designed; the two moved
  helpers land in existing local modules.
- NOT added: `grounding-verify` — the plan's claims are grep/build-grounded with
  file:line anchors recorded here; Phase 6 re-verifies mechanically.

No further skill iteration follows this list.

## 11. Final decision

**ACCEPT** — the plan proceeds as written with the gate-adopted revisions (Decision H
phase order; Decision A purge; §7 .rules proposal; §4-G provenance flag; §5-P5
concurrent-stream protocol). Execution begins only on the operator's explicit go;
the concurrent trust-verdict stream should land or pause first (operator call).