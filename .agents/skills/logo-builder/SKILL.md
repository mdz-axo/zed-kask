---
name: logo-builder
description: "Pragmatic logo design using LLM-assisted generation. Three-phase pipeline: discovery (brand-to-design mapping), formal generation (five formal gates), and iterative refinement (weighted critique loop). Produces text-based logo design specifications and prompts."
---

# Logo Builder

Pragmatic and principled logo design using LLM-assisted generation. Synthesizes Martin (Made By James — Minimum Viable Brand), Bokhua (Principles of Logo Design — five formal gates), and Peters (Logos That Last — iterative case study method). Three-phase pipeline: discovery (brand-to-design mapping), formal generation (Bokhua gates), and iterative refinement (weighted critique loop). Produces text-based logo design specifications and detailed prompts, then generates images via the `hkask-mcp-media` server's `generate_image`, `image_remove_background`, and `upscale_image` tools.

## When to Use

- Map qualitative brand identity inputs (industry, audience, values) to formal logo design parameters before generation.
- Select an appropriate logo generation strategy (single-shot, iterative-refine, or moodboard-first) based on brand complexity and user preference.
- Generate a professional logo from formal design parameters using Bokhua's five design gates (simplicity, monochrome viability, grid discipline, negative space, scalability).
- Generate multiple logo candidate descriptions, critique them across weighted dimensions, and iteratively refine the best candidate.
- Produce a complete logo deliverables package, including a transparent PNG, monochrome variant, icon-only mark, and real-world context mockup.

## Instructions

### Phase 1 — Discovery

1. Render the `media/logo-discovery-map` template with the provided brand inputs (name, industry, audience, values, personality).
2. Call the inference router with the rendered prompt to classify and map brand attributes.
3. Parse the JSON response into formal design parameters: `style`, `logo_type`, `dominant_shape`, `typography_class`, `palette_hex`, `density`, `rationale`.
4. Select a generation strategy based on brand complexity:
   - **Single-shot** — simple brands (single word, clear industry). Use `media/logo-formal-prompt` template + `generate_image` tool.
   - **Iterative-refine** — complex brands (multiple products, unclear audience). Use Phase 3 below.
   - **Moodboard-first** — visual-first brands (luxury, fashion). Generate a moodboard image first, then use it as a reference for logo generation.

### Phase 2 — Formal Generation

1. Render the `media/logo-formal-prompt` template with the design parameters from Phase 1.
2. Call the `generate_image` tool with the rendered prompt to produce the logo image.
3. If the logo needs background removal, call `image_remove_background` on the generated image.
4. If the logo needs upscaling for print quality, call `upscale_image` on the result.

### Phase 3 — Iterative Refinement

1. Generate the specified number of initial logo candidates (default: 3) using the `media/logo-formal-prompt` template and `generate_image` tool.
2. Critique each candidate using `describe_image` with this prompt:
   > Critique this logo design for professional quality. Evaluate:
   > 1. Readability — is the name clear and legible?
   > 2. Scalability — does it work at icon size and billboard size?
   > 3. Distinctiveness — is it memorable, not generic?
   > 4. Professionalism — would a client pay for this?
   > 5. Text accuracy — are there any garbled letters, misspellings, or artifacts?
   > Return a score 1-10 for each criterion and a one-paragraph summary of the strongest weakness.
3. Select the best candidate based on the highest aggregate score.
4. Regenerate the selected candidate with this refined prompt:
   > Redesign this logo concept addressing the following critique: {critique_summary}. Keep the same business name, industry, and style direction. Fix the identified weaknesses while preserving the strengths.
5. Repeat the critique and refine cycles for the specified number of rounds (default: 1).
6. Call `image_remove_background` on the final logo for transparent PNG output.

### Phase 4 — Deliverables

1. Call `image_remove_background` on the final logo for transparent PNG output.
2. Generate a monochrome variant: call `generate_image` with prompt:
   > Monochrome (pure black on white background) version of this logo. Same exact design, no color, no gradients. High contrast. Clean sharp edges.
3. Generate a 1:1 square icon-only mark: call `generate_image` with prompt:
   > Square icon-only version of this logo mark. Remove all text and typography, keep only the symbol/icon/graphic element. 1:1 aspect ratio. Works at 64x64 pixels. Simple and recognizable at tiny sizes.
4. Generate a context mockup: call `generate_image` with prompt:
   > Photorealistic product photography mockup showing the logo for "{name}" applied in a real-world context. Show it on a clean, well-lit storefront sign or website header. Professional photography style. Good natural lighting. No other brands or logos visible. The logo should look naturally placed, not composited.
5. Return the complete deliverables package: transparent PNG, monochrome variant, icon mark, and context mockup.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `media/logo-discovery-map` | Map qualitative brand identity inputs (industry, audience, values) to formal logo design parameters (style, type, shape, palette, density) using strategic brand reasoning. From Martin's Minimum Viable Brand. |
| `media/logo-formal-prompt` | Core logo generation prompt encoding Bokhua's five design gates: simplicity, monochrome viability, grid discipline, negative space, and scalability. Every logo generation flow delegates to this template. |

To render a template, call the `render_template` tool with the template ref (e.g., `media/logo-discovery-map`) and a context object with the required variables.

## Constraints

- `media/logo-discovery-map`: Public.
- `media/logo-formal-prompt`: Public.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
