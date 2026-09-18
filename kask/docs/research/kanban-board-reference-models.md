---
title: "Kanban Boards — Reference Models from Established Open-Source Implementations"
audience: [architects, developers, agents]
last_updated: 2026-09-18
version: "1.2.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, trust]
---

# Kanban Boards — Reference Models from Established Open-Source Implementations

This document records the reference models for kanban **boards as named
first-class containers** — how boards are named, listed, opened, renamed, and
related to their tasks — drawn from established open-source kanban
implementations, and assesses the kata-kanban (MCP server + GPUI panel)
against them. It exists because the operator reported a mismatch between the
kanban concept model, the GPUI panel, and the implementation: agents appeared
to use "the kanban board" as a generic single board, and opening the panel
did not present a named board.

The reference models below are the anchor for shaping and testing our kanban
surfaces. **Implemented 2026-09-18** — §10 records the implementation
state and evidence.

Version 1.1.0 is the reviewed revision: the v1.0.0 plan was stress-tested
through the `metacognition` (measured-vs-inferred audit), `grill-me` (edge-case
interrogation), `falsifiability` (discriminating tests for every claim), and
`refactor-architecture` (deletion test on the proposed controls) skills. Two
gaps were added (G8 identity drift, G9 name-validation), one over-claim was
corrected (G5), and the identity-surface design was consolidated. §10 records
the full review deltas.

## 1. Purpose and scope

- Record the converged reference model (invariants R1–R8, §4) with primary-
  source evidence, so kanban work can be anchored against something published
  rather than private taste.
- Assess the kata-kanban server (`kask/mcp-servers/hkask-mcp-kata-kanban/`)
  and panel (`crates/kanban_panel/`) against those invariants (§5).
- Provide the shaping recommendations (§6), capability deltas (§7), operator
  decisions (§8), and the falsifier-annotated test plan (§9).

Out of scope, with the divergence labeled: **cross-board task moves**. Kan,
Planka, and Wekan all let cards move or copy between boards; the kata-kanban's
status transitions are strictly within-board (see
[`diagrams/kanban.md`](../diagrams/kanban.md)) and the operator's report is
about board identity, not card mobility. Deferred — not forgotten.

## 2. Method and provenance

All external facts below were read directly from the upstream repositories on
2026-09-18 (files fetched from the default branch of each repo; star counts
observed on the repo landing pages the same day):

