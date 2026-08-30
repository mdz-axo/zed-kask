# Rust coding guidelines

* Prioritize code correctness and clarity. Speed and efficiency are secondary priorities unless otherwise specified.
* Do not write organizational or comments that summarize the code. Comments should only be written in order to explain "why" the code is written in some way in the case there is a reason that is tricky / non-obvious.
* Prefer implementing functionality in existing files unless it is a new logical component. Avoid creating many small files.
* Avoid using functions that panic like `unwrap()`, instead use mechanisms like `?` to propagate errors.
* Be careful with operations like indexing which may panic if the indexes are out of bounds.
* Never silently discard errors with `let _ =` on fallible operations. Always handle errors appropriately:
  - Propagate errors with `?` when the calling function should handle them
  - Use `.log_err()` or similar when you need to ignore errors but want visibility
  - Use explicit error handling with `match` or `if let Err(...)` when you need custom logic
  - Example: avoid `let _ = client.request(...).await?;` - use `client.request(...).await?;` instead
* When implementing async operations that may fail, ensure errors propagate to the UI layer so users get meaningful feedback.
* Never create files with `mod.rs` paths - prefer `src/some_module.rs` instead of `src/some_module/mod.rs`.
* When creating new crates, prefer specifying the library root path in `Cargo.toml` using `[lib] path = "...rs"` instead of the default `lib.rs`, to maintain consistent and descriptive naming (e.g., `gpui.rs` or `main.rs`).
* Avoid creative additions unless explicitly requested
* Use full words for variable names (no abbreviations like "q" for "queue")
* Use variable shadowing to scope clones in async contexts for clarity, minimizing the lifetime of borrowed references.
  Example:
  ```rust
  executor.spawn({
      let task_ran = task_ran.clone();
      async move {
          *task_ran.borrow_mut() = true;
      }
  });
  ```

# Timers in tests

* In GPUI tests, prefer GPUI executor timers over `smol::Timer::after(...)` when you need timeouts, delays, or to drive `run_until_parked()`:
  - Use `cx.background_executor().timer(duration).await` (or `cx.background_executor.timer(duration).await` in `TestAppContext`) so the work is scheduled on GPUI's dispatcher.
  - Avoid `smol::Timer::after(...)` for test timeouts when you rely on `run_until_parked()`, because it may not be tracked by GPUI's scheduler and can lead to "nothing left to run" when pumping.

# GPUI

GPUI is a UI framework which also provides primitives for state and concurrency management.

## No `block_on` on the foreground thread

Never use `futures::executor::block_on` (or any blocking executor) inside a sync trait method that is called from the GPUI foreground thread. If the underlying future needs the GPUI or tokio executor to make progress, `block_on` will deadlock. Instead, make the trait method async (return `Pin<Box<dyn Future>>`) and await it in an async context like `run_turn_internal`.

## Mutating `Arc<Message>` in `Thread.messages`

When injecting deferred tool results into a stored `Arc<Message>`, do not use `Arc::get_mut` — it silently returns `None` when the Arc is shared (e.g., after a DB save clones the message), dropping the result. Instead, read the `AgentMessage` from the `Arc`, clone it (`AgentMessage` derives `Clone`), modify the clone, and replace the `Arc` in the vector.

## Deferred results and the turn loop

The `end_turn` path in `run_turn_internal` must not busy-spin or use timers to wait for pending deferred tool results — the GPUI test scheduler's `run_until_parked` will fail with "Parking forbidden" if a foreground task is alive but waiting. If deferred results are pending at `end_turn`, let the turn end. The results will be drained on the next turn iteration (triggered by a user message or `cx.notify()`).

## Context

Context types allow interaction with global state, windows, entities, and system services. They are typically passed to functions as the argument named `cx`. When a function takes callbacks they come after the `cx` parameter.

* `App` is the root context type, providing access to global state and read and update of entities.
* `Context<T>` is provided when updating an `Entity<T>`. This context dereferences into `App`, so functions which take `&App` can also take `&Context<T>`.
* `AsyncApp` and `AsyncWindowContext` are provided by `cx.spawn` and `cx.spawn_in`. These can be held across await points.

## `Window`

`Window` provides access to the state of an application window. It is passed to functions as an argument named `window` and comes before `cx` when present. It is used for managing focus, dispatching actions, directly drawing, getting user input state, etc.

## Entities

An `Entity<T>` is a handle to state of type `T`. With `thing: Entity<T>`:

* `thing.entity_id()` returns `EntityId`
* `thing.downgrade()` returns `WeakEntity<T>`
* `thing.read(cx: &App)` returns `&T`.
* `thing.read_with(cx, |thing: &T, cx: &App| ...)` returns the closure's return value.
* `thing.update(cx, |thing: &mut T, cx: &mut Context<T>| ...)` allows the closure to mutate the state, and provides a `Context<T>` for interacting with the entity. It returns the closure's return value.
* `thing.update_in(cx, |thing: &mut T, window: &mut Window, cx: &mut Context<T>| ...)` takes a `AsyncWindowContext` or `VisualTestContext`. It's the same as `update` while also providing the `Window`.

Within the closures, the inner `cx` provided to the closure must be used instead of the outer `cx` to avoid issues with multiple borrows.

Trying to update an entity while it's already being updated must be avoided as this will cause a panic.

`WeakEntity<T>` is a weak handle. It has `read_with`, `update`, and `update_in` methods that work the same, but always return an `anyhow::Result` so that they can fail if the entity no longer exists. This can be useful to avoid memory leaks - if entities have mutually recursive handles to each other they will never be dropped.

## Concurrency

All use of entities and UI rendering occurs on a single foreground thread.

`cx.spawn(async move |cx| ...)` runs an async closure on the foreground thread. Within the closure, `cx` is `&mut AsyncApp`.

When the outer cx is a `Context<T>`, the use of `spawn` instead looks like `cx.spawn(async move |this, cx| ...)`, where `this: WeakEntity<T>` and `cx: &mut AsyncApp`.

