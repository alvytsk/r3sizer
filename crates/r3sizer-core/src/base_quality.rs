//! Base resize quality scoring (step 4).
//!
//! Computes [`BaseResizeQuality`] from the source and downscaled images immediately
//! after the resize stage, before any sharpening is applied.
//!
//! # Ringing score (active in v1)
//!
//! Measured via a sign-flip oscillation detector on the resized luma: near-edge
//! pixels where the forward and backward 1-D differences change sign are counted
//! as ringing.  This is a no-reference spatial proxy — it does not require the
//! source image and captures Lanczos/sinc overshoot near sharp transitions.
//!
//! # Edge and texture retention (diagnostic-only in v1)
//!
//! Both are reference-aware ratios (resized / source) and do not affect the
//! solver budget in this release.
//!
//! # Envelope scale
//!
//! ```text
//! envelope_scale = clamp(1.0 − RINGING_K × ringing_score, MIN_ENVELOPE_SCALE, 1.0)
//! effective_p0   = target_artifact_ratio × envelope_scale
//! ```

use crate::{BaseResizeQuality, LinearRgbImage};

/// Coefficient mapping ringing_score to budget reduction.
/// `envelope_scale = 1.0 − RINGING_K × ringing_score` (then clamped).
const RINGING_K: f32 = 2.0;

/// Minimum allowed envelope scale — prevents a noisy ringing estimate from
/// collapsing the sharpening budget.  Conservative: keeps at least 65% of the
/// original budget even for maximally-ringy resize outputs.
const MIN_ENVELOPE_SCALE: f32 = 0.65;

/// Minimum gradient magnitude (unnormalized Sobel scale; max ≈ 5.66 for luma
/// in [0, 1]) required to consider a pixel "near an edge" for ringing detection.
const EDGE_THRESHOLD: f32 = 0.10;

/// Minimum absolute 1-D pixel difference for a sign-flip to be counted.
/// Filters out sub-percent noise while catching meaningful ringing.
const MIN_DIFF: f32 = 0.01;

// ---------------------------------------------------------------------------
// Shared: luminance extraction
// ---------------------------------------------------------------------------

#[inline]
fn pixel_luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn extract_luma(img: &LinearRgbImage) -> Vec<f32> {
    img.pixels()
        .chunks_exact(3)
        .map(|rgb| pixel_luma(rgb[0], rgb[1], rgb[2]))
        .collect()
}

// ---------------------------------------------------------------------------
// Ringing score (no-reference, resized image only)
// ---------------------------------------------------------------------------

/// Fraction of near-edge pixels showing sign-flip oscillation in the horizontal
/// or vertical 1-D difference — a spatial proxy for ringing.
///
/// For each pixel with gradient magnitude > [`EDGE_THRESHOLD`]:
/// - compute forward and backward 1-D differences in x and y
/// - if both differences exceed [`MIN_DIFF`] and have opposite signs → ringing pixel
///
/// Returns a value in `[0, 1]`; higher means more ringing.
pub(crate) fn compute_ringing_score(luma: &[f32], w: usize, h: usize) -> f32 {
    if w < 3 || h < 3 {
        return 0.0;
    }

    let clamp_x = |x: isize| -> usize { (x.max(0) as usize).min(w - 1) };
    let clamp_y = |y: isize| -> usize { (y.max(0) as usize).min(h - 1) };
    let px = |xi: isize, yi: isize| -> f32 { luma[clamp_y(yi) * w + clamp_x(xi)] };

    let mut ringing_count = 0u32;

    for y in 0..h {
        for x in 0..w {
            let xi = x as isize;
            let yi = y as isize;

            // Unnormalized 3×3 Sobel gradient magnitude.
            let gx = -px(xi - 1, yi - 1) + px(xi + 1, yi - 1)
                - 2.0 * px(xi - 1, yi)
                + 2.0 * px(xi + 1, yi)
                - px(xi - 1, yi + 1)
                + px(xi + 1, yi + 1);
            let gy = -px(xi - 1, yi - 1) - 2.0 * px(xi, yi - 1) - px(xi + 1, yi - 1)
                + px(xi - 1, yi + 1)
                + 2.0 * px(xi, yi + 1)
                + px(xi + 1, yi + 1);
            let grad = (gx * gx + gy * gy).sqrt();

            if grad < EDGE_THRESHOLD {
                continue; // not near an edge — skip
            }

            // Sign-flip oscillation in x: fwd and bwd differences change sign.
            let oscillates_x = if x > 0 && x + 1 < w {
                let fwd = luma[y * w + x + 1] - luma[y * w + x];
                let bwd = luma[y * w + x] - luma[y * w + x - 1];
                fwd.abs() > MIN_DIFF && bwd.abs() > MIN_DIFF && fwd * bwd < 0.0
            } else {
                false
            };

            // Sign-flip oscillation in y.
            let oscillates_y = if y > 0 && y + 1 < h {
                let fwd = luma[(y + 1) * w + x] - luma[y * w + x];
                let bwd = luma[y * w + x] - luma[(y - 1) * w + x];
                fwd.abs() > MIN_DIFF && bwd.abs() > MIN_DIFF && fwd * bwd < 0.0
            } else {
                false
            };

            if oscillates_x || oscillates_y {
                ringing_count += 1;
            }
        }
    }

    ringing_count as f32 / (w * h) as f32
}

