# UI / Interaction Audit — Kask GPUI Widgets

> Method: `ui-layout-discipline` (Sense→Orient→Decide→Act→Review) across the
> kask-owned GPUI crates: `hkask-{viz-core,media-widget,graph-widget,
> kanban-widget,portfolio-widget,scenarios-widget,conversation-injector}`,
> `kask_extensions_ui`, `marketplace_ui_common`, and the D9 settings page.
> Fitts's Law, Hick's Law, Rosenholtz feature congestion, Tufte data-ink,
> Nielsen progressive disclosure. Upstream Zed editor/panel code is out of scope.

## Headline findings

- **UI-13 (blocking)**: all 5 viz widgets + media transport use raw
  `div().id(...).cursor_pointer().on_click(...)` with a `Label` child for action
  affordances — 18 sites — instead of Zed's `Button::new(id, label)` /
  `IconButton::new(id, icon)`. `kask_extensions_ui` already uses `Button`/
  `IconButton` correctly (`kask_extensions_ui.rs:816-909`), proving the
  primitives are available and the divergence is unintentional. This is the
  dominant interaction-language inconsistency between kask widgets and Zed.
- **UI-01 (high)**: `hkask-graph-widget` header is a single-line `h_flex()` (no
  `flex_wrap`/`min_w_0`/truncate) carrying a subject, an optional joint-prob
  string, a ~90-char backward-inference warning, and the "I disagree" affordance.
  The widget root applies `overflow_hidden()` (view.rs:839), so on a narrow
  markdown block the header overflows rightward and is clipped — the "I
  disagree" affordance is pushed off-screen and becomes unclickable.
- **UI-15**: zero kask widget uses `PopoverMenu`/`ContextMenu`. Action rows
  grow linearly with data (portfolio: one "Explain" per row; scenarios: 8
  pipeline stages; kanban: Confirm/Cancel/Evaluate) with no progressive-
  disclosure escape valve. This is the root cause of the action-congestion cluster.

## Findings

| ID | axis | sev | widget | file:line | force |
|----|------|-----|--------|-----------|-------|
| UI-01 | measured-layout | high | hkask-graph-widget | `view.rs:684-717` | blocking |
| UI-02 | measured-layout | med | hkask-graph-widget | `view.rs:593-680` | directing |
| UI-03 | measured-layout | med | hkask-kanban-widget | `view.rs:201-264` | directing |
| UI-04 | measured-layout | med | hkask-portfolio-widget | `view.rs:787-817` | directing |
| UI-05 | measured-layout | med | hkask-portfolio-widget | `view.rs:461-515` | directing |
| UI-06 | measured-layout | med | hkask-portfolio-widget | `view.rs:367-400` | directing |
| UI-07 | measured-layout | med | hkask-scenarios-widget | `view.rs:378-396` | directing |
| UI-08 | measured-layout | low | hkask-scenarios-widget | `view.rs:296-313` | directing |
| UI-09 | measured-layout | low | hkask-media-widget | `transport.rs:148-188` | directing |
| UI-10 | action-congestion | med | hkask-media-widget | `media_widget.rs:775-825` | directing |
| UI-11 | action-congestion | med | kask_extensions_ui | `kask_extensions_ui.rs:822-910` | directing |
| UI-12 | action-congestion | low | kask_extensions_ui | `kask_extensions_ui.rs:1305-1403` | enabling |
| UI-13 | interaction-language | high | all 5 viz + media | `view.rs:705-717` (graph) + 17 more | blocking |
| UI-14 | interaction-language | med | portfolio/scenarios/media | `view.rs:805-816` + more | directing |
| UI-15 | interaction-language | med | all viz widgets | (no PopoverMenu anywhere) | enabling |
| UI-16 | interaction-language | low | graph/portfolio/scenarios/media | (no Tooltip) | enabling |
| UI-17 | measured-layout | low | hkask-kanban-widget | `view.rs:405,469-475` | enabling |
| UI-18 | theme-consistency | low | kanban/graph/media | `view.rs:352` + more | enabling |
| UI-19 | toggle-vs-togglefocus | low | kask_extensions_ui | `kask_extensions_ui.rs:52-91` | enabling (POSITIVE CONTROL) |
| UI-20 | theme-consistency | low | settings_ui (D9) | `kask_page.rs:394-703` | enabling (non-finding, D9-scoped) |

## Pattern-gap summary — top 3 Zed primitives kask widgets should adopt

1. **`Button::new(id, label).style(...)` / `IconButton::new(id, icon)`** — the
   raw-div affordance pattern (UI-13, 18 sites) re-implements a subset of
   `Button` (only `cursor_pointer` + `Label::color`) and misses consistent
   hit-area padding, focus rings, keyboard activation, and `disabled` styling.
   The Kanban move chip (`view.rs:565-591`) re-implements `disabled` via a
   conditional `cursor_pointer`/`on_click` branch that `Button::disabled`
   handles natively.
2. **`PopoverMenu` (kebab collapse for secondary actions)** — zero kask widget
   uses it (UI-15). Action rows grow linearly with data with no progressive
   disclosure. Root cause of the UI-10/11/12 congestion cluster.
3. **`Tooltip::text(...)` on non-self-evident affordances** — only Kanban
   uses `Tooltip` (`view.rs:420,432,571`). Graph "Observe (LR 3:1)",
   portfolio "Explain"/"Apply", scenarios "Next: <stage>", media
   "Explain"/"I disagree" have no tooltip despite non-obvious semantics.

## Positive controls (traps verified handled)

- **UI-19**: `KaskExtensionsPage` is the only kask center-pane `Item`. It
  correctly registers `Toggle` (deploys new item) for the View-menu action and
  separately `ToggleFocus` (focus-only), and calls
  `extensions_page.focus_handle(cx).focus(window, cx)` after
  `add_item_to_active_pane(..., true, ...)` (L79) — pinning both the
  `.rules` "Toggle vs ToggleFocus" and "deploy-and-focus" traps.
  `KaskExtensionsPage::focus_handle` (L1434-1436) delegates to a child `Editor`
  constructed in `cx.new`, which is exactly the trap scenario. Handled.
- **UI-20**: `kask_page.rs` only emits `Vec<SettingsPageItem>`; all rendering
  is delegated to the settings framework. No custom `Render`/`RenderOnce` impl,
  no `div().flex()` chains. D9-seam-scoped; no GPUI layout to audit. Non-finding.

## Remediation pattern (applies to the UI-13 cluster)

Replace raw-div affordances with primitives, preserving `on_click` handlers:

```rust
// before
div().id("graph-disagree").cursor_pointer().on_click(...).child(
    Label::new("I disagree").size(LabelSize::XSmall).color(Color::Accent))
// after
Button::new("graph-disagree", "I disagree")
    .style(ButtonStyle::Subtle)
    .on_click(...)
```

Use `IconButton::new(id, icon)` for icon-only actions (media Play/Pause/Stop),
and `Button::disabled(...)` for the Kanban Installing/Removing states instead
of the conditional `cursor_pointer`/`on_click` branch.