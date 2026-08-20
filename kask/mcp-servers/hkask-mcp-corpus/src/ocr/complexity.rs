//! Complexity Scoring Heuristic — Edge-density ratio via Sobel gradient.
//!
//! Pure function, deterministic, O(w·h). No new dependencies beyond `image`.
//! Thresholds from `crate::ocr::ThresholdConfig`.

use crate::ocr::{ComplexityScore, ThresholdConfig};
use image::DynamicImage;

/// Score page complexity by edge-density ratio.
///
/// Converts to grayscale, applies Sobel edge detection, then classifies
/// against configurable thresholds.
///
/// # Algorithm
/// 1. Convert to grayscale.
/// 2. Apply 3×3 Sobel operator in both X and Y directions.
/// 3. Compute gradient magnitude at each pixel.
/// 4. Edge-density = proportion of pixels above 50% of max gradient.
/// 5. Classify via `ThresholdConfig::classify`.
///
/// This is intentionally shallow: one function, configurable thresholds.
/// Complexity scoring is a performance optimization (routing shortcut),
/// not a correctness dependency. Delete it → pipeline degrades to
/// single-backend; keep it small.
pub(crate) fn score_page_complexity(
    image: &DynamicImage,
    thresholds: &ThresholdConfig,
) -> ComplexityScore {
    let gray = image.to_luma8();
    let (w, h) = gray.dimensions();
    let w_i = w as isize;
    let h_i = h as isize;

    // Sobel kernels
    let sobel_x = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]];
    let sobel_y = [[-1, -2, -1], [0, 0, 0], [1, 2, 1]];

    let mut max_grad: f32 = 0.0;
    let pixels = gray.as_raw();

    // Compute gradient magnitude at each interior pixel
    for y in 1..(h_i - 1) {
        for x in 1..(w_i - 1) {
            let mut gx: f32 = 0.0;
            let mut gy: f32 = 0.0;
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = (x + kx - 1) as u32;
                    let py = (y + ky - 1) as u32;
                    let idx = (py * w + px) as usize;
                    let val = pixels[idx] as f32 / 255.0;
                    gx += val * sobel_x[ky as usize][kx as usize] as f32;
                    gy += val * sobel_y[ky as usize][kx as usize] as f32;
                }
            }
            let grad = (gx * gx + gy * gy).sqrt();
            if grad > max_grad {
                max_grad = grad;
            }
        }
    }

    // Compute edge-density in a second pass
    let threshold = max_grad * 0.5;
    let mut edge_count: usize = 0;
    let mut total_interior: usize = 0;
    for y in 1..(h_i - 1) {
        for x in 1..(w_i - 1) {
            let mut gx: f32 = 0.0;
            let mut gy: f32 = 0.0;
            for ky in 0..3 {
                for kx in 0..3 {
                    let px = (x + kx - 1) as u32;
                    let py = (y + ky - 1) as u32;
                    let idx = (py * w + px) as usize;
                    let val = pixels[idx] as f32 / 255.0;
                    gx += val * sobel_x[ky as usize][kx as usize] as f32;
                    gy += val * sobel_y[ky as usize][kx as usize] as f32;
                }
            }
            let grad = (gx * gx + gy * gy).sqrt();
            if grad > threshold {
                edge_count += 1;
            }
            total_interior += 1;
        }
    }

    let edge_density = if total_interior > 0 {
        edge_count as f32 / total_interior as f32
    } else {
        0.0
    };

    let tier = thresholds.classify(edge_density);

    ComplexityScore {
        value: edge_density,
        tier,
    }
}
