---
name: media-workflow
description: "Multi-tool media generation pipelines: product shots, stylized art, reaction GIFs, collages, memes, and NFT derivatives. Each pipeline chains media server tools in a fixed sequence — the agent supplies the subject and style, the step topology is known-good."
---

# Media Workflow

Multi-tool media generation pipelines that chain `hkask-mcp-media` server tools in fixed sequences. Each pipeline is a known-good step topology — the agent supplies the subject, style, and parameters; the tool sequence is fixed. The agent coordinates execution by calling each tool in order, passing the previous step's output as the next step's input.

## Reference model

MovieLabs Ontology for Media Creation (OMC) — the creation-graph vocabulary the media server records for every asset (`kask/crates/hkask-bridge-ontology/src/omc.rs`; `gallery_asset_detail` returns the OMC creation graph). The pipeline topologies themselves are house recipes with no published source: known-good tool orderings, each proven by its Verification-loop acceptance property, not by a methodology. The Logo pipeline's five design gates cite Bokhua (see `media/logo-formal-prompt`).

## Initial and target condition

- **Initial condition:** the chosen pipeline and its inputs — subject, style, brand inputs, or source gallery image.
- **Target condition:** the pipeline's acceptance property in the Verification loop holds, and the operator has seen the final artifact.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Media tool calls | D | the tool's receipt; `video_info` for GIF duration and width |
| Prompts, captions, logo brand mapping | P | the operator |
| `describe_image` checks and logo scores (1–10) | P | model estimates; the operator reviews the artifact before it counts as done |

## When to Use

- Generate a product shot with clean background removal and upscaling.
- Create stylized artwork with style transfer and final-resolution upscaling.
- Create a reaction GIF from a text prompt via image generation + video animation + GIF conversion.
- Create a collage from gallery images with background removal.
- Create a meme video from a gallery template image.
- Derive an NFT from a gallery image with style transfer, upscaling, and metadata caption.
- Design a logo from brand inputs through formal gates, operator-chosen refinement, and a deliverables package (the Logo pipeline).

## When NOT to Use

- Brand strategy with no logo to generate — the Logo pipeline maps identity to design parameters and produces a logo; strategy alone is a different deliverable.
- Single-tool media operations — call the media tool directly; these workflows are fixed multi-tool pipelines.
- Audio/transcript work — use `transcript-reel` (capture, correction, speaker passes, EDL rendering).

## Instructions

### Product Shot Pipeline

Generates a clean product image, removes the background for a clean cutout, and upscales to 4K.

1. Call `generate_image` with a product photography prompt: centered product, studio lighting, clean background.
2. Call `image_remove_background` on the generated image.
3. Call `upscale_image` on the result with scale=4 for 4K output.

### Stylize-and-Upscale Pipeline

Generates a base image, applies an artistic style transfer, then upscales to final resolution.

1. Call `generate_image` with the subject prompt.
2. Call `image_apply_style` on the generated image with the target style prompt and strength (use 0.75 unless the operator directs otherwise — the tool carries no default).
3. Call `upscale_image` on the styled result with scale=2 or scale=4.

### Reaction-GIF Pipeline

Generates a still image, animates it into a short video clip, then converts to a shareable GIF.

1. Call `generate_image` with the reaction scene prompt.
2. Call `image_to_video` on the generated image with a motion prompt (e.g., "slow zoom in", "dramatic pan right"). Duration: 3-5 seconds.
3. Call `video_to_gif` on the generated video clip. Width: 480px, FPS: 15 for web-optimized GIF.

### Collage Pipeline

Creates a collage from gallery images with background removal for transparent compositing.

1. Call `gallery_search` to find images matching the desired theme, or use `gallery_organize` to set up a gallery first.
2. Call `image_remove_background` on each selected image for transparent PNGs.
3. Call `image_create_collage` with the processed images, layout (grid/horizontal/vertical/masonry), spacing, and canvas size.

### Meme Pipeline

Creates a meme video: select a template, generate a caption, animate, and overlay text.

1. Call `gallery_search` or use a gallery image index to select a meme template image.
2. Generate a meme-style caption yourself, or use `describe_image`
   (style "descriptive") on the template image as grounding for your own
   caption — the tool returns a caption, not a meme prompt.
3. Call `image_to_video` on the template image with a motion prompt (e.g., "slow zoom in"). Duration: 3-5 seconds.
4. Call `video_add_caption` on the animated video with the caption text, positioned top or bottom. (The server also registers a purpose-built `video_meme` tool — text overlay plus motion in one call — as an alternative to steps 3–4.)

### NFT Derivation Pipeline

Derives an NFT from a gallery image: style transfer, upscale, and metadata caption.

1. Call `gallery_search` to select a source image from the gallery.
2. Call `image_apply_style` on the source image with a style prompt for the NFT aesthetic.
3. Call `upscale_image` on the styled result to target resolution (scale=4 for 4K).
4. Call `describe_image` on the final image to generate a caption for NFT metadata.

### Logo Pipeline

Principled logo design (Martin, *Minimum Viable Brand*; Bokhua, *Principles of Logo Design* — five formal gates). Brand mapping and critique are P; the operator chooses the logo, never the critique.

