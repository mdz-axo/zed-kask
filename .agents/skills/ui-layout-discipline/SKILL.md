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

## The discipline

Before adding any element: **measure** the container, **count** the
elements (≤5 primary per Hick's Law), **protect text** (≥20em residual),
**match patterns** (grep sibling cards), **declare overflow**
(PopoverMenu with `IconName::Ellipsis`, `truncate()`, `flex_shrink_0`).

## GPUI patterns the skill checks

- `min_w_0()` on flexible text columns.
- `flex_shrink_0()` on fixed-width elements.
- `.truncate()` on labels.
- `PopoverMenu::new(id).trigger_with_tooltip(IconButton::new(id, IconName::Ellipsis), Tooltip::text(...))` for overflow.
- `ContextMenu::build(window, cx, |menu, ...| menu.entry(label, None, handler))` for menu items.
- `gap_1()`/`gap_2()` on the 4px/8px grid.

## When to use

- Before adding elements to a card/panel renderer.
- When a card has >2 action buttons or a text column next to actions.
- When modifying a shared card container with multiple consumers.
- When a layout looks jumbled or cramped.

## When NOT to use

- Pure logic changes with no layout impact.
- Single-element cards. Test-only changes.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `sense.j2` | KnowAct | Measure container width, child min-widths, and text-column residual (Fitts's Law, flexbox overflow invariant). |
| `orient.j2` | KnowAct | Count actions vs budget (≤5 primary, Hick's Law), grep sibling card patterns, compute congestion score and action-to-content ratio. |
| `decide.j2` | KnowAct | Gate on hard layout constraints (Fagan checklist): no overflow, primary action visible, text column ≥ min width, on-grid spacing, action count ≤ budget. |
| `act.j2` | KnowAct | Apply progressive disclosure remedies: PopoverMenu with Ellipsis trigger, truncate(), flex_shrink_0, min_w_0, hide-secondary. |
| `review.j2` | KnowAct | Adversarial probes: 40-char label, localized German string, 320px container, 7 actions. |

## Constraints

- All templates are `KnowAct` with `Public` visibility; they emit `reg.ui_layout.*` spans.
- rJoule cap: 1 per invocation. Maximum 5 iterations (Sense → Orient → Decide → Act → Review loop; convergence = all gates pass under adversarial probes).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