To do work on other threads, `cx.background_spawn(async move { ... })` is used. Often this background task is awaited on by a foreground task which uses the results to update state.

Both `cx.spawn` and `cx.background_spawn` return a `Task<R>`, which is a future that can be awaited upon. If this task is dropped, then its work is cancelled. To prevent this one of the following must be done:

* Awaiting the task in some other async context.
* Detaching the task via `task.detach()` or `task.detach_and_log_err(cx)`, allowing it to run indefinitely.
* Storing the task in a field, if the work should be halted when the struct is dropped.

A task which doesn't do anything but provide a value can be created with `Task::ready(value)`.

## Elements

The `Render` trait is used to render some state into an element tree that is laid out using flexbox layout. An `Entity<T>` where `T` implements `Render` is sometimes called a "view".

Example:

```
struct TextWithBorder(SharedString);

impl Render for TextWithBorder {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().border_1().child(self.0.clone())
    }
}
```

Since `impl IntoElement for SharedString` exists, it can be used as an argument to `child`. `SharedString` is used to avoid copying strings, and is either an `&'static str` or `Arc<str>`.

UI components that are constructed just to be turned into elements can instead implement the `RenderOnce` trait, which is similar to `Render`, but its `render` method takes ownership of `self` and receives `&mut App` instead of `&mut Context<Self>`. Types that implement this trait can use `#[derive(IntoElement)]` to use them directly as children.

The style methods on elements are similar to those used by Tailwind CSS.

If some attributes or children of an element tree are conditional, `.when(condition, |this| ...)` can be used to run the closure only when `condition` is true. Similarly, `.when_some(option, |this, value| ...)` runs the closure when the `Option` has a value.

## Input events

Input event handlers can be registered on an element via methods like `.on_click(|event, window, cx: &mut App| ...)`.

Often event handlers will want to update the entity that's in the current `Context<T>`. The `cx.listener` method provides this - its use looks like `.on_click(cx.listener(|this: &mut T, event, window, cx: &mut Context<T>| ...)`.

## Actions

Actions are dispatched via user keyboard interaction or in code via `window.dispatch_action(SomeAction.boxed_clone(), cx)` or `focus_handle.dispatch_action(&SomeAction, window, cx)`.

Actions with no data are defined with the `actions!(some_namespace, [SomeAction, AnotherAction])` macro call. Otherwise the `Action` derive macro is used. Doc comments on actions are displayed to the user.

Action handlers can be registered on an element via the event handler `.on_action(|action, window, cx| ...)`. Like other event handlers, this is often used with `cx.listener`.

## Notify

When a view's state has changed in a way that may affect its rendering, it should call `cx.notify()`. This will cause the view to be rerendered. It will also cause any observe callbacks registered for the entity with `cx.observe` to be called.

## Entity events

While updating an entity (`cx: Context<T>`), it can emit an event using `cx.emit(event)`. Entities register which events they can emit by declaring `impl EventEmitter<EventType> for EntityType {}`.

Other entities can then register a callback to handle these events by doing `cx.subscribe(other_entity, |this, other_entity, event, cx| ...)`. This will return a `Subscription` which deregisters the callback when dropped.  Typically `cx.subscribe` happens when creating a new entity and the subscriptions are stored in a `_subscriptions: Vec<Subscription>` field.

## Build guidelines

- Use `./script/clippy` instead of `cargo clippy`

# Pull request hygiene

When an agent opens or updates a pull request, it must:

- Use a clear, correctly capitalized, imperative PR title (for example, `Fix crash in project panel`).
- Avoid conventional commit prefixes in PR titles (`fix:`, `feat:`, `docs:`, etc.).
- Avoid trailing punctuation in PR titles.
- Optionally prefix the title with a crate name when one crate is the clear scope (for example, `git_ui: Add history view`).
- Include a `Release Notes:` section as the final section in the PR body.
- Use one bullet under `Release Notes:`:
  - `- Added ...`, `- Fixed ...`, or `- Improved ...` for user-facing changes, or
  - `- N/A` for docs-only and other non-user-facing changes.
- Format release notes exactly with a blank line after the heading, for example:

```
Release Notes:

- N/A
```

# Crash Investigation

## Sentry Integration
- Crash investigation prompts: `.factory/prompts/crash/investigate.md`
- Crash fix prompts: `.factory/prompts/crash/fix.md`
- Fetch crash reports: `script/sentry-fetch <issue-id>`
- Generate investigation prompt from crash: `script/crash-to-prompt <issue-id>`

# Sankey diagrams

