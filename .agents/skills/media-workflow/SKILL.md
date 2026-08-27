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

## Constraints

- All pipelines use tools from the `hkask-mcp-media` server. The server must be running and configured with at least one media provider (DeepInfra or OpenRouter).
- Image generation and video generation are cloud calls — they incur cost and have latency. Local tools (collage, video_clip, video_to_gif, video_add_caption) are free and fast.
- The agent coordinates execution by calling each tool in sequence. There is no FlowDef executor — the step topology is encoded in this SKILL.md body and the model follows it.
- This SKILL.md body is the authoritative methodology.
