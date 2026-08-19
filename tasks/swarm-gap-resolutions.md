# Swarm Gap Resolutions

Tracked from the gap analysis (2026-08-18). Each item cites the evidence and
the minimal resolution path. Items 1-6 are the implementation order; item 7
is a follow-up UX examination.

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

## Items 1-6: Implementation

See the gap analysis above for evidence and resolution paths. Implementation
order (dependency-ordered):

1. **Gap 1:** Persist delegation response as `HMem` in `record_delegation`
   (`local_knowledge.rs`).
2. **Gap 2:** Add `swarm_search_knowledge_local` instruction to SENSE template
   (`swarm-sense.j2`).
3. **Gap 3:** Add `swarm_a2a_broadcast` tool (`a2a_tools.rs`).
4. **Gap 4:** Make `steering` default + add post-Act execute step in manifest
   (`swarm-intelligence.yaml`).
5. **Gap 5:** Add Jaccard redundancy pairs to SENSE Step 3 (`swarm-sense.j2`).
6. **Gap 6:** Add `membership_source` to `SwarmMember` (`local_swarms.rs`).