* Sankey conservation is domain-dependent, not universal. The original engineering Sankey (Sankey 1898, Schmidt 2008) requires conservation of energy/mass, but user-journey funnels, attribution paths, and value-stream maps do not conserve. A Sankey skill must carry a per-domain conservation mode (mandatory/asserted/none), not a single global rule. Assuming universal conservation produces silent "balancing" that fabricates loss branches — the exact failure mode the "never fabricate" rule exists to prevent.
* When a prompt references an external source (URL, financial statement, codebase), delegate extraction to a specialized skill (structured-extraction, sequential-inquiry — do not ask the user to transcribe data that already exists in a machine-readable source. Transcription requests shift cognitive load to the user and introduce transcription errors.
* Never fabricate Sankey weights. A Sankey's entire semantic value is that link width encodes flow magnitude. A fabricated width is a visualization of the LLM's guess, which is worse than no diagram. If the user declines to provide a weight, mark the edge as `value=1` (unitless placeholder) and note it in the description.

# Rules Hygiene

These `.rules` files are read by every agent session. Keep them high-signal.

## After any agentic session
If you discover a non-obvious pattern that would help future sessions, include a **"Suggested .rules additions"** heading in your PR description with the proposed text. Do **not** edit `.rules` inline during normal feature/fix work. Reviewers decide what gets merged.

## High bar for new rules
Editing or clarifying existing rules is always welcome. New rules must meet **all three** criteria:
1. **Non-obvious** — someone familiar with the codebase would still get it wrong without the rule.
2. **Repeatedly encountered** — it came up more than once (multiple hits in one session counts).
3. **Specific enough to act on** — a concrete instruction, not a vague principle.

Rules that apply to a single crate belong in that crate's own `.rules` file, not the repo root.

## What NOT to put in `.rules`
Avoid architectural descriptions of a crate (module layout, data flow, key types). These go stale fast and the agent can gather them by reading the code. Rules should be **traps to avoid**, not **maps to follow**.

## No drive-by additions
Rules emerge from validated patterns, not one-off observations. The workflow is:
1. Agent notes a pattern during a session.
2. Team validates the pattern in code review.
3. A dedicated commit adds the rule with context on *why* it exists.

# Center-pane Item Toggle vs ToggleFocus

For center-pane `Item` views (not dock `Panel`s), the View menu entry must use
the `Toggle` action (deploys a new item if none exists), NOT `ToggleFocus`
(silent no-op when no item is open — produces "nothing happens" with no error).
Dock `Panel`s register `ToggleFocus` as their `toggle_action`, so the menu
pattern differs between docks and center-pane items.

## Center-pane `Item` deploy-and-focus

When a `Toggle` action handler for a center-pane `Item` deploys a new item via
`workspace.add_item_to_active_pane(Box::new(page), None, true, window, cx)`,
the `activate = true` flag activates the item in the pane but does NOT transfer
keyboard focus to it on the same turn if the item's `Focusable::focus_handle`
impl delegates to a child entity constructed inside `cx.new` (e.g. an inner
`Editor`). The child's `FocusHandle` isn't reachable through the workspace
focus chain until a subsequent turn. Symptom: the user must click the menu
entry / status bar button twice — the first click adds the item, the second
click hits the `existing` branch and focuses via `activate_item`.

Fix: after `add_item_to_active_pane`, explicitly call
`page.focus_handle(cx).focus(window, cx)` on the newly created entity. Clone
the `Entity` before boxing it so the handle remains available:

```rust
let page = MyPage::new(workspace, window, cx);
workspace.add_item_to_active_pane(Box::new(page.clone()), None, true, window, cx);
page.focus_handle(cx).focus(window, cx);
```

This is NOT needed when `Focusable::focus_handle` returns a stable field
created in the constructor (e.g. `self.focus_handle.clone()`), because that
handle is reachable immediately. The trap is specific to delegated focus
handles (child `Editor`, child `Input`, etc.).

# zed-kask integration traps

These traps recur when modifying the Kask↔Zed seam. Each was hit multiple
times across audit cycles 3–6.

## Tests must pin deliberate zed-kask deviations from upstream

`DIVERGENCE.md` defines the divergence surface: everything under `kask/` is
ours; everything else tracks upstream Zed and is only touched via the named
D-seams. When you find a problem in an upstream file (anything outside
`kask/` and outside the D1–D14 seams), do not "fix" it speculatively —
renaming, reformatting, or "correcting" upstream files creates merge noise
and risks silent breakage on the next upstream merge. Push the fix into a
`kask/` crate behind a D-seam instead. If an upstream edit is genuinely
unavoidable, add a new D-seam entry to `DIVERGENCE.md` in the same PR and
pin the deviation with a test (below). File an upstream issue for real bugs;
don't fork-fix.

Example: `crates/vim/test_data/*.json` files are JSONL despite the `.json`
extension. This is upstream's format, loaded by `read_test_data` in
`crates/vim/src/test/neovim_connection.rs`. Renaming them to `.jsonl` would
touch 307 upstream files for zero kask benefit and conflict with every
upstream vim-test change. Leave them alone.

Every `// zed-kask:` comment that disables upstream behavior needs a
corresponding test asserting the disabled behavior stays disabled. The
upstream tests still assert the old behavior and will fail silently (CI
normalizes them as "pre-existing failures"). When you change a production
path with a `// zed-kask:` comment, grep the test files for the old
assertions and update them in the same commit. Cycles 3 and 4 both found
stale tests asserting upstream behavior that zed-kask deliberately removed
(body injection, catalog budget, description-length warnings).

## Process-global hooks set at runtime need a startup-failure signal

Every `set_*` hook in `crates/agent/src/agent.rs` and `crates/zed/src/main.rs`
that's wired conditionally must `log::warn!` when the condition fails, not
silently leave the hook `None`. The pattern: `resolve_x().unwrap_or_default()`
or `if condition { set_hook(Some(...)) }` without an `else` branch leaves
the hook absent with no signal. Operators reading logs cannot distinguish
"not configured" from "configured but broken". Always add a `log::warn!` in
the failure branch naming the hook, the failure reason, and the remediation
(`set HKASK_*` env var, open the panel, etc.). Cycle 5 found the
`a2a_secret` resolution path silently producing an empty secret; the same
pattern exists for `TOOL_ROUTER`, `CONTEXT_INJECTOR`, and `THREAD_CONDENSER`.

`OnceLock`-based hooks (`set_context_injector`,
`set_curator_context_injector`, `set_template_base_path`) must `log::warn!` on the `Err` branch of
`OnceLock::set` — a second call (e.g. deferred task re-firing) is silently
dropped without the warn. `Mutex`-based hooks (`set_memory_port`,
`set_thread_condenser`, `set_tool_invoker`, `set_tool_router`,
`set_metacognition_provider`) are re-settable and don't need the warn (the
second call replaces the first).

When a deferred task wires multiple `set_*` hooks inside a single `if`
block, the `else` branch warn must name ALL hooks left unwired, not just
one. An operator reading the log sees "context injector not wired" and
misses that 4 other hooks are also unwired. The warn is the effector that
drives operator remediation; if it only names 1 of 5 hooks, the operator
cannot remediate correctly.

This also covers startup config reads, not just `set_*` hooks: a numeric env
var (budget cap, threshold, limit) read at startup that fails to parse must
`log::warn!` naming the malformed value, not silently fall back to
disabled/default via a `.filter(|c| *c > 0)?` / `.and_then(parse)?` chain —
otherwise the operator cannot distinguish "not configured" from "configured
but broken" (the same trap as a missing `else` branch).