1. **Discovery.** Render `media/logo-discovery-map` with name, industry, audience, values; send it to inference and parse `style`, `logo_type`, `dominant_shape`, `typography_class`, `palette_direction`, `palette_hex`, `density`, `rationale`. Choose single-shot (simple brand), iterative-refine (complex brand) or moodboard-first (visual-first brand, e.g. luxury, fashion).
2. **Formal generation.** Render `media/logo-formal-prompt` with the consumed parameters (map `palette_hex`, joined into a readable list, to its `palette` input; `rationale` is discovery's output, not this template's input) and call `generate_image`; then `image_remove_background` and, for print, `upscale_image` as needed.
3. **Refinement (iterative-refine).** Generate 3 candidates, critique each — your own model judgment on readability, scalability, distinctiveness, professionalism and text accuracy (1–10 each, plus the strongest weakness; `describe_image` style "descriptive" can ground the readability check, but the tool returns a caption, not a rubric — the scores are model estimates, per the Step types table). Show the operator every candidate with its scores and ask which to refine; if the operator is unavailable, report the ranked candidates and stop. Regenerate the chosen one addressing its critique, show it beside the previous version, and repeat only while the operator asks — at most 3 rounds.
4. **Deliverables.** `image_remove_background` for a transparent PNG; `generate_image` for a monochrome variant (pure black on white, same design), a 1:1 icon-only mark that works at 64×64, and a photorealistic real-world context mockup of "{name}". Return all four.

## Verification loop (all pipelines)

After each pipeline's final artifact, verify its acceptance property before
reporting done: product shot — `describe_image` confirms the background is
fully removed; reaction GIF — `video_info` confirms duration ≤ 5s at 480px
width; collage — `describe_image` confirms all selected subjects are present;
meme — the caption is visible in the rendered video; NFT — the style is
applied and the caption generated; logo — `describe_image` confirms the name
is spelled correctly and the icon mark carries no text. Only `video_info` is a deterministic check; every `describe_image` check is a vision model judging generated media, so show the artifact to the operator before reporting it done. If the property fails, re-run the failing
step once with an adjusted prompt (the Act). Bound: one retry per pipeline;
a second failure delivers the artifact with the imperfection named — never
silently.

## Cleanup (all pipelines)

After the operator accepts the deliverables, delete the run's rejected outputs — unchosen logo candidates, failed-retry artifacts and other variants that are not ancestors of a kept deliverable — with `gallery_delete_image(image_id, delete_file: true)`. Keep every ancestor of a kept deliverable: its lineage and OMC creation graph reference them. File deletion needs a destructive-mode gallery; in read-only or copy-on-write mode, remove only the index entry and list the files left on disk in the report. Confirm with `gallery_list_assets` (storage Cleanup rule).

## Regression case

All receipts executed live (2026-10-01, batch-14 audit):

- Logo discovery render: `render_template` `media/logo-discovery-map`
  with a brand brief (name, industry, audience, values)
  renders the strategist prompt with the closed parameter schema
  (style, logo_type, dominant_shape, typography_class, palette_direction,
  palette_hex, density, rationale) — the discovery step's D output.
- Logo formal-prompt render: `render_template`
  `media/logo-formal-prompt` with the mapped parameters (palette_hex
  joined into the `palette` list per the Instructions) renders the
  generation prompt carrying Bokhua's five design gates verbatim
  (G1 simplicity … G5 scalability) — the formal-generation step's D
  output.

The pipelines' verification loop is honest about its oracles: only
`video_info` is deterministic; every `describe_image` check is a vision
model judgment shown to the operator. There is no lisp_eval form — the
D steps are the tool receipts and the template renders.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `logo-discovery-map.j2` | Map a brand brief to formal logo design parameters (style, logo type, shape, typography, palette, density) with a rationale. |
| `logo-formal-prompt.j2` | The logo generation prompt encoding Bokhua's five design gates (simplicity, monochrome viability, grid discipline, negative space, scalability). |

Both live under `kask/registry/templates/media/` (the `media/` namespace,
not `media-workflow/`). Template context variables (formal-prompt: its
`[inference]` contract; discovery-map: the Jinja body — its `[inference]`
block is the legacy shape retained for provenance only):
- `logo-discovery-map.j2`: `name` (required), `industry` (required),
  `audience`, `values` — its output schema adds `style`, `logo_type`,
  `dominant_shape`, `typography_class`, `palette_direction`, `palette_hex`,
  `density`, `rationale` (parsed by the caller, not consumed by
  formal-prompt)
- `logo-formal-prompt.j2`: `name`, `logo_type`, `style`, `dominant_shape`,
  `density`, `industry`, `palette`, `tagline`, `typography_class`

## Constraints

- All pipelines use tools from the `hkask-mcp-media` server. The server must be running and configured with at least one media provider (DeepInfra or OpenRouter).
- Image generation and video generation are cloud calls — they incur cost and have latency. Local tools (collage, video_clip, video_to_gif, video_add_caption) are free and fast.
- The agent coordinates execution by calling each tool in sequence. There is no FlowDef executor — the step topology is encoded in this SKILL.md body and the model follows it.
- `media/logo-discovery-map` and `media/logo-formal-prompt` are the Logo pipeline's templates; render them with `render_template`.
