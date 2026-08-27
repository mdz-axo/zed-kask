//! Meme text rendering — font loading, measurement, and outlined drawing
//! on top of the `image` crate's buffer operations.

use crate::error::MediaError;
use ab_glyph::Font;

/// Load a font for meme text rendering. Tries the provided path first,
/// then common system paths, then returns an error with guidance.
pub(crate) fn load_meme_font(font_path: Option<&str>) -> Result<ab_glyph::FontVec, MediaError> {
    if let Some(path) = font_path {
        // Reject path traversal attempts — font_path must be a simple filename
        if path.contains('/') || path.contains('\\') || path.contains("..") {
            return Err(MediaError::Io(format!(
                "font_path must be a simple filename, not a path: '{}'",
                path
            )));
        }
        let data = std::fs::read(path)
            .map_err(|e| MediaError::Io(format!("Cannot read font at '{}': {}", path, e)))?;
        return ab_glyph::FontVec::try_from_vec(data)
            .map_err(|e| MediaError::Io(format!("Invalid font file at '{}': {:?}", path, e)));
    }

    // Try common system paths
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    ];

    for path in &candidates {
        if let Ok(data) = std::fs::read(path)
            && let Ok(font) = ab_glyph::FontVec::try_from_vec(data)
        {
            return Ok(font);
        }
    }

    Err(MediaError::Io("No system font found".to_string()))
}

/// Measure rendered text dimensions for centering.
pub(crate) fn measure_text(
    font: &ab_glyph::FontVec,
    scale: ab_glyph::PxScale,
    text: &str,
) -> (u32, u32) {
    let mut total_width = 0.0f32;
    for c in text.chars() {
        let glyph_id = font.glyph_id(c);
        total_width += font.h_advance_unscaled(glyph_id) * scale.x;
    }
    let height = (font.ascent_unscaled() * scale.y / font.height_unscaled()).ceil() as u32;
    (total_width.ceil() as u32, height)
}

/// Draw text onto an image with alpha-blended glyph rasterization.
///
/// Replaces `imageproc::drawing::draw_text_mut` — uses only `ab_glyph`'s
/// built-in rasterizer + `image` pixel manipulation, dropping `imageproc`
/// and its `nalgebra` transitive dep tree (~155 packages).
pub(crate) fn draw_text_mut(
    img: &mut image::DynamicImage,
    color: image::Rgba<u8>,
    x: i32,
    y: i32,
    scale: ab_glyph::PxScale,
    font: &ab_glyph::FontVec,
    text: &str,
) {
    use ab_glyph::{Font, ScaleFont};
    let scaled = font.as_scaled(scale);
    let mut pen = ab_glyph::point(x as f32, y as f32 + scaled.ascent());
    let Some(img_buf) = img.as_mut_rgba8() else {
        return;
    };
    for ch in text.chars() {
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = pen;
        let advance = scaled.h_advance(glyph.id);
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let bb = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = (bb.min.x + gx as f32) as i32;
                let py = (bb.min.y + gy as f32) as i32;
                if px >= 0
                    && py >= 0
                    && (px as u32) < img_buf.width()
                    && (py as u32) < img_buf.height()
                {
                    let pixel = img_buf.get_pixel_mut(px as u32, py as u32);
                    let alpha = (coverage * 255.0).round() as u8;
                    if alpha > 0 {
                        for i in 0..4 {
                            pixel[i] = ((pixel[i] as u32 * (255 - alpha as u32)
                                + color[i] as u32 * alpha as u32)
                                / 255) as u8;
                        }
                    }
                }
            });
        }
        pen.x += advance;
    }
}
