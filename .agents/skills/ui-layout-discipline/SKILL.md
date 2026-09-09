---
name: ui-layout-discipline
description: "Enforces measured layout discipline for GPUI card/panel renderers. Prevents unmeasured action congestion — adding elements without checking width, counting elements, or verifying text columns. Measures, gates on constraints, applies remedies."
---

# UI Layout Discipline

Enforces measured layout decisions in GPUI renderers. Layout failures
share a single root cause: **adding elements without measuring**.

## The failure pattern

1. No container measurement (dock ~300-400px, center ~600px+).
2. No element count (how many buttons fit?).
3. No text-column check (does `flex_1` retain readable width?).
4. No codebase pattern match (how do sibling cards handle this?).
5. No overflow policy (wrap, truncate, PopoverMenu, hide-secondary).

## Instructions

The loop lives in this body; the five templates are its step-leaves.

1. **Measure** (`sense.j2`) — container width, each child's minimum
   width, text-column residual width.
2. **Count** (`orient.j2`) — interactive elements against the ≤5
   primary budget (Hick's Law); sibling card conventions; congestion
   score.
3. **Gate** (`decide.j2`) — the five yes/no gates: no overflow,
   primary action visible, text column ≥ min width, on-grid spacing,
   action count ≤ budget. Call `lisp_eval` with:
   - form: `(and (eq (length failed_gates) 0) (eq (length probe_failures) 0))`
   - env: `{ "failed_gates": <failing gate names from decide>,
            "probe_failures": <broken adversarial probes from step 5> }`
4. **Remedy** (`act.j2`) — for each failing gate, apply the canonical
   GPUI remedy (PopoverMenu with `IconName::Ellipsis`, `truncate()`,
   `flex_shrink_0`, `min_w_0`).
5. **Probe** (`review.j2`) — adversarial probes: a 40-character button
   label, a localized German string (~30% longer), a 320px container,
   7 actions. Any broken probe rejects the layout and re-enters step 4.
   Bound: max 2 remedy rounds; a third failing gate set rejects the
   layout change — hide the secondary actions permanently or defer,
   and say so.

## GPUI patterns the skill checks

- `min_w_0()` on flexible text columns.
- `flex_shrink_0()` on fixed-width elements.
- `.truncate()` on labels.
- `PopoverMenu::new(id).trigger_with_tooltip(IconButton::new(id, IconName::Ellipsis), Tooltip::text(...))` for overflow.
- `ContextMenu::build(window, cx, |menu, ...| menu.entry(label, None, handler))` for menu items.
- `gap_1()`/`gap_2()` on the 4px/8px grid.

## When to Use

- Before adding elements to a card/panel renderer.
- When a card has >2 action buttons or a text column next to actions.
- When modifying a shared card container with multiple consumers.
- When a layout looks jumbled or cramped.

## When NOT to Use

- Pure logic changes with no layout impact.
- Single-element cards. Test-only changes.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `sense.j2` | Measure the rendering container's available width and each child element's minimum width. Compute whether sum(min_widths) + gaps exceeds the container. If a text column (flex_1) is present, compute its residual width after fixed-width children claim their space. Grounded in Fitts's Law (minimum target width) and the CSS flexbox overflow invariant. |
| `orient.j2` | Count the interactive elements in the container. Compare to the action budget (≤5 primary, per Hick's Law). Grep the crate for sibling card/panel components and compare action-count/spacing conventions (Nielsen consistency). Compute the feature congestion score (Rosenholtz) and the action-to-content ratio (Tufte data-ink). |
| `decide.j2` | Apply the Fagan-style inspection checklist as hard gates: (1) no overflow, (2) primary action visible, (3) text column ≥ min width, (4) on-grid spacing, (5) action count ≤ budget. Each gate is a yes/no verdict, not a vibe check. Produce a layout_health vector and a pass/fail decision. |
| `act.j2` | For each failing gate, prescribe the canonical GPUI remedy: secondary actions behind a PopoverMenu with IconName::Ellipsis trigger (Nielsen progressive disclosure), truncate() on text labels, flex_shrink_0 on fixed elements, min_w_0 on flexible text columns, or explicit hide-secondary. Reference the agent_panel.rs render_panel_options_menu pattern. |
| `review.j2` | Run adversarial probes against the proposed layout: a 40-character button label, a localized German string (~30% longer), a 320px container, 7 actions. If any probe breaks a gate, the layout is rejected and the remedy phase re-enters. Grounded in Klein's premortem and the squint test for visual hierarchy. |

To render a template, call the `render_template` tool with the template ref (e.g., `ui-layout-discipline/sense`) and a context object with the required variables.

## Constraints

- All templates are prompt templates with `Public` visibility; they emit `reg.ui_layout.*` spans.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