This also covers runtime feature gates, not just startup config: an opt-in
`HKASK_USE_*` feature that fails silently via `.ok()?` (collapsing 401, 429,
500, timeout, and malformed-response to `None` with no log) leaves the
operator unable to distinguish "not configured" from "configured but broken."
The operator set the env var — they opted in — so silence is a broken
feedback loop, not graceful degradation. Log the failure classification (HTTP
status, error variant) at each fallible step; `.ok()?` is for the fallback
(the caller proceeds without the feature), `tracing::warn!` is for the
diagnostic (it precedes the `?`).

## Cross-thread GPUI communication uses channels, not `AsyncApp` handles

`AsyncApp` is not `Send` (GPUI is single-threaded; it holds `Rc`/`Weak<AppCell>`).
Background tokio tasks that need to dispatch to the GPUI foreground must use a
`tokio::sync::mpsc` channel with a foreground drainer spawned via
`cx.spawn(async move |cx| { while let Some(...) = rx.recv().await { ... } })`,
not capture `AsyncApp`. This applies to any `Send + Sync` trait (e.g.
`RegulationSink`, `AlertSink`) implemented over GPUI state. Cycle 6 found
this the hard way — the first `ToastAlertSink` impl captured `AsyncApp` and
failed to compile as `Send + Sync`.

## `background_spawn` of tokio-dependent futures panics at poll time

`cx.background_spawn(async move { ... })` schedules on GPUI's own
thread-pool executor, which has no tokio reactor. Any future that drives
`reqwest`, `tokio::time`, `tokio::process`, or `tokio::io` panics with
"there is no reactor running, must be called from the context of a Tokio
1.x runtime" when polled on the worker thread. The `let _guard =
Tokio::handle_async(&*cx).enter();` pattern around `background_spawn` does
NOT help — `enter()` sets a thread-local for the current scope, but the
future is polled later on a different thread where the guard is gone. Use
`gpui_tokio::Tokio::spawn(&*cx, async move { ... })` instead (returns
`Task<Result<R, JoinError>>`; handle the `Err(JoinError)` arm). The
`main.rs` comment block at the cybernetics-loop spawn documents this, but
agents editing other files (MCP launchers, panel helpers, deferred-task
additions) miss it. Before merging any kask change that adds a
`background_spawn`, grep the future body for `reqwest`, `tokio::`, or any
`await` on a tokio-backed client — if found, route through `Tokio::spawn`.

## Kask MCP servers have two parallel launch paths by design

Kask MCP servers (`hkask-mcp-{id}` binaries) are launched by two independent
systems. Do not try to unify them — they serve different consumers with
different scoping and governance requirements:

1. **`McpRuntime` (app-global)** — launches one copy of each server for
   governed dispatch (call metering, `reg.tool.*` span emission). Serves the
   skill system and kask panel. Runs outside any project context.

2. **`ContextServerStore` (per-project)** — each project launches its own
   copies via `ContextServerDescriptorRegistry` descriptors (registered by
   `sync_kask_mcp_servers` in `main.rs`). Serves the agent tool picker.
   Project-scoped, no governance membrane.

The `ContextServerDescriptorRegistry` is app-level (global), but the
`ContextServerStore` that actually spawns processes is per-project. Both
systems launching independent process instances is correct — removing either
path breaks its consumers (removing `McpRuntime` breaks skill tool calls;
removing `ContextServerStore` registration hides kask tools from the agent).

The `KaskMcpDescriptor::command()` method resolves env vars (credentials,
inference socket) at call time. After `INFERENCE_SOCKET_PATH` is set (in a
deferred task post-login), `sync_kask_mcp_servers` must be called again so
the registry notifies `ContextServerStore` to restart servers with the
updated env.

## Model-dependent kask wiring: template base path vs. user-dependent hooks

The `set_template_base_path` hook is `OnceLock`-based and depends on
the kask data directory being resolved. The template base path is
resolved from the kask data dir (prod) or the repo's
`kask/registry/templates/` (dev), not from Zed cloud auth — so it must
NOT be gated on Zed user login. It is wired by a separate `cx.spawn` task
that runs at startup, independent of user login.

`set_thread_condenser` and `set_memory_port` are `Mutex`-based (re-settable)
and are wired twice: once unconditionally (pre-login, so the condenser works
before the model resolves) and once in the deferred task (post-login, to
upgrade). The `Mutex` pattern allows the deferred call to replace the early
one.

`set_tool_invoker` (in `crates/swarm_panel/src/tool_invoker.rs`) is `Mutex`-based
and wired once in the deferred task — it only needs the `tool_port`, which is
available before the model resolves, but the swarm panel's lifecycle actions
are rarely used before login so the single deferred wiring is sufficient.

Note: `set_curator_session_factory`, `set_regulation_status`, and
`set_scoped_inference` were removed when curator turns were routed through
`NativeAgent` (the `ConversationView` handles streaming + tool dispatch).
Do not re-add them — they have no consumer.

## LazyToolRouter filters MCP tools only

The `LazyToolRouter` in `crates/agent/src/tool_router.rs` was introduced to
tame MCP tool floods, not to filter built-in zed tools.
`Thread::enabled_tools` must skip built-in tools (those in
`crate::tools::ALL_TOOL_NAMES`) when applying the router — filtering them
caused the agent to lose access to `fetch`, `diagnostics`, `list_directory`,
etc. on ordinary coding requests, with the model discovering the loss only via
"tool not found" errors mid-turn. The filtering logic is extracted into
`tool_router::apply_router_bypassing_built_ins` for testability without the
process-global `TOOL_ROUTER` `OnceLock`.

## Skill invocation is `skill` tool call, not `read_file(SKILL.md)`

When a user asks to "run", "apply", "use", or "invoke" a skill on a target
(e.g., "run skill-maintenance on each of the 42 skills"), the correct agent
action is a single `skill` tool call with the skill's name and the target as
context. Do not `read_file` the `SKILL.md` body and improvise the methodology
— the SKILL.md body encodes the methodology; the catalog `description` only tells the
agent which skill to invoke. The system-prompt skills section
(`crates/agent/src/templates/system_prompt.hbs`) pins this for the model;
this rule pins it for agent sessions that read `.rules`. The failure mode is
the agent `read_file`-ing `~/.agents/skills/<name>/SKILL.md` and then
researching/questioning the skill instead of invoking it — observed
when asked to run `skill-maintenance` across the corpus.

## Skill body injection carries the user's task

Both `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs`) and
`NativeAgent::send_skill_invocation` (`crates/agent/src/agent.rs`) must inject
the user's task into the skill context as `task` (a
`serde_json::Value::String`). The SKILL.md body references `{{ task }}` to act on the
user's request. Passing an empty context causes the skill to
run blind — the body gets model defaults but never the actual request,
producing generic outputs unrelated to the user's intent. The `SkillToolInput.task`
field exists for this purpose; do not remove it or bypass it. The failure mode
is silent (no error, just wrong output) and was present at both call sites
before the fix.

## Convention priors drawn from .rules must be verified against the codebase

A `.rules` trap used as a "convention prior" (expected-field model for
gradient analysis or audit) must be verified against the codebase before
use. `.rules` entries can be stale — renamed functions, inlined variables,
removed hooks. A convention prior that references a nonexistent artifact
produces false gradients or wastes audit cycles hunting for a symbol that
doesn't exist. Before treating a `.rules` trap as the expected field, grep
for the artifact name in `crates/` — if no match, the `.rules` entry is
itself a finding (stale refactor, topological hole).

## Cargo.toml deps outlive their consumers

When a commit deletes a source file, grep the crate's `Cargo.toml` deps
against the remaining `src/` in the same commit — including the workspace
`Cargo.toml` for `[workspace.dependencies]` entries, which become transitive
orphans when their sole consumer crate drops them. A dep whose only `use`
statements lived in the deleted file stays declared silently — `cargo check`
passes because the dep is simply never imported, and `cargo machete`/`cargo
udeps` are not in the default toolchain. The fix is mechanical: for each dep,
grep `src/` and `tests/` for `use <dep>` and `<dep>::`; zero hits = remove.

## Kask settings defaults must live in `Default` impls — the single source of truth

Do not encode kask settings defaults in `#[serde(default = "...")]` attributes
on `KaskSettings` or its subsection structs (dead code — the settings system
deserializes `SettingsContent`, not `KaskSettings`, so the serde attributes
never fire), in `From<Content>` literals (use `unwrap_or(default.field)` where
`default = Self::default()`), or in `mcp_env()` comparison literals (compare
against `Default::default().field`, not a magic number). The drift class
(serde says `true`, `Default` says `false`, `mcp_env` says `1024`) silently
disabled all 10 kask MCP servers when users omitted `kask.mcp` and panicked in
`EmbeddingStore::from_driver` when users omitted `kask.corpus`. `Default` is
the single source of truth; `From` and `mcp_env` read from it. The settings UI
(`kask_page.rs`) must also resolve via `From<Content>` rather than applying its
own `unwrap_or(false)` — otherwise the UI shows toggles as off while the
runtime sees them as on (the inference-providers display bug). All 14 kask
settings UI sub-pages resolve `Content` → `Settings` via `From` and read
concrete fields — no inlined `unwrap_or` defaults in the UI.