| Implementation | Stars | Primary sources fetched |
| --- | --- | --- |
| [wekan/wekan](https://github.com/wekan/wekan) (`master`) | 21.1k | `models/boards.js` |
| [plankanban/planka](https://github.com/plankanban/planka) (`master`) | 12.6k | `server/api/models/Board.js`; `client/src/components/boards/Boards/Boards.jsx`, `Boards/Item.jsx`, `AddBoardStep/AddBoardStep.jsx`; repo tree |
| [kanbn/kan](https://github.com/kanbn/kan) (`main`) | 5.7k | `packages/db/src/schema/boards.ts`; `packages/shared/src/utils/generateSlug.ts`; `packages/api/src/schemas/board.ts`; repo tree |
| Kanboard (docs, [docs.kanboard.org/v1/user/projects/](https://docs.kanboard.org/v1/user/projects/)) | — | user documentation page |

In-tree facts cite `path:line` at commit `4e421ce9db` (2026-09-18; the
v1.1.0 review re-verified the load-bearing ones, including the git history of
the selector hiding, the mermaid round-trip, the service validation code, and
the refresh cadence). The live board pool was observed via `kanban_board_list`
the same day. This document is a reconstruction from primary sources — it is
research, not an operator spec.

## 3. The reference implementations

### 3.1 Wekan — boards are titled, slug-addressed documents

Wekan's board schema (`models/boards.js`) carries:

- `title` — required `String`, "The title of the board".
- `slug` — auto-derived from the title at insert: `getSlug(title.value) ||
  'board'` (empty-slug fallback for scripts where slugification yields "").
- `originRelativeUrl()` returns `/b/${this._id}/${this.slug || 'board'}`,
  matching the `board` route `/b/:id/:slug` — **opening a board is opening a
  named thing**: the URL itself carries the name-slug.
- `rename(title)` is a first-class model method; `Boards.uniqueTitle` even
  deduplicates copied titles (`title [n]`), because the title is the identity
  users address boards by.
- `Boards.userBoards(userId, …)` scopes the boards a user may see — the boards
  list is the navigation surface, never an anonymous "current board".
- Every card/list/swimlane helper queries `{ boardId: this._id }` — there is
  no board-less card.

### 3.2 Planka — per-project board tabs, always visible, always named

Planka groups boards inside projects. The server model
(`server/api/models/Board.js`) carries `name` (required `String`,
"Name/title of the board"), `position` (required — ordered within its
project), and `projectId` (required). The client renders boards as a
horizontal tab strip at the top of the project view:

- `Boards.jsx` lists the current project's boards (`selectBoardIdsForCurrentProject`),
  drag-orderable, with an add-board affordance in the same strip.
- `Item.jsx` renders each tab as a `Link` whose text is `board.name`, with an
  `isActive` marker (`id === path.boardId`) — the open board's name is always
  visible, and even a not-yet-persisted board renders its name.
- `AddBoardStep.jsx` is a name-first create form: the submit **trims** the
  name and **refuses a name that is empty after trimming**, `maxLength={128}`.
- Rename is first-class: the board tab carries an edit pencil that opens
  `BoardSettingsModal/GeneralPane/EditInformation`.

### 3.3 Kan — name-derived unique slug; agent-facing boards are still named

Kan is a Trello-alternative that, notably for us, also ships an MCP server
(`packages/mcp`, `@kan/mcp`) so agents drive boards over the same model.
Its board schema (`packages/db/src/schema/boards.ts`) carries `name`
varchar(255) NOT NULL, `publicId` varchar(12) unique, `workspaceId` NOT NULL,
and `slug` varchar(255) NOT NULL under a partial unique index
`unique_slug_per_workspace` on `(workspaceId, slug)`. The slug is generated
from the name (`packages/shared/src/utils/generateSlug.ts`: lowercase, trim,
strip specials, spaces→hyphens). The API surface
(`packages/api/src/schemas/board.ts`) exposes:

- board list items carrying `publicId` + `name`;
- board detail carrying `name` + `slug`, and a `board.bySlug` fetch — boards
  are addressable by name-derived slug;
- `boardUpdateResponseSchema` including `{ publicId, name }` — rename is
  first-class.

The web routes are name-addressed (`apps/web/src/pages/[workspaceSlug]/[...boardSlug].tsx`),
the sidebar and boards list render boards by name
(`apps/web/src/views/boards/`, `SideNavigation.tsx`), and the board view
carries a switcher (`views/board/components/BoardDropdown.tsx`) plus a rename
surface (`BoardSettingsModal/GeneralPane/EditInformation`).

### 3.4 Kanboard — corroborating

Kanboard's boards *are* its projects, each with a required name ("It's very
easy: you just have to find a name for your project!"). Renaming is a
first-class operation ("To rename a project, click on the link Edit
project"), removal cascades ("Removing a project deletes all tasks that
belong to this project"), and the dashboard is the project list. One-click
board switching from the header is third-party-corroborated
(methodsandtools.com tool review) — treat as secondary evidence.

## 4. The converged reference model

Every surveyed implementation exhibits all of the following. These are the
invariants to anchor and test against:

| # | Invariant | Evidence |
| --- | --- | --- |
| R1 | **A board is a named container.** The name/title is required at creation, is the board's human identity, and is enforced non-empty (including after trimming). | Wekan `title` required; Planka `name` required + trim-and-refuse create form; Kan `name` NOT NULL; Kanboard name required |
| R2 | **The name is the addressing key.** Users (and agents, and shared URLs) refer to boards by name; identifiers exist but names are how boards are addressed. | Wekan route `/b/:id/:slug` (slug from title); Kan `unique_slug_per_workspace` + `board.bySlug`; Kanboard project switcher |
| R3 | **The board list is the navigation root.** You open a board by picking its name from a list/sidebar/tabs. No implementation opens an anonymous "current board". | Wekan boards list; Planka project board tabs; Kan sidebar + boards page; Kanboard dashboard |
| R4 | **The open board's name is displayed at the point of use.** | Planka `Item.jsx` name + active marker; Kan `BoardDropdown`; Wekan title-slug URL |
| R5 | **Every task/card belongs to exactly one board.** There is no board-less task. | Wekan `{ boardId }` card helpers; Planka board→lists→cards; Kan workspace→board→lists→cards |
| R6 | **Board rename is a first-class operation** (because the name is the addressing key). | Wekan `rename(title)`; Kan `board.update`; Planka `EditInformation`; Kanboard "Edit project" |
| R7 | **Create-board is name-first and lands you in the created board.** | Planka `AddBoardStep` name form + navigation; Kanboard named project creation; Kan `NewBoardForm` over NOT NULL `name` |
| R8 | **Deleting a board deletes its tasks** (no orphan tasks). | Kanboard docs; Planka delete-related helpers; Kan FK cascade; Wekan board remover |

Where the references **diverge**, the invariant is the shared commitment and
the divergence is a labeled implementation decision:

- **Name uniqueness**: Kan enforces it structurally (slug unique per
  workspace); Planka and Wekan allow duplicate names (Wekan deduplicates only
  copies). R2 does not require uniqueness — see §8 decision 2.
- **Switcher shape**: Planka uses always-visible named tabs; Kan uses a
  dropdown. Both satisfy R2–R4 — see §8 decision 1.

## 5. Alignment of the kata-kanban against the reference model

### 5.1 What matches

The **concept model and server implementation already satisfy** R1 (partially
— see G9), R5, R7 (name-first), and R8:

- `Board` carries a required human `name`
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/board.rs:12-25`),
  and `board_create` rejects an empty name and a zero-column board
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs:120-133`).
- `task_create` verifies the board exists and returns `NotFound` otherwise
  (`service_impl/service.rs:278-284`) — R5 is enforced at the service seam,
  not just the schema. (No test currently pins this; T3.)
- `kanban_task_create` requires a `board_id`
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:601`),
  and the board index prefix `BOARD_TASKS_PREFIX`
  (`service_impl/service.rs:49`) ties tasks to their board.
- The panel's create-board form is name-first (placeholder "Board name",
  `crates/kanban_panel/src/kanban_panel.rs:1426-1434`; submit refuses an
  all-whitespace name client-side, `crates/kanban_panel/src/task_actions.rs:638-641`).
- Board identity already round-trips through export/import: the mermaid
  export writes `%% kanban board: <name>`
  (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/mermaid.rs:183`) and the
  parser reads it back (`mermaid.rs:219-227`); the panel's import deliberately
  preserves the exported name (`task_actions.rs:776-780`).
- `kanban_board_delete` cascades to the board's tasks
  (`hkask_mcp_kata_kanban.rs:352-390`) — R8.
- The board h_mem is ontology-anchored as the PKO procedure root
  (`service_impl/service.rs:139-154`), with tasks carrying step identifiers —
  the in-tree vocabulary for R5. (Note: `onto_anchor("kanban board")` itself
  currently lands on the coarse 5W1H core rung; this document deliberately
  assigns no private definition — the PKO mapping above is the in-tree
  anchor.)
- Live evidence (2026-09-18): `kanban_board_list` returned four named boards,
  so the model layer functions end to end. The mismatch the operator
  reported lives in the **panel presentation layer**, not the model.

### 5.2 Gaps

| # | Gap | Violates | Site |
| --- | --- | --- | --- |
| G1 | The board switcher hides itself when there are zero or one boards — a single-board install renders as a nameless generic board. History check: this shipped with the panel's initial commit (`08ed8c68d2`); it is a greenfield default, not a later deliberate decision being reversed. | R3, R4 | `crates/kanban_panel/src/kanban_panel.rs:929-932` |
| G2 | The panel headline and the item tab text are always the generic "Kanban Board" — the open board's name is not carried at the panel identity level (two kanban tabs are indistinguishable). | R4 | `crates/kanban_panel/src/kanban_panel.rs:1457`, `:1565-1567` |
| G3 | Opening the panel silently auto-selects `boards[0]`. Measured: `board_list` sorts `created_at` descending (`service.rs:209`), so `boards[0]` is by construction the most recently created board. Inferred (labeled): from the observed pool, that is typically an agent session board rather than the operator's intended board. | R3 | `crates/kanban_panel/src/fetch.rs:86-92` |
| G4 | There is no scalable open-by-name affordance: the switcher is a flat label row (hidden at n≤1, unbounded width, no search), and no searchable picker exists despite the in-tree precedent of the PopoverMenu+Picker thread picker. | R2, R3 | `crates/kanban_panel/src/kanban_panel.rs:929-965`; pattern at `crates/hkask-steer/src/thread_picker.rs` |
| G5 | The Steer-mode prompt binds the agent to the active board by id only ("The active board is `{id}`"), omitting the name. Refined in v1.1.0: agents *can* see names — `kanban_board_list` returns them — the gap is that the steer binding never presents the name, so the agent's working context is id-addressed where every reference model is name-addressed. | R2 | `crates/kanban_panel/src/kanban_panel.rs:310-315` |
| G6 | Creating a board does not open it: `submit_create_board` refreshes the board list but never selects the created board (only the first-ever board is opened, accidentally, by G3's auto-select). The same holds for `import_board` (`RefreshTarget::Boards`, no selection). | R7 | `crates/kanban_panel/src/task_actions.rs:634-652`, `:760-788` |
| G7 | Board rename does not exist anywhere — no server tool and no panel affordance. | R6 | grep over `kask/mcp-servers/hkask-mcp-kata-kanban/` finds no `rename`/`board_update` (2026-09-18) |
| G8 | **Identity drift (new in v1.1.0).** Once a board is selected, the refresh tick only re-fetches tasks (`crates/kanban_panel/src/fetch.rs:217-238` picks one target per 10s tick, `kanban_panel.rs:93`), and `fetch_boards` replaces the board list without reconciling the selected board's name or columns (`fetch.rs:65-92` sets those only on auto-select). Consequence: an external rename (or the G7 tool, once it exists) never reaches an open panel — its identity surfaces freeze at selection time. Found by edge-case interrogation: rename without reconciliation would be visible-nowhere. | R4, R6 | `crates/kanban_panel/src/fetch.rs:65-92`, `:217-238` |
| G9 | **Inconsistent name validation (new in v1.1.0).** The service checks `name.is_empty()` without trimming (`service.rs:126-128`), so an MCP caller can create `"   "` and the panel — which only guards all-whitespace client-side but sends the untrimmed string (`task_actions.rs:638-644`) — can store `"  My Board  "`. Meanwhile the mermaid path trims both task titles (`mermaid.rs:115`) and board names (`mermaid.rs:222-227`). Reference: Planka trims and refuses. Falsifier: `kanban_board_create {"name": "   "}` succeeds today. | R1 | `service.rs:126-128`; `task_actions.rs:638-644` |

The widget does render the board's name in its own header
(`crates/hkask-kanban-widget/src/view.rs:232-253`), which is why this reads as
a presentation-layer mismatch rather than a model bug.

## 6. Shaping recommendations

Revised in v1.1.0. The headline change: the original "always render the
label row" patch is **replaced by a consolidation** — the deletion test
(refactor-architecture) was applied to the three identity surfaces, and the
label row fails it once a deeper control exists.

### 6.1 Consolidate the identity surfaces (G1, G2, G4 → R2, R3, R4)

Today the panel has three identity surfaces: the static headline
(`kanban_panel.rs:1457`), the label-row selector (hidden at n≤1,
`:929-965`), and the widget header name (kept — it is the chat-viz identity
surface, `view.rs:236`). Replace the first two with:

- **A named board control** in the toolbar: a dropdown-style button showing
  the selected board's name (or "Select a board…" / "No boards yet"),
  opening a searchable `BoardPicker` — the Kan `BoardDropdown` shape. One
  control carries all three duties: displays the open board's name (R4),
  opens the board list by name (R3), and scales to any board count (R2).
- **Headline carries the open board's name** when one is selected, generic
  otherwise — the strongest R4 surface.
- **Tab text** becomes `Kanban — {name}` when a board is selected, so
  multiple kanban panels on different boards are distinguishable.
- The label-row selector module is deleted, not patched. Planka-style
  always-visible tabs remain the labeled alternative (§8 decision 1).

### 6.2 BoardPicker module (G4 → R2, R3)

New `crates/kanban_panel/src/board_picker.rs` mirroring the
`ThreadPicker` shape (`crates/hkask-steer/src/thread_picker.rs`):
PopoverMenu + `Picker` delegate, fuzzy match over board names, `on_select`
→ `select_board(board_id)`. Placement is kanban_panel, not hkask-steer —
the steer crate is cross-panel conversation infrastructure and a board
picker is kanban's. Dependency direction: add `picker.workspace = true` and
`fuzzy.workspace = true` to `crates/kanban_panel/Cargo.toml` (both verified
workspace crates; hkask-steer consumes them identically). Duplicate names
(allowed per §8 decision 2) are disambiguated by a sub-label (task count /
created date), the way the thread picker shows timestamps.

### 6.3 Board rename (G7 → R6)

- Service: `KanbanService::board_rename(board_id, new_name)` — trim and
  reject empty-after-trim (the single validation point, closing G9 for
  rename), ownership check mirroring `kanban_board_delete`
  (`hkask_mcp_kata_kanban.rs:369-380`), update the board h_mem **value**
  preserving the PKO procedure ontology (the update path task moves already
  use, `service_impl/service.rs:887`), and emit a `board_renamed` REG span
  mirroring `board_created` (`service.rs:159-167`).
- Tool: thin `kanban_board_update` wrapper.
- **Replay safety (refined in v1.1.0):** rename is convergent by
  construction — replaying the same call re-applies the same name — so it
  joins the `task_update` class and needs **no** idempotency key. The
  panel's `IDEMPOTENT_TOOLS` list (`kanban_panel.rs:115-120`) stays
  unchanged, and its shape test
  (`idempotent_tools_are_creates_or_spawns`) still passes.
- Panel: a rename affordance on the identity control (the Planka
  edit-pencil analog). After success, panel state reconciles via §6.4, and
  an open Steer conversation is **invalidated** so its rebuild picks up the
  new name — the same mechanism `select_board` already uses on board switch
  (`kanban_panel.rs:915-922`).

### 6.4 Identity reconciliation (G8 → R4, R6)

- `fetch_boards` gains a reconcile step: after replacing `self.boards`,
  refresh the selected board's `name` and `columns` from the fresh row (the
  stale-selection clear at `fetch.rs:79-85` stays).
- The 10s refresh tick fetches **both** the board list and the task list
  (two local stdio calls — the current cadence is 10s, `kanban_panel.rs:93`,
  so the added cost is one `kanban_board_list` per tick per open panel).
  This makes renames and external board-list changes visible within one
  tick, everywhere.

### 6.5 Select-after-create and select-after-import (G6 → R7)

`submit_create_board` and `import_board` must open the created board.
Plumbing correction found in review: `dispatch_mutation` **discards
successful tool outputs** (`kanban_panel.rs:684-695`), so "capture the
created board id" needs a small extension — a response-carrying variant (or
`before_refresh`-style callback) for these two gestures that reads
`board_id` from the `kanban_board_create` / `kanban_board_import` response
and calls `select_board` once the refreshed list lands. Both tools are
already replay-protected (`IDEMPOTENT_TOOLS` includes
`BOARD_CREATE_TOOL` and `BOARD_IMPORT_TOOL`).

### 6.6 Name validation at the service boundary (G9 → R1)

`board_create` trims and rejects empty-after-trim — one enforcement point
for every caller (panel, MCP agents, future rename). The panel create path
then also sends the trimmed name (`task_actions.rs:638-644` currently sends
the untrimmed string). This matches Planka exactly and reuses the import
path's existing trim behavior (`mermaid.rs:222-227`).

### 6.7 Name-first Steer binding (G5 → R2)

The steer prompt clause becomes: "The active board is `{name}` (`{id}`). Use
this board id when creating or moving tasks." Name for understanding, id as
the dispatch key — the same split Kan's MCP tools use (names and publicIds
travel together).

## 7. Capability deltas (users and agents)

Fidelity check against what the tool sets and GPUI actually give each party
today. Nothing is removed; the plan is additive:

| Party | Has today | Gains from the plan |
| --- | --- | --- |
| Agents (MCP) | `board_create` (name-first), `board_list` (names), `board_delete`, `task_*`, goals, export/import, replay-protected creates | `kanban_board_update` (rename); steer working context that names the board instead of addressing it by bare id |
| Users (GPUI) | create/delete board, create/edit/spawn/assign/delete tasks, move chips, export/import, steer mode, refresh | named board identity everywhere (headline, tab, switcher); searchable open-by-name; create/import opens the board; rename affordance; identity auto-reconciliation within 10s |
| Deferred | — | Cross-board task moves (§1); last-board persistence across restarts (§8 decision 3) |

## 8. Operator decisions

1. **Switcher shape.** (A) Named dropdown + searchable picker — the
   recommendation: one deep control, scales to any board count, matches the
   in-tree ThreadPicker precedent and Kan's shape; switching costs a click.
   (B) Always-visible scrollable named tabs — Planka's shape: one-click
   visible switching, at the cost of layout risk with many boards in a GPUI
   panel width.
2. **Board-name uniqueness.** (A) No uniqueness constraint — the
   recommendation (Planka-faithful; keeps create non-blocking; the picker
   disambiguates duplicates with sub-labels). (B) Enforce unique names per
   owner (Kan-faithful; strongest R2, but creates rename-blocked failure
   modes for agents naming boards).
3. **Persist the last-opened board across restarts** via the currently-unused
   `SerializableItem` seam (`kanban_panel.rs:1586-1628`). (A) Defer — the
   recommendation: orthogonal to the reference-model gaps, the seam exists
   when wanted. (B) Include now.

## 9. Test plan — using the reference model as the oracle

Server-side (the `KanbanService`/`Parameters<T>` seams; the empty-name
rejection is enforced but **unpinned by any test** — grep for
name-validation tests finds none):

- **T1 (R1, red-first for G9)**: `board_create` rejects both `""` and
  whitespace-only names; a trimmed name is stored trimmed. Falsifier for the
  current code: `kanban_board_create {"name": "   "}` succeeds today.
- **T2 (R1)**: the panel's create gesture sends the trimmed name (red-first:
  `task_actions.rs:638-644` sends untrimmed today).
- **T3 (R5)**: `task_create` with an unknown board returns `NotFound`
  (`service.rs:278-284`) — correction (2026-09-18): this was already pinned
  by `task_create_rejects_unknown_board` in
  `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs`;
  verified passing, no new test needed.
- **T4 (R6)**: rename round-trip — create → rename → `board_list` returns
  the new name; rename by a non-owner rejected; empty-after-trim rejected;
  rename-to-same-name is a convergent no-op success.

Panel-side (extend the existing test module,
`crates/kanban_panel/src/kanban_panel.rs:1631-2061`, which today has
refresh/steer/stale-selection coverage but no board-identity coverage):

- **T5 (R4)**: `tab_content_text` and the headline carry the selected
  board's name; generic when nothing is selected.
- **T6 (R3, red-first)**: the identity control renders with exactly one
  board and shows that board's name (fails against `kanban_panel.rs:929-932`
  today).
