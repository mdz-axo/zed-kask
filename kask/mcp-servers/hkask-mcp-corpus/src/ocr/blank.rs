//! Pre-call blank-page detection.
//!
//! A page whose image carries no recoverable ink is blank — a fact about
//! the source, not a conversion failure. Detecting it before the vision
//! call skips the generation entirely: no limiter slot, no provider
//! request, no deadline risk (the 2026-10-03 zk-ref class — empty and
//! near-blank pages deterministically burned full deadline budgets under
//! load for ~0 words while converting clean solo).

use image::DynamicImage;

/// Luminance distance from the modal background bucket for a pixel to
/// count as ink. Anti-aliasing fringes and scanner noise sit inside this
/// band; glyphs, stamps, and figures sit well outside it.
const INK_LUMA_DELTA: i32 = 40;

/// A page whose ink covers less than this fraction of its pixels is
/// confidently blank. Deliberately conservative: a library stamp
/// (~0.1-0.2% ink) or a sparse real page stays content-ambiguous and goes
/// to the model — only pages with essentially no ink skip the call.
const BLANK_INK_RATIO_MAX: f64 = 0.001;

/// Whether a page image is confidently blank.
///
/// The modal luminance bucket is the page background (paper white, or the
/// dark surround of an inverted scan — the modal bucket handles both).
/// Ink is any pixel at least [`INK_LUMA_DELTA`] from the bucket's center;
/// a blank page is one whose ink fraction is under [`BLANK_INK_RATIO_MAX`].
pub(crate) fn is_blank_page(image: &DynamicImage) -> bool {
    let rgb = image.to_rgb8();
    let (width, height) = (rgb.width() as usize, rgb.height() as usize);
    if width == 0 || height == 0 {
        // A degenerate image carries no content — no ink to recover.
        return true;
    }
    let luma: Vec<u8> = rgb
        .pixels()
        .map(|p| {
            let [r, g, b] = p.0;
            ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8
        })
        .collect();
    // 16 luminance buckets of 16 levels each: the modal bucket is the
    // background.
    let mut histogram = [0u64; 16];
    for &l in &luma {
        histogram[(l >> 4) as usize] += 1;
    }
    let modal_bucket = histogram
        .iter()
        .enumerate()
        .max_by_key(|&(_, count)| count)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let modal_center = (modal_bucket as i32) * 16 + 8;
    let ink = luma
        .iter()
        .filter(|&&l| (l as i32 - modal_center).abs() >= INK_LUMA_DELTA)
        .count();
    let total = luma.len() as f64;
    (ink as f64 / total) < BLANK_INK_RATIO_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, value: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb(value),
        ))
    }

    /// A uniform page (any hue — blank paper, black film, an inverted
    /// scan's surround) has no ink: blank.
    #[test]
    fn uniform_pages_are_blank() {
        assert!(is_blank_page(&solid(64, 64, [255, 255, 255])));
        assert!(is_blank_page(&solid(64, 64, [0, 0, 0])));
        assert!(is_blank_page(&solid(64, 64, [200, 180, 160])));
    }

    /// A page with a text-sized block (a paragraph, a title) is not blank.
    #[test]
    fn text_pages_are_not_blank() {
        let mut buffer = image::RgbImage::from_pixel(64, 64, image::Rgb([255, 255, 255]));
        for y in 20..40 {
            for x in 10..50 {
                buffer.put_pixel(x, y, image::Rgb([30, 30, 30]));
            }
        }
        assert!(!is_blank_page(&DynamicImage::ImageRgb8(buffer)));
    }

    /// An inverted scan (dark surround, light text) is not blank — the
    /// modal bucket is the surround, and the text is ink against it.
    #[test]
    fn inverted_pages_are_not_blank() {
        let mut buffer = image::RgbImage::from_pixel(64, 64, image::Rgb([20, 20, 20]));
        for y in 20..40 {
            for x in 10..50 {
                buffer.put_pixel(x, y, image::Rgb([240, 240, 240]));
            }
        }
        assert!(!is_blank_page(&DynamicImage::ImageRgb8(buffer)));
    }

    /// A stamp-sized mark (well under 1% ink) stays content-ambiguous:
    /// the conservative threshold does NOT classify it blank, so it goes
    /// to the model (the 2026-10-03 library-stamp evidence).
    #[test]
    fn stamp_sized_marks_are_not_blank() {
        let mut buffer = image::RgbImage::from_pixel(200, 200, image::Rgb([255, 255, 255]));
        // ~0.5% ink — five times the blank ceiling.
        for y in 95..105 {
            for x in 90..110 {
                buffer.put_pixel(x, y, image::Rgb([30, 30, 30]));
            }
        }
        assert!(!is_blank_page(&DynamicImage::ImageRgb8(buffer)));
    }

    /// Speckle noise under the ink ceiling reads as blank.
    #[test]
    fn speckle_pages_are_blank() {
        let mut buffer = image::RgbImage::from_pixel(200, 200, image::Rgb([255, 255, 255]));
        // A handful of dark specks — far under 0.1%.
        for &(x, y) in &[(10, 10), (50, 80), (150, 120), (99, 33)] {
            buffer.put_pixel(x, y, image::Rgb([40, 40, 40]));
        }
        assert!(is_blank_page(&DynamicImage::ImageRgb8(buffer)));
    }
}
