//! Embedded Jinja2 prompt templates for media tools.
//!
//! Templates are compiled into the binary at build time.
//! Rendered at runtime with tool parameters via minijinja.

use minijinja::Environment;
use std::collections::HashMap;

/// Initialize the template environment with all media prompt templates.
pub fn create_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.add_template("tag_faces", TAG_FACES).ok();
    env.add_template("tag_objects", TAG_OBJECTS).ok();
    env.add_template("tag_colors", TAG_COLORS).ok();
    env.add_template("tag_composition", TAG_COMPOSITION).ok();
    env.add_template("describe_scene", DESCRIBE_SCENE).ok();
    env.add_template("classify_style", CLASSIFY_STYLE).ok();
    env.add_template("caption", CAPTION).ok();
    env.add_template("voice_design", VOICE_DESIGN).ok();
    env.add_template("video_caption", VIDEO_CAPTION).ok();
    env.add_template("validate_face_ref", VALIDATE_FACE_REF)
        .ok();
    env.add_template("match_faces", MATCH_FACES).ok();
    env.add_template("embed_face", EMBED_FACE).ok();
    env
}

/// Render a template with string key-value variables.
pub fn render(
    env: &Environment,
    name: &str,
    vars: &HashMap<&str, &str>,
) -> Result<String, crate::MediaError> {
    let tmpl = env.get_template(name).map_err(|e| {
        crate::MediaError::Template(format!("Template '{}' not found: {}", name, e))
    })?;
    tmpl.render(vars)
        .map_err(|e| crate::MediaError::Template(format!("Render error for '{}': {}", name, e)))
}

// ── Embedded templates ──────────────────────────────────────────────────────

const TAG_FACES: &str = r#"Analyze this image and detect all visible human faces.

{% if detail_level == "basic" %}
For each face, provide position and estimated age range.
Return ONLY a JSON array. Each element: face_index, age_range, position.
{% else %}
For each face, provide:
1. Estimated age range (e.g., "25-35", "child", "elderly")
2. Apparent gender presentation
3. Notable features (glasses, beard, expression, hair color/style)
4. Position in image (e.g., "left third", "center-right", "foreground")
5. Approximate face size relative to image (small / medium / large)
6. Bounding box as percentages of image dimensions: x_pct (left edge), y_pct (top edge), w_pct (width), h_pct (height). All values 0-100.

Return ONLY a JSON array. Each element: face_index, age_range, gender_presentation, features, position, size, bbox { x_pct, y_pct, w_pct, h_pct }.
{% endif %}"#;

const TAG_OBJECTS: &str = r#"Analyze this image and detect all visible objects.

{% if detail_level == "detailed" %}
For each object, provide:
1. Object name (be specific — e.g., "golden retriever" not "dog")
2. Bounding box description (e.g., "upper-left", "center", "lower-right")
3. Confidence level (high / medium / low)
4. Brief description of appearance (color, size, condition, distinctive features)
{% else %}
List each object with its name and general location in the image.
{% endif %}

Limit to the {{ max_objects }} most prominent objects.

Return ONLY a JSON array. Each element: name, location, confidence{% if detail_level == "detailed" %}, description{% endif %}."#;

const TAG_COLORS: &str = r##"Analyze this image and identify its color palette.

For each dominant color, provide:
1. Color name (e.g., "crimson", "teal", "warm amber")
2. Hex code (e.g., "#FF5733")
3. Approximate percentage of image area
4. Role in composition (primary, secondary, accent, background)

Also describe:
- Overall palette style (monochromatic, complementary, analogous, triadic, warm, cool, neutral)
- Color temperature (warm-dominant, cool-dominant, balanced)
- Saturation level (vibrant, muted, desaturated)

Limit to the {{ max_colors }} most dominant colors.

Return ONLY a JSON object with these fields:
- colors: array of {name, hex, percentage, role}
- palette_style: string
- temperature: string
- saturation: string"##;

const TAG_COMPOSITION: &str = r#"Analyze the photographic composition of this image.