## Model-name constants must not be duplicated across crates

Model-name defaults in kask_bridge and hkask-services-core must reference
`hkask_inference::model_constants::DEFAULT_*_MODEL` as `const` references,
not re-declare literals. The drift class (kask-bridge said
`openrouter/z-ai/glm-5.2`, model_constants said `OpenRouter/z-ai/glm-5.2`,
kask-bridge embedding default was a chat model) silently routed embedding calls
to a chat model and broke provider-prefix matching on case. The existing
"Kask settings defaults must live in `Default` impls" rule governs intra-struct
placement; this rule governs cross-crate duplication of the same constant.

## Kask MCP server credentials and config are scoped per-server

Each `BuiltinMcpServer` in `kask_bridge::mcp_servers` has two allowlists:
`credentials` (secrets from the keychain) and `config_env` (non-secret config
from `mcp_env()`). `KaskMcpDescriptor::command()` and the `McpRuntime` launch
path both call `filter_credentials_for_server` and
`filter_config_env_for_server` to filter env vars before injecting them into
the child process. A server that only needs `OPENROUTER_API_KEY` must not
receive `HKASK_SMTP_PASSWORD` (credential leak) or `HKASK_SMTP_USERNAME`
(config leak) — a compromised MCP server process has access to every env var
in its process. New servers must use `Some(&[])` (no credentials/config) for
both allowlists and add specific env vars as needed, never `None` (receives
all). The `all_servers_have_credential_allowlist` test pins this.

## MCP server allowlists must align with actual env-var reads

Every env var in a server's `config_env`/`credentials` allowlist must be read
by that server, and every env var the server reads via `std::env::var` or
`ctx.credentials` must be in the allowlist. The
`all_servers_have_credential_allowlist` test only checks `Some(&[])` minimum
shape — it does not check content alignment. Over-granting leaks config to a
compromised child process; under-granting silently drops operator overrides
(server falls back to default). The existing "scoped per-server" rule governs
allowlist shape; this rule governs content alignment.

## `unwrap_or(0)` on regulation-loop sense inputs is a broken feedback loop

Regulation loops (`CyberneticsLoop`, `MetacognitionLoop`,
`ConsolidationService`) read sense inputs from fallible storage queries.
A DB outage that returns 0 via `unwrap_or(0)` is interpreted by the loop
as "no deviation from set-point" — the opposite of the truth. The loop
sees no signal and suppresses its corrective action, producing a
reinforcing loop instead of a corrective one. This generalizes the
"Process-global hooks need a startup-failure signal" trap: every
`unwrap_or(0)` on a regulation signal is a missing failure signal.

