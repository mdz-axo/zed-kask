# Swarm Gap Resolutions

Tracked from the gap analysis (2026-08-18). Each item cites the evidence and
the minimal resolution path. Items 1-6 are implemented; item 7 is a follow-up
UX examination.

## Status: IMPLEMENTED (2026-08-18)

All 6 gaps resolved. Tests pass (`cargo test -p hkask-mcp-swarm --all-features`,
`cargo test -p swarm_panel`, `cargo test -p hkask-templates`). Clippy clean
(`./script/clippy -p hkask-mcp-swarm -p swarm_panel -p hkask-templates`).

## Item 7 (TODO — examine, do not implement yet): Compose → Launch → Steer UX

**Question:** Should the swarm panel have a "Launch Swarm" button on the
Steer page, beneath the compose interface, that takes the user to the Steer
page as part of launching the swarm?

**Current wiring (verified):**
- `SwarmPanel::create_swarm` (`crates/swarm_panel/src/swarm_panel.rs:1248`)
  already transitions to `PanelMode::Steer` after a successful create — both
  the local path (`:1311-1317`) and the ABW path (`:1465-1469`) call
  `this.set_mode(PanelMode::Steer, window, cx)` and set
  `this.selected_workspace = Some(id)`.
- The Steer page already has a "Launch Plan" button
  (`swarm_panel.rs:2820-2834`) that injects a message telling the Curator to
  execute the pending `swarm-intelligence` plan via
  `swarm_execute_plan_local` and feed `delegate_results` back.
- The compose form (`crates/swarm_panel/src/compose.rs`) has a "Create Local
  Swarm" / "Create ABW Swarm" button that triggers `create_swarm`.

**What to examine:**
1. Is the compose→steer transition discoverable enough? The user creates a
   swarm and lands on the Steer page with an empty conversation — do they
   know to type a composition intent, or should there be a "Launch Swarm"
   button that auto-injects a composition prompt?
2. Should the "Launch Plan" button (currently on the Steer page, for
   executing a pending plan) be distinct from a "Launch Swarm" button (which
   would kick off the first `swarm-intelligence` invocation on a freshly
   created swarm)?
3. Should the compose form have a "Compose & Steer" button that creates
   the swarm AND auto-invokes `swarm-intelligence` with a default
   composition task, rather than requiring the user to type in the Steer
   conversation?

**Resolution path (when implemented):**
- Add a "Launch Swarm" button to the Steer page header (next to "Launch
  Plan") that injects a default composition prompt
  (`/swarm-intelligence mode=<mode> swarm_id=<id> compose my swarm`)
  via the D21 `ConversationInjector`.
- OR: add a "Compose & Steer" checkbox/button to the compose form that,
  after `create_swarm` succeeds, injects the composition prompt into the
  Steer conversation automatically.

**Status:** TODO — examine the UX, do not implement until the design is
decided.

---

## Items 1-6: Implementation — COMPLETE

All 6 gaps implemented and tested. Summary of changes:

1. **Gap 1 (condenser not wired into swarm delegation path):**
   - `local_knowledge.rs`: `record_delegation` now takes a `response: &str`
     parameter and writes a `delegation:response` `HMem` triple (64KB-capped)
     alongside the existing latency + task_success annotations. The response
     is now recallable via `swarm_search_knowledge_local`.
   - `local_tools.rs`: both `swarm_delegate_local` and
     `swarm_execute_plan_local` pass `&result.response` to
     `record_delegation`.
   - Tests updated: `record_delegation_degrades_gracefully_when_memory_unavailable`,
     `record_delegation_writes_and_reads_back_stigmergy_trail`,
     `record_delegation_skips_task_success_when_none`.

2. **Gap 2 (curator memory ≠ swarm memory):**
   - `swarm-sense.j2`: added Step 1a (local mode only) instructing the agent
     to call `swarm_search_knowledge_local` for each roster member with
     `query = "delegation"`. Results feed Step 5b's per-agent fitness
     computation. Added `stigmergy` to the output contract and output
     section.

3. **Gap 3 (no shared workspace chat):**
   - `a2a_tools.rs`: added `swarm_a2a_broadcast` tool — broadcasts an A2A
     message to all members of a local swarm via sequential
     `LocalSwarmRuntime::delegate` dispatch. Capped at `MAX_FANOUT`.
   - `request_types.rs`: added `A2aBroadcastRequest` + schema test.
   - `hkask_mcp_swarm.rs`: updated tool count test from 52 to 53.
   - `build.rs`: updated comment from 52 to 53.

4. **Gap 4 (steering closure prompt-driven):**
   - `swarm-intelligence.yaml`: changed `steering_mode` default from
     `"advisory"` to `"steering"`. Added post-Act execute step (ordinal 8)
     that calls `swarm_execute_plan_local` when `mode == 'local' and
     steering_mode == 'steering'`. Renumbered steps 8-14 to 9-15. Added
     `delegate_results` binding to the LOOP step's input_mapping, reading
     from `step_8_result.results`.
   - `registry.rs` + `yaml_schema_validation.rs`: updated ordinal references
     in manifest tests (CHECK 10→11, accumulate 11→12, monitor 12→13,
     lisp.eval 13→14, loop 14→15; step count 14→15, execute steps 4→5).
   - `SKILL.md` (swarm-intelligence + swarm-steering): updated Loop A
     closure status from OPEN to STRUCTURAL.
   - `swarm_panel.rs`: updated steer system prompt to reflect structural
     closure.

5. **Gap 5 (no pairwise attribution):**
   - `swarm-sense.j2`: added Step 3a — Jaccard similarity of port labels
     (accepts ∪ produces) for each agent pair. Pairs with `jaccard >= 0.75`
     are flagged as redundant. Added `redundancy_pairs` to the output
     contract and output section.

6. **Gap 6 (no orchestra governance):**
   - `local_swarms.rs`: added `MemberSource` struct and `member_sources`
     field to `LocalSwarm` (backward-compatible via `#[serde(default)]`).
     `create` seeds `curated_seed` provenance; `add_member` adds `operator`
     provenance; `remove_member` prunes in sync.
   - Tests updated: `create_seeds_members`, `add_and_remove_member_roundtrip`.
   - New test: `legacy_swarm_json_without_member_sources_deserializes`.