// ---------------------------------------------------------------------------
// Edge retention (reference-aware)
// ---------------------------------------------------------------------------

/// Per-pixel mean squared Sobel gradient magnitude.
///
/// Using squared magnitudes (energy) instead of raw magnitudes makes the metric
/// less sensitive to small noise contributions.  The per-pixel normalization
/// (`/ w*h`) makes it scale-independent across different image dimensions.
fn sobel_energy_mean(luma: &[f32], w: usize, h: usize) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }

    let clamp_x = |x: isize| -> usize { (x.max(0) as usize).min(w - 1) };
    let clamp_y = |y: isize| -> usize { (y.max(0) as usize).min(h - 1) };
    let px = |xi: isize, yi: isize| -> f32 { luma[clamp_y(yi) * w + clamp_x(xi)] };

    let mut sum_sq = 0.0f64;
    for y in 0..h {
        for x in 0..w {
            let xi = x as isize;
            let yi = y as isize;
            let gx = (-px(xi - 1, yi - 1) + px(xi + 1, yi - 1)
                - 2.0 * px(xi - 1, yi)
                + 2.0 * px(xi + 1, yi)
                - px(xi - 1, yi + 1)
                + px(xi + 1, yi + 1)) as f64;
            let gy = (-px(xi - 1, yi - 1) - 2.0 * px(xi, yi - 1) - px(xi + 1, yi - 1)
                + px(xi - 1, yi + 1)
                + 2.0 * px(xi, yi + 1)
                + px(xi + 1, yi + 1)) as f64;
            sum_sq += gx * gx + gy * gy;
        }
    }

    (sum_sq / (w * h) as f64) as f32
}

// ---------------------------------------------------------------------------
// Texture retention (reference-aware)
// ---------------------------------------------------------------------------

