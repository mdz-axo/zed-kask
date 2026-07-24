---
name: logo-builder
visibility: public
description: "Pragmatic and principled logo design using LLM-assisted generation. Synthesizes Martin (Made By James — Minimum Viable Brand), Bokhua (Principles of Logo Design — five formal gates), and Peters (Logos That Last — iterative case study method). Three-phase pipeline: discovery (brand-to-design mapping), formal generation (Bokhua gates), and iterative refinement (weighted critique loop). Uses the media server's generate_image and describe_image tools. Logos stored in the gallery as regular images — no separate storage.
"
---

# Logo Builder

Pragmatic and principled logo design using LLM-assisted generation. Synthesizes Martin (Made By James — Minimum Viable Brand), Bokhua (Principles of Logo Design — five formal gates), and Peters (Logos That Last — iterative case study method). Three-phase pipeline: discovery (brand-to-design mapping), formal generation (Bokhua gates), and iterative refinement (weighted critique loop). Uses the media server's generate_image and describe_image tools. Logos stored in the gallery as regular images — no separate storage.


## When to Use

- Map qualitative brand identity inputs (industry, audience, values) to formal logo design parameters before generation.
- Select an appropriate logo generation strategy (single-shot, iterative-refine, or moodboard-first) based on brand complexity and user preference.
- Generate a professional logo from formal design parameters using Bokhua's five design gates (simplicity, monochrome viability, grid discipline, negative space, scalability).
- Generate multiple logo candidates, critique them with a vision LLM across weighted dimensions, and iteratively refine the best candidate.
- Produce a complete logo deliverables package, including a transparent PNG, monochrome variant, icon-only mark, and real-world context mockup.

## Instructions

### logo-discovery

1. Render the `logo-discovery-map` template with the provided brand inputs.
2. Call the inference router with the rendered prompt to classify and map brand attributes.
3. Parse the JSON response into formal design parameters (style, logo_type, dominant_shape, typography_class, palette_hex, density, rationale).
4. Select a generation strategy (single-shot, iterative-refine, or moodboard-first) based on brand complexity and user preference.

### logo-discovery-map

1. Act as a brand strategist to recommend formal logo design parameters from a business identity brief.
2. Return a JSON object containing `style`, `logo_type`, `dominant_shape`, `typography_class`, `palette_direction`, `palette_hex`, `density`, and `rationale`.
3. Base recommendations on brand strategy principles, considering audience expectations and industry visual conventions.
4. Recommend differentiation over imitation.

### logo-formal-prompt

1. Design a professional logo using the provided design parameters (style, logo type, dominant shape, density, typography, palette).
2. Ensure the mark is simple, describable in one sentence, and built on one strong idea (G1 — Simplicity).
3. Verify the logo works in pure black on pure white, treating color as additive rather than structural (G2 — Monochrome Viability).
4. Align key elements to geometric relationships like the golden ratio or integer ratios without accidental placement (G3 — Grid Discipline).
5. Design negative space to be active or neutral, never accidental (G4 — Negative Space).
6. Guarantee legibility at 16px and balance at billboard size, avoiding hairline strokes (G5 — Scalability).
7. Produce a clean, scalable vector-style design with high contrast and no text artifacts.
8. Avoid photographic elements, UI chrome, trademarked logos, and multiple variations in a single image.
9. Return a single logo image.

### logo-iterative-refine

1. Generate the specified number of initial logo candidates using the `logo-formal-prompt` template.
2. Critique each candidate using a vision LLM, scoring readability, scalability, distinctiveness, professionalism, and text accuracy from 1-10.
3. Summarize the strongest weakness of each candidate in one paragraph.
4. Select the best candidate based on the highest aggregate score.
5. Regenerate the selected candidate by incorporating critique feedback, fixing weaknesses while preserving strengths.
6. Repeat the critique and refine cycles for the specified number of rounds.
7. Remove the background from the final selected logo to produce a transparent PNG.

### logo-presentation

1. Remove the background from the final logo to produce a transparent PNG.
2. Generate a monochrome variant (pure black on white) for single-color applications, maintaining the exact same design without color or gradients.
3. Generate a 1:1 square icon-only version by removing all text and keeping only the symbol, ensuring it works at 64x64 pixels.
4. Generate a phot

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `../media/logo-discovery.yaml` | FlowDef | Discovery-phase pipeline: map qualitative brand identity inputs to formal logo design parameters, then select the appropriate generation strategy. Agent-coordinated; uses inference router for classification.  |
| `../media/logo-discovery-map.j2` | KnowAct | Map qualitative brand identity inputs (industry, audience, values) to formal logo design parameters (style, type, shape, palette, density) using strategic brand reasoning. From Martin's Minimum Viable Brand.  |
| `../media/logo-formal-prompt.j2` | KnowAct | Core logo generation prompt encoding Bokhua's five design gates: simplicity, monochrome viability, grid discipline, negative space, and scalability. Every logo generation flow delegates to this template.  |
| `../media/logo-iterative-refine.yaml` | FlowDef | Peters-inspired iterative logo pipeline: generate N candidates, score each against 7 weighted critique dimensions (5 Bokhua gates + brand-fit + distinctiveness), select best, refine through critique cycles. Final output is background-removed for transparent PNG deliverables.  |
| `../media/logo-presentation.yaml` | FlowDef | Generate a complete logo deliverables package from a refined logo: transparent PNG, monochrome variant, icon-only mark, and context mockup showing the logo in real-world use.  |

## Constraints

- `../media/logo-discovery.yaml`: Public.
- `../media/logo-discovery-map.j2`: Public.
- `../media/logo-formal-prompt.j2`: Public.
- `../media/logo-iterative-refine.yaml`: Public.
- `../media/logo-presentation.yaml`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