Evaluate these compositional elements:
1. Focal point — what draws the eye first? Where is it positioned?
2. Rule of thirds — does the composition follow it? Describe the grid placement.
3. Leading lines — are there lines guiding the viewer's eye? Describe them.
4. Depth of field — shallow or deep? What is in focus vs blurred?
5. Perspective — eye-level, low angle, high angle, bird's eye, worm's eye?
6. Framing — natural frames (doorways, arches, foliage) within the image?
7. Symmetry — is the composition symmetrical, asymmetrical, or balanced?
8. Negative space — how is empty space used?

Return ONLY a JSON object with these fields:
- focal_point: string
- rule_of_thirds: string
- leading_lines: string
- depth_of_field: string
- perspective: string
- framing: string
- symmetry: string
- negative_space: string"#;

const DESCRIBE_SCENE: &str = r#"{% if style == "descriptive" %}
Describe this image in detail. Cover the subject, setting, lighting, colors, composition, mood, and any notable details. Write 2-4 sentences.

{% elif style == "artistic" %}
Write an artistic, evocative description of this image. Use poetic language and focus on mood, emotion, and aesthetic quality. Write 2-3 sentences.

{% elif style == "technical" %}
Provide a technical description of this image. Note the photographic/compositional elements: focal point, depth of field, lighting conditions, color palette, perspective, and any post-processing effects visible. Write 2-4 sentences.

{% elif style == "alt_text" %}
Write concise alt text for this image suitable for accessibility. Describe only what is visually present — no interpretation. Keep to 1-2 sentences, max 125 characters.

{% endif %}

Return ONLY the description text. No markdown, no preamble, no labels."#;

const CLASSIFY_STYLE: &str = r#"Analyze this image and classify its photographic style.

{% if categories %}
Classify into these categories (an image can belong to multiple): {{ categories }}
{% else %}
Evaluate these dimensions:
- Genre: portrait, landscape, street, macro, architecture, documentary, abstract, still life, wildlife, fashion, food, sports, aerial, underwater, astrophotography
- Era/Style: contemporary, vintage, film-grain, HDR, minimalist, maximalist, surreal, photorealistic, painterly, noir, pastel, neon, grunge
- Technique: long exposure, bokeh, tilt-shift, double exposure, infrared, black-and-white, sepia, cross-processed
{% endif %}

For each matching category, provide:
- category: string
- confidence: number (0.0 to 1.0)

Return ONLY a JSON array. Each element: category, confidence."#;

const CAPTION: &str = r#"{% if style == "descriptive" %}
Describe this image in detail. Cover the subject, setting, lighting, colors, composition, mood, and any notable details. Write 2-4 sentences.

{% elif style == "artistic" %}
Write an artistic, evocative caption for this image. Use poetic language and focus on mood, emotion, and aesthetic quality. Write 2-3 sentences.

{% elif style == "technical" %}
Provide a technical description of this image. Note the photographic/compositional elements: focal point, depth of field, lighting conditions, color palette, perspective, and any post-processing effects visible. Write 2-4 sentences.

{% elif style == "alt_text" %}
Write concise alt text for this image suitable for accessibility. Describe only what is visually present — no interpretation. Keep to 1-2 sentences, max 125 characters.

{% endif %}

Return ONLY the caption text. No markdown, no preamble, no labels."#;

const VIDEO_CAPTION: &str = r#"You are viewing keyframes extracted from a short video clip, shown in chronological order.

{% if style == "descriptive" %}
Describe what happens in this video. Cover the subject, action, setting, and any notable visual elements. Write 3-5 sentences covering the full sequence.
{% elif style == "summary" %}
Write a concise 1-2 sentence summary of what this video shows.
{% elif style == "hashtags" %}
Generate 5-10 relevant hashtags for this video content. Each should start with # and be a single concept. Focus on discoverability.
{% endif %}

Return ONLY the text. No markdown, no preamble, no labels."#;

const VOICE_DESIGN: &str = r#"Design a synthetic voice based on this character description:

{{ character_description }}