The previously-cited sites have been fixed — `consolidation_service.rs`
`semantic_low_confidence_count` / `semantic_h_mem_count` (L114, L132) now
use explicit `match` + `tracing::warn!` ("signal stale, returning 0"),
and `runtime.rs` `variety_for_domain` (L312) now emits `tracing::warn!`
for untracked domains. The trap remains a forward-looking guideline for
new sense inputs: when you add a sense input to a regulation loop, either
propagate the error so the loop can mark the signal stale, or document the
degradation in the doc comment and emit a `tracing::warn!` so an operator
reading logs can distinguish "measured zero" from "failed to measure".
Do not use `unwrap_or(0)` on a signal that a loop reads as a measurement
unless the degradation is documented in the doc comment.

## `LanguageModelInferencePort` honors `model_override` via registry resolution

`LanguageModelInferencePort::generate_with_model` resolves the named model
from `LanguageModelRegistry` when `model_override` is `Some(name)`. The
resolution happens inside the GPUI-side receiver task (which has `&mut
AsyncApp` → `cx.update(|cx| ...)`). If the model can't be resolved, the port
falls back to the default model with a `tracing::warn!`. MCP servers
(condenser, corpus, training) that pass `Some(model)` to `generate_with_model`
expect the override to be honored — silently dropping it (the pre-fix
behavior) caused the condenser to use the default model instead of the
requested one. Tests in `inference_chat.rs` pin the propagation.

**`generate_stream` does not honor `model_override`.** The `generate_stream`
trait override on `LanguageModelInferencePort` hardcodes `model_override:
None` in the `StreamInferenceRequest`. The cascade's `call_inference_stream`
calls `generate_stream` (no override), so this is not triggered today.
`generate_stream_with_model` (added 2026-08) threads `model_override`
through `StreamInferenceRequest.model_override` to `handle_streaming`,
which calls `resolve_model` with the override — so streaming + override
preserves the live trace. (The prior claim that it fell back to
non-streaming `generate_with_model` was stale.)

## `LanguageModelProvider` registry subscriptions must filter self-events

A `LanguageModelProvider` that subscribes to `LanguageModelRegistry` events
must filter out `ProviderStateChanged`/`AddedProvider`/`RemovedProvider`
events carrying its own provider id. The registry observes every provider's
`LanguageModelProviderState` entity, so `cx.notify()` on that entity emits a
`ProviderStateChanged` the provider's own subscription receives — a
structural self-trigger loop, not a subscription bug. Left unfiltered,
this produces an unbounded warn storm (one warn per provider-state change
per loop iteration, hundreds of identical lines in the log).

## Trait-with-one-impl is speculative generality

A `trait` with a single implementor in the same crate, where the implementor
is constructed but never read, is dead code regardless of ADR-042 "port
promotion" aspirations. The port promotion rule says a port *moves* to a
shared crate when a second consumer materializes — it does not justify
creating the port before the first consumer exists. Grep for `self.<field>`
before merging a new trait + router + field; if the only matches are the
constructor, the abstraction is unwired. Found in `hkask-mcp-training`
`AdapterPort`/`AdapterRouter` — 1,740 lines of dead surface area including
a fictional OCAP seam (`_token` parameters never verified); both since
removed. The same anti-pattern recurred in `huggingface.rs`
(`ModelRegistry`/`AdapterRegistry`/`DatasetRegistry`, 0/0/1-never-constructed
impls) and was also removed — grep for the named symbol before treating a
`.rules` example as a live artifact.

## Advertised invariants need enforcement points

A doc comment that claims a property — a security invariant (OCAP gating,
capability tokens, unforgeability), an audit/recording surface, or a
migration/replacement ("X → Y") — must point to the line of code that
enforces or realizes it. A trait whose every method takes
`_token: &DelegationToken` and never reads it is theater; an audit surface
whose write path (`store`/`insert`) is never called in production is the
same theater (a `list_*` reader over a table that records nothing — the
write call is the enforcement point); a doc comment naming a replacement
that does not exist in the codebase advertises a migration that never
landed — grep the symbol, and if absent, delete the comment or land the
migration. The `.rules` trap "Process-global hooks set at runtime need a
startup-failure signal" generalizes: every advertised invariant needs an
an enforcement point, or the doc must say "not yet enforced."

