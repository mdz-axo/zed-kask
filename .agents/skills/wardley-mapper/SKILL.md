---
name: wardley-mapper
description: "Generic Wardley mapping methodology. Given components and their relationships, classifies each on the evolution axis (Genesis to Custom to Product to Commodity), maps them on the value chain, identifies strategic movement, and produces a Wardley map."
audience: user
---

# Wardley Mapper

Generic Wardley mapping methodology. Given a set of components and their relationships, inventories them, classifies each on the evolution axis (Genesis → Custom → Product → Commodity), maps them on the value chain (visibility vs evolution), identifies strategic movement (what to commoditize, invest in, divest, or build), and produces a Wardley map with recommendations. Applicable to any system — software platforms, business capabilities, technology stacks.


## Reference model and labels

Wardley, *Wardley Maps* (2016) — `onto_anchor` → derived `wardley_map` (operator ruling 2026-09-25): anchor on a user need, lay out the value chain, position each component by evolution.

- **Initial condition:** the target system description and any supplied component inventory or previous map.
- **Target condition:** the `WM-1` and `WM-2` `lisp_eval` gates pass and the map plus recommendations reach the chat through `present-map`.
- **D/P labelling:** inventory, evolution classification, coordinates, movement and recommendations are P — judgment, critiqued by the operator and by the requirement that every recommendation trace to a named component and movement. `WM-1` (stage-band coordinate and dependency closure check), `WM-2` (recommendation component traceability check), and `present-map` (pure rendering) are D. An evolution class is a judgment against Wardley's characteristics, not a measurement; say so when data is sparse.

## When to Use

- When you need to inventory and enumerate all components of a target system (software, business, or technology) to prevent strategic blind spots.
- When classifying system components on the Wardley evolution axis (Genesis, Custom, Product, Commodity) based on maturity, adoption, and standardization.
- When mapping classified components onto a value chain (visibility vs. evolution) and generating a visual Mermaid quadrant chart.
- When identifying strategic movement, including what to commoditize, what to keep as a differentiator, what is missing, or how the system has drifted from a previous state.
- When synthesizing actionable, prioritized strategic recommendations (invest, divest, commoditize, ecosystem) from a Wardley map.

## When NOT to Use

- Component inventory with no strategic question — a plain architecture list does not need an evolution map.
- Executing the strategic moves it recommends — the map feeds planning; it does not run the invest/divest/commoditize decisions.
- Non-visual strategy prose — the deliverable is the value-chain quadrant chart; a text-only strategy doc is a different artifact.

## Instructions

The skill runs as a 4-step process (`inventory-components` → `classify-evolution` + `WM-1` → `synthesize-recommendations` + `WM-2` → `present-map`).

### inventory-components

1. Render `wardley-mapper/inventory-components` with `target_system` and `component_inventory`. Enumerate every component in the target system.
2. Capture the name, type (infrastructure, platform_service, user_facing, conceptual, protocol), dependencies, and description for each component.
3. Consider infrastructure, platform services, user-facing capabilities, and conceptual components.
4. Be exhaustive to avoid blind spots, and note any gaps or assumptions if the system description is sparse.

### classify-evolution

1. Render `wardley-mapper/classify-evolution` with `components` (from `inventory-components`). Classify each component on the Wardley evolution axis (Genesis, Custom, Product, Commodity) and place it on the value chain map with `(x, y, links)` and a Mermaid `quadrantChart` (`map_diagram`).
2. Assess components based on maturity, adoption, standardization, and differentiation criteria:
   - Industry standards with open-source equivalents → `Commodity`, `x` in `[0.75, 1.0]`
   - Stable, rentable, off-the-shelf components → `Product`, `x` in `[0.50, 0.75)`
   - Bespoke, emerging components → `Custom`, `x` in `[0.25, 0.50)`
   - Experimental or uncharted components → `Genesis`, `x` in `[0.0, 0.25)`
3. Assign `y` for value-chain visibility (`0.0` = infrastructure, `1.0` = user-facing) and `links` to other inventoried components by name.
4. Generate `map_diagram` (`quadrantChart` visualizing the value chain).
5. **Gate `WM-1` (D — `lisp_eval`):** Verify that every component has an evolution class whose `x` coordinate falls inside its stage band (`[0.0,0.25)`, `[0.25,0.50)`, `[0.50,0.75)`, `[0.75,1.0]`), `y` in `[0.0, 1.0]`, all `links` resolve to inventoried component names, and `map_diagram` starts with/contains `"quadrantChart"`:
   - form: `(begin (define band-ok (lambda (c) (let ((ev (assoc "evolution" c)) (x (assoc "x" c)) (y (assoc "y" c))) (and (>= y 0.0) (<= y 1.0) (cond ((string= ev "Genesis") (and (>= x 0.0) (< x 0.25))) ((string= ev "Custom") (and (>= x 0.25) (< x 0.50))) ((string= ev "Product") (and (>= x 0.50) (< x 0.75))) ((string= ev "Commodity") (and (>= x 0.75) (<= x 1.0))) (t false)))))) (define links-ok (lambda (ls names) (if (is_null ls) true (and (member (car ls) names) (links-ok (cdr ls) names))))) (define component-ok (lambda (c names diagram) (and (member (assoc "name" c) names) (band-ok c) (links-ok (assoc "links" c) names) (string-contains (assoc "name" c) diagram)))) (define all-components-ok (lambda (cs names diagram) (if (is_null cs) true (and (component-ok (car cs) names diagram) (all-components-ok (cdr cs) names diagram))))) (define name-present (lambda (name cs) (if (is_null cs) false (or (string= name (assoc "name" (car cs))) (name-present name (cdr cs)))))) (define all-names-present (lambda (names cs) (if (is_null names) true (and (name-present (car names) cs) (all-names-present (cdr names) cs))))) (and (> (length components) 0) (= (length components) (length component_names)) (string-contains "quadrantChart" map_diagram) (all-components-ok components component_names map_diagram) (all-names-present component_names components)))`
   - env: `{ "components": <classified components array>, "component_names": <list of inventoried component name strings>, "map_diagram": <the generated Mermaid quadrantChart string> }`
   Bound: one re-entry — re-run `classify-evolution` for any out-of-band or unresolved components; a second pass that still leaves gaps notes them as assumptions in the map.