- **T7 (R3)**: after `fetch_boards` auto-selects, `board_name` is set and
  rendered — auto-open names what it opened.
- **T8 (R7, red-first)**: `submit_create_board` selects the created board
  once the refreshed list lands; `import_board` likewise. Falsifier for the
  current code: both leave the selection unchanged today.
- **T9 (R2)**: the BoardPicker lists every board by name with duplicate
  disambiguation; confirming a match dispatches `select_board` with that
  board's id.
- **T10 (R2)**: the steer system prompt carries the active board's name and
  id (extend `steer_prompt_is_scoped_to_the_selected_board`,
  `kanban_panel.rs:1943-1966`).
- **T11 (G8, red-first)**: after the selected board's row changes in a
  board-list read (rename, external edit), the panel's identity surfaces show
  the new name within one refresh tick. Falsifier: today the identity never
  updates after selection.
- **T12 (E2)**: a rename invalidates an open Steer conversation; the rebuilt
  prompt carries the new name (mirror `select_board`'s invalidation).
- **T13 (R2, decision 2)**: duplicate names are allowed and disambiguated by
  the picker sub-label — or rejected, if the operator picks uniqueness.

## 10. Review record (v1.0.0 → v1.1.0) and implementation

### Implementation record (2026-09-18)

The v1.1.0 plan was implemented with the operator's three decision
defaults (dropdown + searchable picker; no name uniqueness; persistence
deferred):

- **Server (G7, G9)**: `kanban_board_update` (rename; owner-only, convergent,
  no idempotency key) with `KanbanService::board_rename` updating the board
  h_mem in place via the store's `update` path; `validate_board_name` trims
  and rejects empty-after-trim at the service boundary for create, rename,
  and import. Tool-seam tests (`tests/board_rename.rs`: owner round-trip,
  non-owner permission denial, whitespace rejection, unknown-board NotFound)
  and service tests (trim storage, whitespace rejection, rename round-trip
  preserving id/columns/created_at and task links, convergent same-name
  rename) all pass.
- **Panel (G1–G6, G8)**: the `≤1`-hidden label-row selector is **deleted**;
  a named board switcher (ui Button → `BoardPickerDelegate` popover,
  fuzzy-matched by name with duplicate-name id-suffix disambiguation in
  `crates/kanban_panel/src/board_picker.rs`) opens any board by name;
  headline and tab carry the open board's name (`panel_titles`); create and
  import capture the response's `board_id` and open the created board once
  its row lands (`pending_select_board` + `pending_board_to_select`);
  every 10s tick re-reads the board list so identity reconciles
  (`selected_identity_from_boards` + `RefreshTarget::BoardsAndTasks`);
  the Steer prompt binds `name` + `id`, and a rename invalidates the open
  Steer conversation.
- **Tests (T1–T12)**: 27 panel tests pass (new: panel titles, reconcile,
  pending-select, refresh-both-lists, steer name+id, rename convergent in
  the keyless class; updated: steer scoping, tick behavior); server suites
  pass. `cargo check -p zed` passes; clippy is clean for both crates
  (after repairing four `redundant_clone` errors in the
  `hkask-spreadsheet-widget` crate that were committed at HEAD and blocked
  the shared clippy gate for every downstream crate — gate repair, clearly
  outside the kanban change's scope).
- **Commit state**: partially harvested into `e3644b2904` (fetch.rs,
  Cargo.toml, part of the panel work, mixed with the spreadsheet stream's
  portfolio change); the remaining panel work (board_picker.rs,
  kanban_panel.rs, task_actions.rs) was uncommitted at report time.

### Review deltas (v1.0.0 → v1.1.0)

The plan was re-reviewed through the `metacognition`, `grill-me`,
`falsifiability`, and `refactor-architecture` lenses on 2026-09-18, with a
second verification pass over the tree. What changed and why:

- **New gaps from edge-case interrogation (grill-me):** G8 (identity drift —
  rename would be visible nowhere without a reconciliation path) and G9
  (whitespace-name enforcement is inconsistent across service, panel, and
  mermaid paths; the service does not trim).
- **Over-claim corrected (falsifiability):** G5 claimed agents "address
  boards by opaque ids, never names" — false as stated, since
  `kanban_board_list` returns names. Refined to what is measured: the steer
  binding omits the name.
- **Inference relabeled (metacognition):** G3's "typically an agent session
  board" is now labeled an inference from the observed pool; the measured
  part (auto-select picks the most recently created board, by the
  `created_at`-descending sort) is separated out.
- **History check (grill-me mechanism):** the `boards.len() <= 1` hiding
  shipped with the panel's initial commit (`08ed8c68d2`) — removing it
  reverses a greenfield default, not a considered later decision.
- **Consolidation (refactor-architecture deletion test):** the v1.0.0
  recommendation "always render the label row" kept two overlapping identity
  surfaces and did not scale; replaced by the single named board control +
  picker (§6.1), with the label-row module deleted rather than patched. The
  BoardPicker's home and dependency direction were fixed (kanban_panel, +
  `picker`/`fuzzy` workspace deps, not hkask-steer).
- **Plumbing correction (grill-me):** v1.0.0 said "capture the created
  board id from the response" — but `dispatch_mutation` discards successful
  outputs; §6.5 now names the extension needed.
- **Idempotency refined (falsifiability):** v1.0.0 said rename "requires the
  idempotency-safety review"; the review classifies rename as convergent by
  construction (the `task_update` class, per `kanban_panel.rs:102-108`) —
  no key needed, `IDEMPOTENT_TOOLS` unchanged.
- **Reference divergence surfaced:** name-uniqueness (Kan enforces; Planka
  does not) and switcher shape (tabs vs dropdown) are now labeled as
  labeled decisions (§8), not silent choices.
- **Fidelity inventory added:** capability deltas for users and agents (§7),
  with cross-board task moves recorded as a deferred divergence (§1).

## 11. See also

- [`diagrams/kanban.md`](../diagrams/kanban.md) — task-status lifecycle and
  the widget move controller.
- `kask/mcp-servers/hkask-mcp-kata-kanban/README.md` — the kanban tool
  surface.
- [`chunking-for-rag-research.md`](chunking-for-rag-research.md) — the
  prior-art research-document pattern this file follows.