This includes documented config behavior, not just code doc comments: an
env-var description or README row that promises runtime behavior ("warnings
fire at 80%") must point to the line that evaluates it, or say "not yet
enforced" — the rule broadens from `///` comments to any user-facing
documentation of behavior.

This also covers validation-gate return values, not just doc comments: a gate
that returns `Ready`/`Success` with an empty findings list when it could not
perform the check advertises a check that didn't happen. The gate must return
a distinct verdict (`Undetermined`, `Skipped`) or emit an `Info` finding
stating the check was skipped and why. Do not return `Ready` with empty
findings when the check was not performed.

## Manifest `ocap:` is declared config, not a security gate

The real OCAP boundary is `McpRuntime::invoke` (`hkask-mcp/src/runtime.rs`):
it matches the in-process `DelegationToken`'s `(resource, resource_id, action)`
against the invoked tool (`is_valid_for`) OR resolves agent domain shorthand
via `verify_capability_domain`, plus a gas gate via `CyberneticsLoop`. A
second real gate is the per-agent `mcp_tools` allowlist enforced at the
`ToolDispatchPort` dispatch boundary (`hkask-types`). Tokens are minted and
consumed in-process — there is **no signature verification and no
unforgeability**; do not describe the system as providing either.

The `ocap:` manifest block and `OcapConfig` struct were removed (2026-08-02).
Do not re-add an `ocap:` manifest block or `OcapConfig` struct without wiring
it into the runtime gate; silent manifest-only config is how the former
`required_capabilities` / `capability_expiry_seconds` theater persisted
undetected (authored in every skill manifest for zero runtime effect). Gas is
enforced via `step.gas_cap` + `CyberneticsLoop`, not `ocap`.

**Token expiry is NOT enforced.** The `OcapConfig` struct, `ocap:` manifest
blocks, `DelegationToken.expires_at`, `new_with_expiry`, `is_valid_for_at`,
and `is_expired` were removed (see `kask/docs/diagrams/flowchart-mcp-runtime-invoke.md`,
59 files). `DelegationToken` (`hkask-tool-port/src/token_types.rs`) carries no
`expires_at` field; `is_valid_for` checks only `(resource, resource_id, action)`
equality. All tokens are no-expiry. If token expiry is re-introduced, add an
`expires_at` field to `DelegationToken`, an `is_valid_for_at` method that
rejects expired tokens, and wire it into `McpRuntime::invoke` in the same
change — then update this rule.

OCAP is enforced at the runtime tool gate, not the registry-list level.

This is the OCAP-specific form of "Advertised invariants need enforcement
points": the manifest block advertises a security membrane; the membrane is
the two runtime gates above, not the YAML.

## `input_mapping` bindings must propagate taint before `context.insert`

`input_mapping` bindings must call `propagate_taint_for_binding(v, k)` before
`context.insert(k, bound)`. Without it, inline-Jinja bindings
(`{{ step_N_result }}`) lose their Source taint label — `context.insert` is a
plain `HashMap` method with no taint awareness, and the FIDES Source→Sink
block is silently bypassed. The compiler does not enforce this; it is a
convention enforced by `RR-0026` (cargo-test kind).

## MCP tool responses are `{"content": <value>}` envelopes

`execute_tool_semantic` (server) and the panel's `invoke_tool` both wrap the
tool value under a `content` key — a response is `{"content": {…}}`, not the
payload itself. Any extractor that reads a tool response must unwrap
`content` first, or every field read returns `None`/`Ok(None)` with no error.
The canonical unwrapper is `hkask_types::tool_response::{parse_tool_response,
unwrap_tool_envelope}` (the single seam, extracted 2026-08-02 so the panel,
corpus tool responses, and MCP server test helpers all share it) — do not
re-implement `value.get("content")` locally. Hit 2026-08-02:
`extract_workspace_id` in `hkask-mcp-swarm` initially read `workspace_id` off
the envelope top level, returned `None` on a live create-swarm response, and
panicked a live probe.

## MCP tool error classification — classify per variant, not blanket `internal`

Inline `McpToolError::internal(format!("…: {e}"))` over every variant of a domain
error mis-classifies `NotFound`/`Unavailable`/auth variants as `Internal`. Use a
`map_<domain>_error(e) -> McpToolError` fn that classifies per variant (see
`map_media_error`, `map_portfolio_error`, `hkask-mcp-training/src/tools/error_mapping.rs`).

## Missing credentials must surface as `permission_denied`, not `unavailable`/`invalid_argument`/silent fallback

A missing credential is an authorization failure, not a transient unavailability or a bad argument. All kask MCP servers must classify a missing credential as `McpToolError::permission_denied` with the env var named in the message, not `unavailable`, `invalid_argument`, `failed_precondition`, or a silent fallback (empty `Vec`, in-memory DB, skipped env injection). Canonical pattern: read credential from `ctx.credentials.get("ENV_VAR")` → if `None`, return a typed domain error → map to `permission_denied` at the tool boundary. Reference: `hkask-mcp-swarm/src/abw_client.rs:require_auth`. Silent fallbacks are broken feedback loops — the operator cannot distinguish "not configured" from "no results" or "provider down." Intentional exceptions: prediction-markets live context (curated static defaults), research `web_search` (free providers always available), kata-kanban (ephemeral in-memory boards by design), curator SMTP (fire-and-forget alert sink). Typed variants (`InferenceError::NotConfigured(String)`, `HostProviderError::NotConfigured(String)`, `WebError::NoProviderConfigured(String)`, `HostProviderError::MissingPrecondition(String)`) replace string-matching where the upstream error type is owned by kask.

## Single keychain namespace for API keys

All API keys (Exa, Tavily, Brave, SerpAPI, Firecrawl, FMP, EODHD, FRED, RunPod, Nebius, HF Token, ABW, etc.) are stored in zed's `CredentialsProvider` keychain namespace under `kask://credentials/<key>` (label `zed-github-account`, attribute `url=kask://credentials/<key>`). There is NO `service=hkask` keychain fallback for API keys — `resolve_credential` reads API keys from env vars only (injected by `build_mcp_server_env` which reads zed's keychain). The legacy `service=hkask` namespace is fully removed and purged at startup (`hkask-keystore/src/keychain.rs`) — do not write anything to it; entries silently vanish. The single internal key `hkask_db_passphrase` lives in the same `kask://credentials/` namespace; there is one passphrase for all SQLCipher DBs and no separate swarm-memory passphrase. Do NOT add `*_enabled` settings toggles — the key's presence in the keychain IS the toggle.

## Canonical `HKASK_DB_PASSPHRASE` resolution helper

All MCP servers that consume `HKASK_DB_PASSPHRASE` must use `hkask_mcp_server::server::resolve_db_passphrase(&ctx.credentials)` — a 2-tier chain (ctx.credentials → `resolve_credential` which does env → `hkask-keystore` keychain `hkask-db-passphrase`) — not inline re-implementations. `ServerContext::resolve_db_credential` delegates to it. Reference pattern: `hkask-mcp-kata-kanban`. Corpus captures the resolved passphrase at server construction into `static CORPUS_DB_PASSPHRASE: OnceLock<Option<String>>`. First-run provisioning: `kask_bridge::identity::provision_db_passphrase` (`identity.rs:145`) provisions the passphrase directly into `kask://credentials/hkask_db_passphrase` (idempotent: env override → existing keychain entry → default `"allostery"`); called at governed MCP server launch (`mcp_servers.rs:677`) — no mirror step.

## `nudge_mcp_servers` restart trigger after keychain writes

`write_credential` / `delete_credential` in `crates/settings_ui/src/pages/kask_page.rs` call `nudge_mcp_servers(cx)` after a `kask://credentials/...` keychain write/delete. The nudge fires `SettingsStore::notify_observers` → `sync_kask_mcp_runtime_servers` → `build_mcp_server_env` (re-reads keychain) → restart. Only fires for `kask://credentials/...` URLs, not for inference-provider `api_url` writes. Without this, a keychain write doesn't trigger a server restart and the server keeps reading the old key until next launch.

