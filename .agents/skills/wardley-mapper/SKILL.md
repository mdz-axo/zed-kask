---
name: wardley-mapper
visibility: public
description: "Generic Wardley mapping methodology. Given a set of components and their relationships, inventories them, classifies each on the evolution axis (Genesis → Custom → Product → Commodity), maps them on the value chain (visibility vs evolution), identifies strategic movement (what to commoditize, invest in, divest, or build), and produces a Wardley map with recommendations. Applicable to any system — software platforms, business capabilities, technology stacks.
"
---

# Wardley Mapper

Generic Wardley mapping methodology. Given a set of components and their relationships, inventories them, classifies each on the evolution axis (Genesis → Custom → Product → Commodity), maps them on the value chain (visibility vs evolution), identifies strategic movement (what to commoditize, invest in, divest, or build), and produces a Wardley map with recommendations. Applicable to any system — software platforms, business capabilities, technology stacks.


## When to Use

- When you need to inventory and enumerate all components of a target system (software, business, or technology) to prevent strategic blind spots.
- When classifying system components on the Wardley evolution axis (Genesis, Custom, Product, Commodity) based on maturity, adoption, and standardization.
- When mapping classified components onto a value chain (visibility vs. evolution) and generating a visual Mermaid quadrant chart.
- When identifying strategic movement, including what to commoditize, what to keep as a differentiator, what is missing, or how the system has drifted from a previous state.
- When synthesizing actionable, prioritized strategic recommendations (invest, divest, commoditize, ecosystem) from a Wardley map.

## Instructions

### inventory-components

1. Enumerate every component in the target system.
2. Capture the name, type (infrastructure, platform_service, user_facing, conceptual, protocol), dependencies, and description for each component.
3. Consider infrastructure, platform services, user-facing capabilities, and conceptual components.
4. Be exhaustive to avoid blind spots, and note any gaps or assumptions if the system description is sparse.

### classify-evolution

1. Classify each component on the Wardley evolution axis (Genesis, Custom, Product, Commodity).
2. Assess components based on maturity, adoption, standardization, and differentiation criteria.
3. Assign industry standards with open-source equivalents to Commodity.
4. Assign strategic differentiators to Product or Custom (if still emerging).
5. Assign experimental or actively designed components to Genesis.
6. Assign stable but non-standardized components to Custom.

### map-value-chain

1. Place each classified component on the value chain map.
2. Assign an x-coordinate for evolution position (0.0 = Genesis, 1.0 = Commodity).
3. Assign a y-coordinate for value chain position (0.0 = infrastructure, 1.0 = user-facing).
4. Define dependency links to other components by name.
5. Generate a Mermaid quadrant chart visualizing the map.

### surface-map

1. Render the quadrant chart (from map-value-chain) and strategic recommendations (from synthesize-recommendations) as a single markdown string containing a fenced ```mermaid block.
2. This is the cascade's final user-facing output — without this step, the diagram stays buried in an intermediate step result and never reaches the chat stream.
3. Deterministic (no LLM call) — pure Jinja2 rendering via the `render` action.

### identify-movement

1. Analyze the current map for strategic movement across five dimensions.
2. Identify components to commoditize (Custom components providing no differentiation).
3. Identify components to keep at Product (strategic differentiators).
4. Identify over-commoditized components that should move left toward Custom to recover differentiation.
5. Identify missing components or gaps in the value chain.
6. Identify drift by comparing the current map to a previous map, if provided.

### synthesize-recommendations

1. Synthesize actionable strategic recommendations from the movement analysis and current map.
2. Identify commoditization targets, investment priorities, divestment candidates, ecosystem plays, and alignment checks.
3. Trace every recommendation to a specific component and movement in the analysis.
4. Prioritize recommendations by impact (highest first).
5. Be specific in the recommended actions.
6. Flag uncertainty and lower confidence for recommendations based on sparse data.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `inventory-components.j2` | KnowAct | Inventory all components in a target system. Enumerates every component with name, type, dependencies, and description. Exhaustive — missing components create map blind spots.  |
| `classify-evolution.j2` | KnowAct | Classify each component on the Wardley evolution axis (Genesis, Custom, Product, Commodity) using maturity, adoption, standardization, and differentiation criteria.  |
| `map-value-chain.j2` | KnowAct | Place each classified component on the value chain map (Y: visibility, X: evolution) with coordinates and dependency links. Generates a Mermaid quadrant chart.  |
| `identify-movement.j2` | KnowAct | Identify strategic movement: what to commoditize, what to keep at Product, what's over-commoditized, what's missing, and drift from a previous map.  |
| `synthesize-recommendations.j2` | KnowAct | Synthesize actionable strategic recommendations (commoditize, invest, divest, ecosystem, alignment) from the movement analysis and map. Prioritized by impact, specific, traceable to components.  |
| `present-map.j2` | RenderAct | Surface the Wardley Map quadrant chart and recommendations as a markdown string with a fenced ```mermaid block. The cascade's final user-facing output — without it, the diagram stays buried in an intermediate step result. Deterministic (no LLM call).  |

## Constraints

- `inventory-components.j2`: Public.
- `classify-evolution.j2`: Public.
- `map-value-chain.j2`: Public.
- `identify-movement.j2`: Public.
- `synthesize-recommendations.j2`: Public.
- `present-map.j2`: Public. RenderAct (no inference) — surfaces the diagram as the cascade's final output.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