### synthesize-recommendations

1. Render `wardley-mapper/synthesize-recommendations` with `current_map` (the classified components from `classify-evolution`) and optional `previous_map` (when comparing drift against a prior map).
2. Analyze strategic movement across five dimensions: components to commoditize (`Custom` components providing no differentiation), components to keep at `Product` (strategic differentiators), over-commoditized components, missing value-chain components, and drift from `previous_map` (if provided). Emit `movements` and `movement_summary`.
3. Synthesize actionable strategic `recommendations` (commoditize, invest, divest, ecosystem, alignment), prioritized by impact, with `rationale`.
4. **Gate `WM-2` (D — `lisp_eval`):** Verify that every recommendation traces to a named component in `component_names` (or a `"missing"` movement component) and carries a numeric `confidence` in `[0.0, 1.0]`:
   - form: `(begin (define rec-ok (lambda (r names) (let ((c (assoc "component" r)) (conf (assoc "confidence" r))) (and (member c names) (>= conf 0.0) (<= conf 1.0))))) (define all-recs-ok (lambda (rs names) (if (is_null rs) true (and (rec-ok (car rs) names) (all-recs-ok (cdr rs) names))))) (all-recs-ok recommendations valid_names))`
   - env: `{ "recommendations": <recommendations array>, "valid_names": <component_names plus any missing-movement component names> }`

### present-map

1. Rendering step (`wardley-mapper/present-map` via `render_template` with `map_diagram`, `recommendations`, and optional `rationale`): surfaces the quadrant chart (from `classify-evolution`'s `map_diagram`) and strategic recommendations (from `synthesize-recommendations`) as a single markdown string containing a fenced ```mermaid block.
2. This is the process's final user-facing output — without this step, the diagram stays buried in an intermediate step result and never reaches the chat stream.
3. Deterministic (no LLM call) — pure Jinja2 rendering via `render_template`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `inventory-components.j2` | Inventory all components in a target system. Enumerates every component with name, type, dependencies, and description. Exhaustive — missing components create map blind spots. |
| `classify-evolution.j2` | Classify each component on the Wardley evolution axis (Genesis `[0.0,0.25)`, Custom `[0.25,0.50)`, Product `[0.50,0.75)`, Commodity `[0.75,1.0]`), place it on the value chain `(x, y, links)`, and generate a Mermaid `quadrantChart` (`map_diagram`). |
| `synthesize-recommendations.j2` | Identify strategic movement (`movements`, `movement_summary`, including drift from optional `previous_map`) and synthesize actionable strategic recommendations (commoditize, invest, divest, ecosystem, alignment) traceable to components. |
| `present-map.j2` | Rendering template — surfaces the quadrant chart (from `classify-evolution`) and strategic recommendations (from `synthesize-recommendations`) as a single markdown string containing a fenced ```mermaid block. Deterministic (no LLM call). |

To render a template, call the `render_template` tool with the template ref (e.g., `wardley-mapper/inventory-components`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `inventory-components.j2`: `target_system`, `component_inventory`
- `classify-evolution.j2`: `components`
- `synthesize-recommendations.j2`: `current_map`, `previous_map` (optional)
- `present-map.j2` (pure render; its `[inference]` header declares inputs only, no model call): `map_diagram` (string — the mermaid quadrant chart text), `recommendations` (array of objects, each `{category, component, recommendation, priority, confidence, rationale}` — all plain strings; passing an array of strings renders N/A rows), `rationale` (optional string — the assessment paragraph)


## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the convergence gate:

- Zero unclassified components and zero unresolved dependencies → `true`.
- One unresolved dependency → `false`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- `present-map.j2`: rendering template (no inference) — surfaces the diagram as the process's final output.
- The Convergence gate's re-entry is bounded to one pass; a second pass with gaps notes them as assumptions in the map instead of iterating.
- An evolution class is a judgment against Wardley's characteristics, not a measurement — say so when data is sparse.
- Every recommendation traces to a named component and movement; untraceable recommendations are flagged, not emitted.