## `provision_db_passphrase` launch-ordering dependency

`kask_bridge::identity::provision_db_passphrase` must run at governed MCP server launch (`kask/crates/kask_bridge/src/mcp_servers.rs:677`). It is idempotent (env override → existing keychain entry → default `"allostery"` stored on first run) and writes directly to the unified `kask://credentials/hkask_db_passphrase` namespace — no mirror step. A failed provision logs a `tracing::warn!` naming the env var; the affected server then fails with `permission_denied` at tool time rather than silently falling back to the env/keychain tier.



## Live-mutation probe suites must run serialized

A probe that asserts "no `zed-kask-verify-*` artifacts remain" (the
account-clean check) races any concurrently-running probe that creates the
same artifact class: the cleanup can delete a workspace a sibling test is
mid-using, which ABW reports as a misleading `permission_denied` on that
test's next call — not a test failure. Run the live swarm suite with
`--test-threads=1`, keep probes self-contained (create + delete their own
artifacts), and never have one probe assert global absence while another
mutates the same namespace.

## kask MCP tool inputs that accept arbitrary JSON use `AnyJsonValue`

`schemars` renders `serde_json::Value` as the bare boolean `true` (verified in
`schemars-1.2.2/src/json_schema_impls/serdejson.rs`). A boolean in a schema
position (a `properties.<field>` value, `items`, an `anyOf` member, etc.) is
valid JSON Schema but is rejected by strict-schema-decoding providers — Ollama
fails the whole chat-completion with `400 cannot unmarshal bool into ... of type
api.ToolProperty`; Google Gemini's protobuf `Schema` is the same class of
failure. So kask MCP tool input structs (`#[derive(JsonSchema)]` request types
used as `Parameters<T>` in `#[tool]` functions) that accept arbitrary JSON must
use `hkask_mcp_server::AnyJsonValue` (whose `JsonSchema` emits `{}`), not
`serde_json::Value` — not even `Option<serde_json::Value>` or
`HashMap<String, serde_json::Value>` (the map's `additionalProperties: true` is
a nested boolean of the same risk class). Enforced by
`hkask_mcp_server::find_boolean_schema_positions`, which each server's tool-input
test calls on `schema_for!(TheirRequest)` to assert no schema-valued position is
a bare boolean. Built-in Zed tools and third-party/external MCP servers are NOT
guarded here — they are an accepted, scoped risk (a blanket adapter-level
normalize in `language_model_core::adapt_schema_to_format` was considered and
reverted to avoid an upstream D-seam; revisit only if a non-kask tool
reintroduces the break).

## Convention helpers with only test callers are dead code

A `pub fn` that pins a convention (e.g., an entity-ref prefix format) but has
no production caller is the function-level analog of "trait-with-one-impl."
The function exists, tests pass, the convention is pinned — but the
enforcement point (the agent constructing the prefix at runtime, or the tool
returning the prefix in its output) is not the function. The convention drifts
just as effectively if pinned by a doc + a single test on the format string.
Wire the convention into the tool's output (a field on the result struct, e.g.
`TranscriptRecord.entity_ref_prefix`), not a standalone function. If the
convention is only consumed by the agent at runtime (not by Rust code),
annotate with `#[allow(dead_code)]` and document that it's an agent-runtime
convention helper — but prefer wiring it into the tool's output so the
convention is load-bearing, not advisory. This generalizes the
"Trait-with-one-impl is speculative generality" rule from traits to
functions: a public function whose only callers are tests is dead surface
regardless of whether it pins a convention.

## Folded-service dead surface

When a crate is assembled by folding in a former standalone service crate
(the `hkask-services-{compose,corpus,inference,runtime}` → `hkask-mcp-corpus`
merge is the canonical example), the fold brings in modules that were wired in
the old service but have no consumer in the new host. The leftover modules
compile, pass their own unit tests, and often carry doc comments advertising
invariants (P9 spans, cost models, citation storage) with no enforcement point
in the new host — the "Advertised invariants need enforcement points" trap at
module scale.

This is the *module* form of "Trait-with-one-impl is speculative generality"
(trait form) and "Convention helpers with only test callers are dead code"
(function form): an entire submodule re-exported from a `mod.rs` with zero
production callers outside the submodule tree.

After folding a service crate into a host, grep the host for every `pub use`
the fold introduced. For each re-exported symbol, grep the host's `src/`
(outside the folded subtree) for a production caller. Zero hits = delete the
module in the same PR or the next cleanup PR. Do not keep folded modules on
the speculation that "a future tool will use them" — the port-promotion rule
applies: re-add when the first consumer materializes.

## Ontology tag field-drop trap

A new MCP server that emits an `"ontology"` tag in its tool output must add
an S4 sensor test in the corresponding widget crate asserting the widget's
block body struct parses the field. Without the test, `serde`'s `Deserialize`
silently drops unknown fields — the server emits, the widget ignores, no
error surfaces, the "I" pattern dispatch never fires. The `dead_surface_pins`
test catches a module with zero call sites, but not a widget that drops a
field the server emits. The failure mode is silent: the widget renders
without affordances and the operator cannot distinguish "block has no ontology
tag" from "widget dropped the tag."

# Stale diagnostics after bulk edits

After a sequence of `edit_file` calls on the same file, `diagnostics` may
report errors and warnings from the pre-edit state — rust-analyzer's
incremental index lags behind rapid edits. The crate's lib root (the file
named in `[lib] path` in `Cargo.toml`) is the authoritative compile check: if
it compiles clean, the crate compiles, regardless of what individual-file
diagnostics report. Do not retry the same `diagnostics` call expecting it to
refresh, and do not "fix" errors that appear only in a stale individual-file
diagnostic but not in the lib-root diagnostic. The failure mode is an agent
loop (retrying stale diagnostics) or a phantom-bug chase (editing code that
was already correct to silence an error that doesn't exist in the current
file state).