/// Mean per-pixel local luminance variance using a 5×5 window (edge-replicate).
fn local_variance_mean(luma: &[f32], w: usize, h: usize) -> f32 {
    if w == 0 || h == 0 {
        return 0.0;
    }

    const HALF: isize = 2;
    const COUNT: f32 = 25.0;

    let mut total_var = 0.0f64;
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0f32;
            let mut sum_sq = 0.0f32;
            for dy in -HALF..=HALF {
                let yy = ((y as isize + dy).max(0) as usize).min(h - 1);
                for dx in -HALF..=HALF {
                    let xx = ((x as isize + dx).max(0) as usize).min(w - 1);
                    let v = luma[yy * w + xx];
                    sum += v;
                    sum_sq += v * v;
                }
            }
            let mean = sum / COUNT;
            total_var += (sum_sq / COUNT - mean * mean).max(0.0) as f64;
        }
    }

    (total_var / (w * h) as f64) as f32
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Score the quality of a downscaled image relative to its source.
///
/// Computes [`BaseResizeQuality`] containing edge/texture retention (diagnostic),
/// ringing score (active), and the derived [`envelope_scale`][BaseResizeQuality::envelope_scale]
/// multiplier.
///
/// Both arguments should be in linear RGB space.  This function is O(W×H) for
/// both images.
pub fn score_base_resize(source: &LinearRgbImage, resized: &LinearRgbImage) -> BaseResizeQuality {
    let src_luma = extract_luma(source);
    let res_luma = extract_luma(resized);

    let sw = source.width() as usize;
    let sh = source.height() as usize;
    let rw = resized.width() as usize;
    let rh = resized.height() as usize;

    // Edge retention: ratio of per-pixel Sobel energy (diagnostic only in v1).
    let src_edge = sobel_energy_mean(&src_luma, sw, sh);
    let res_edge = sobel_energy_mean(&res_luma, rw, rh);
    let edge_retention = if src_edge > 0.0 {
        (res_edge / src_edge).min(1.0)
    } else {
        1.0
    };

    // Texture retention: ratio of mean local variance (diagnostic only in v1).
    let src_tex = local_variance_mean(&src_luma, sw, sh);
    let res_tex = local_variance_mean(&res_luma, rw, rh);
    let texture_retention = if src_tex > 0.0 {
        (res_tex / src_tex).min(1.0)
    } else {
        1.0
    };

    // Ringing score: no-reference oscillation detector on the resized image.
    let ringing_score = compute_ringing_score(&res_luma, rw, rh);

    // Envelope scale: only ringing_score drives the budget in v1.
    let envelope_scale = (1.0 - RINGING_K * ringing_score).clamp(MIN_ENVELOPE_SCALE, 1.0);

    BaseResizeQuality { edge_retention, texture_retention, ringing_score, envelope_scale }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a LinearRgbImage from a repeated row pattern (grayscale: r=g=b=v).
    fn make_gray_image(row_values: &[f32], h: usize) -> LinearRgbImage {
        let w = row_values.len();
        let mut data = Vec::with_capacity(w * h * 3);
        for _ in 0..h {
            for &v in row_values {
                data.extend_from_slice(&[v, v, v]);
            }
        }
        LinearRgbImage::new(w as u32, h as u32, data).unwrap()
    }

    #[test]
    fn ringing_detected_in_oscillating_signal() {
        // Simulate Lanczos undershoot/overshoot near a step edge.
        // Row pattern: flat-dark → undershoot → transition → overshoot → flat-bright
        let row = [0.0f32, 0.0, 0.0, 0.0, -0.08, 0.5, 1.08, 1.0, 1.0, 1.0, 1.0];
        let img = make_gray_image(&row, 4); // h=4 satisfies the h>=3 requirement
        let w = row.len();
        let h = 4;
        let luma: Vec<f32> = img
            .pixels()
            .chunks_exact(3)
            .map(|rgb| pixel_luma(rgb[0], rgb[1], rgb[2]))
            .collect();
        let score = compute_ringing_score(&luma, w, h);
        assert!(score > 0.0, "expected ringing detection, got score={score}");
    }

    #[test]
    fn ringing_zero_for_smooth_gradient() {
        // A linear ramp has no sign-flip oscillations.
        let row: Vec<f32> = (0..24).map(|i| i as f32 / 23.0).collect();
        let img = make_gray_image(&row, 4);
        let w = row.len();
        let h = 4;
        let luma: Vec<f32> = img
            .pixels()
            .chunks_exact(3)
            .map(|rgb| pixel_luma(rgb[0], rgb[1], rgb[2]))
            .collect();
        let score = compute_ringing_score(&luma, w, h);
        assert_eq!(score, 0.0, "expected zero ringing in smooth gradient");
    }

    #[test]
    fn envelope_scale_clamped_to_min() {
        // Pathological: ringing_score = 1.0 → envelope = clamp(1 - 2, 0.65, 1) = 0.65
        let bq = BaseResizeQuality {
            edge_retention: 1.0,
            texture_retention: 1.0,
            ringing_score: 1.0,
            envelope_scale: (1.0_f32 - RINGING_K * 1.0_f32).clamp(MIN_ENVELOPE_SCALE, 1.0),
        };
        assert!((bq.envelope_scale - MIN_ENVELOPE_SCALE).abs() < 1e-6);
    }

    #[test]
    fn score_base_resize_solid_image_no_ringing() {
        // Solid-color image should have zero ringing and envelope_scale == 1.0.
        let src = LinearRgbImage::new(32, 32, vec![0.5f32; 32 * 32 * 3]).unwrap();
        let res = LinearRgbImage::new(8, 8, vec![0.5f32; 8 * 8 * 3]).unwrap();
        let bq = score_base_resize(&src, &res);
        assert_eq!(bq.ringing_score, 0.0);
        assert!((bq.envelope_scale - 1.0).abs() < 1e-6);
        assert!(bq.edge_retention.is_finite());
        assert!(bq.texture_retention.is_finite());
    }
}
