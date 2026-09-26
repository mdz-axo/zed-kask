---
name: media-workflow
description: "Multi-tool media generation pipelines: product shots, stylized art, reaction GIFs, collages, memes, and NFT derivatives. Each pipeline chains media server tools in a fixed sequence — the agent supplies the subject and style, the step topology is known-good."
---

# Media Workflow

Multi-tool media generation pipelines that chain `hkask-mcp-media` server tools in fixed sequences. Each pipeline is a known-good step topology — the agent supplies the subject, style, and parameters; the tool sequence is fixed. The agent coordinates execution by calling each tool in order, passing the previous step's output as the next step's input.

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
2. Call `image_apply_style` on the generated image with the target style prompt and strength (default: 0.75).
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
2. Generate a meme-style caption using `describe_image` with a meme-captioning prompt, or write the caption directly.
3. Call `image_to_video` on the template image with a motion prompt (e.g., "slow zoom in"). Duration: 3-5 seconds.
4. Call `video_add_caption` on the animated video with the caption text, positioned top or bottom.

### NFT Derivation Pipeline

Derives an NFT from a gallery image: style transfer, upscale, and metadata caption.

1. Call `gallery_search` to select a source image from the gallery.
2. Call `image_apply_style` on the source image with a style prompt for the NFT aesthetic.
3. Call `upscale_image` on the styled result to target resolution (scale=4 for 4K).
4. Call `describe_image` on the final image to generate a caption for NFT metadata.

### Logo Pipeline

Principled logo design (Martin, *Minimum Viable Brand*; Bokhua, *Principles of Logo Design* — five formal gates; Peters, *Logos That Last*). Brand mapping and critique are P; the operator chooses the logo, never the critique.

1. **Discovery.** Render `media/logo-discovery-map` with name, industry, audience, values and personality; send it to inference and parse `style`, `logo_type`, `dominant_shape`, `typography_class`, `palette_hex`, `density`, `rationale`. Choose single-shot (simple brand), iterative-refine (complex brand) or moodboard-first (visual-first brand, e.g. luxury, fashion).
2. **Formal generation.** Render `media/logo-formal-prompt` with those parameters (map `palette_hex`, joined into a readable list, to its `palette` input) and call `generate_image`; then `image_remove_background` and, for print, `upscale_image` as needed.
3. **Refinement (iterative-refine).** Generate 3 candidates, critique each with `describe_image` on readability, scalability, distinctiveness, professionalism and text accuracy (1–10 each, plus the strongest weakness). Show the operator every candidate with its scores and ask which to refine; if the operator is unavailable, report the ranked candidates and stop. Regenerate the chosen one addressing its critique, show it beside the previous version, and repeat only while the operator asks — at most 3 rounds.
4. **Deliverables.** `image_remove_background` for a transparent PNG; `generate_image` for a monochrome variant (pure black on white, same design), a 1:1 icon-only mark that works at 64×64, and a photorealistic real-world context mockup of "{name}". Return all four.

## Verification loop (all pipelines)

After each pipeline's final artifact, verify its acceptance property before
reporting done: product shot — `describe_image` confirms the background is
fully removed; reaction GIF — `video_info` confirms duration ≤ 5s at 480px
width; collage — `describe_image` confirms all selected subjects are present;
meme — the caption is visible in the rendered video; NFT — the style is
applied and the caption generated; logo — `describe_image` confirms the name
is spelled correctly and the icon mark carries no text. If the property fails, re-run the failing
step once with an adjusted prompt (the Act). Bound: one retry per pipeline;
a second failure ships the artifact with the imperfection named — never
silently.

## Constraints

- All pipelines use tools from the `hkask-mcp-media` server. The server must be running and configured with at least one media provider (DeepInfra or OpenRouter).
- Image generation and video generation are cloud calls — they incur cost and have latency. Local tools (collage, video_clip, video_to_gif, video_add_caption) are free and fast.
- The agent coordinates execution by calling each tool in sequence. There is no FlowDef executor — the step topology is encoded in this SKILL.md body and the model follows it.
- `media/logo-discovery-map` and `media/logo-formal-prompt` are the Logo pipeline's templates (Public); render them with `render_template`.
- This SKILL.md body is the authoritative methodology.