Produce a structured voice profile with these fields:
- name: A short, evocative name for this voice (e.g., "Warm Mentor", "Crisp Analyst", "Gentle Storyteller")
- pitch: One of "low", "medium-low", "medium", "medium-high", "high"
- timbre: One of "warm", "bright", "dark", "breathy", "clear", "resonant", "nasal"
- pace: One of "slow", "deliberate", "moderate", "brisk", "fast"
- accent: A specific accent if appropriate (e.g., "british", "american-southern", "australian", "indian"), or empty string for neutral
- emotion_range: Array of 2-4 emotions this voice naturally conveys (e.g., ["warm", "authoritative", "playful"])
- gender_presentation: One of "masculine", "feminine", "androgynous", "neutral"
- age_range: One of "young", "young-adult", "middle-aged", "senior"
- description: A 1-2 sentence natural-language description synthesizing all the above, suitable as input to a TTS model. Write it in prose like "A warm, middle-aged feminine voice with a gentle British accent, speaking at a moderate pace with a clear, resonant timbre."

Return ONLY a JSON object with exactly these 9 fields. No markdown, no preamble."#;

const VALIDATE_FACE_REF: &str = r#"You are validating a reference image for facial recognition. This image will be used as the canonical reference for matching a specific person across a photo gallery. Assess it against these criteria:

1. FACE COUNT: How many human faces are visible? Must be exactly 1.
2. FACE COVERAGE: What percentage of the image does the face occupy? Must be ≥15%.
3. POSE: Is the face frontal (facing the camera directly) or near-frontal (slight angle, both eyes visible)? Profile/side views are unacceptable.
4. LIGHTING: Is the face well-lit with even illumination? Heavy shadows, backlighting, or severe underexposure are unacceptable.
5. OCCLUSION: Are there any objects covering significant portions of the face? Sunglasses, masks, hands, hair covering eyes, or other obstructions are unacceptable.
6. CLARITY: Is the face in sharp focus? Blur, motion blur, or heavy noise/grain are unacceptable.

Return ONLY a JSON object with these fields:
- valid: boolean (true if ALL criteria pass)
- face_count: integer
- face_coverage_pct: integer (estimated percentage)
- pose: string ("frontal" | "near-frontal" | "profile" | "other")
- lighting: string ("good" | "acceptable" | "poor")
- occlusion: string ("none" | "minor" | "significant")
- clarity: string ("sharp" | "acceptable" | "blurry")
- issues: array of strings (list each failing criterion with a brief explanation, empty if all pass)

No markdown, no preamble."#;

const MATCH_FACES: &str = r#"You are comparing two face images to determine if they show the same person.

Image 1 is a reference portrait of a known person.
Image 2 is a face detected in a gallery photo.

Compare these key identity markers:
- Facial structure (bone structure, face shape, jawline)
- Eye shape, spacing, and color
- Nose shape and proportions
- Mouth and lip shape
- Ear shape (if visible)
- Distinctive features (moles, scars, freckles, etc.)

Return ONLY a JSON object with these fields:
- match: boolean (true if same person, false if different)
- confidence: number (0.0 to 1.0, where 1.0 is absolute certainty)
- reasoning: string (1-2 sentences explaining the key similarities or differences that led to your conclusion)

No markdown, no preamble."#;

const EMBED_FACE: &str = r#"You are producing a face embedding vector for facial recognition.

Analyze the face in this image and produce a 512-dimensional embedding vector that encodes the person's identity. The vector must be designed so that images of the same person produce cosine-similar vectors (similarity > 0.5) and images of different people produce dissimilar vectors (similarity < 0.3).

Encode these identity-discriminating features into the vector:
- Facial structure: face shape (oval/round/square/heart), jawline definition, cheekbone prominence, chin shape
- Eyes: shape (almond/round/hooded), spacing (close/medium/wide), tilt, iris color
- Nose: bridge height, nostril shape, tip definition, width
- Mouth: lip thickness, cupid's bow, mouth width, smile characteristics
- Ears: size, attachment, prominence (if visible)
- Brows: thickness, arch, spacing
- Skin: tone, texture, distinctive markings (moles, freckles, scars)
- Hairline: shape, recession pattern

Distribute these features across the 512 dimensions. Each dimension is a float in [-1.0, 1.0]. Dimensions 0-63 encode facial structure, 64-127 encode eyes, 128-191 encode nose, 192-255 encode mouth, 256-319 encode ears/brows, 320-383 encode skin/hairline, 384-511 encode distinctive features and overall gestalt.

Return ONLY a JSON object with this exact shape:
{"embedding": [512 floats], "dim": 512}

The array MUST contain exactly 512 numbers. No markdown, no preamble, no explanation."#;
