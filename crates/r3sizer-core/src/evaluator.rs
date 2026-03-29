//! Learned evaluator — heuristic quality prediction (v0.4 experimental).
//!
//! Branch A: defines a `QualityEvaluator` trait and a hand-crafted
//! `HeuristicEvaluator` implementation. The evaluator is purely
//! diagnostic — it does not alter the pipeline's s* selection.
//!
//! The trait is designed for future extensibility: an ONNX-based
//! implementation could be plugged in without changing the pipeline.

use crate::color::extract_luminance;
use crate::types::{ImageFeatures, LinearRgbImage, QualityEvaluation};

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Quality evaluator interface.
///
/// Designed to be object-safe for dynamic dispatch, but the pipeline
/// currently uses static dispatch via `HeuristicEvaluator`.
pub trait QualityEvaluator: Send + Sync {
    /// Evaluate the quality of a sharpened image relative to the downscaled base.
    fn evaluate(
        &self,
        base: &LinearRgbImage,
        sharpened: &LinearRgbImage,
        strength: f32,
    ) -> QualityEvaluation;

    /// Suggest an optimal sharpening strength based on image content analysis.
    /// Returns `None` if the evaluator cannot make a suggestion.
    fn suggest_strength(
        &self,
        base: &LinearRgbImage,
        target_quality: f32,
    ) -> Option<f32>;
}

// ---------------------------------------------------------------------------
// Feature extraction
// ---------------------------------------------------------------------------

/// Default edge threshold for edge density computation.
const EDGE_THRESHOLD: f32 = 0.05;

/// Number of bins for luminance histogram entropy.
const HIST_BINS: usize = 64;

/// Extract image features for quality prediction.
pub fn extract_features(img: &LinearRgbImage) -> ImageFeatures {
    let luma = extract_luminance(img);
    let w = img.width() as usize;
    let h = img.height() as usize;
    let n = luma.len();

    if n == 0 {
        return ImageFeatures {
            edge_density: 0.0,
            mean_gradient_magnitude: 0.0,
            gradient_variance: 0.0,
            mean_local_variance: 0.0,
            local_variance_variance: 0.0,
            laplacian_variance: 0.0,
            luminance_histogram_entropy: 0.0,
        };
    }

    // --- Sobel gradient ---
    let gradients = sobel_magnitude(&luma, w, h);
    let edge_count = gradients.iter().filter(|&&g| g > EDGE_THRESHOLD).count();
    let edge_density = edge_count as f32 / n as f32;
    let mean_grad = gradients.iter().copied().sum::<f32>() / n as f32;
    let gradient_variance = gradients.iter()
        .map(|&g| (g - mean_grad) * (g - mean_grad))
        .sum::<f32>() / n as f32;

    // --- Local variance (5×5) ---
    let local_vars = local_variance_5x5(&luma, w, h);
    let mean_lv = local_vars.iter().copied().sum::<f32>() / n as f32;
    let lv_variance = local_vars.iter()
        .map(|&v| (v - mean_lv) * (v - mean_lv))
        .sum::<f32>() / n as f32;

    // --- Laplacian variance (frequency proxy) ---
    let laplacian_var = laplacian_variance(&luma, w, h);

    // --- Luminance histogram entropy ---
    let entropy = histogram_entropy(&luma);

    ImageFeatures {
        edge_density,
        mean_gradient_magnitude: mean_grad,
        gradient_variance,
        mean_local_variance: mean_lv,
        local_variance_variance: lv_variance,
        laplacian_variance: laplacian_var,
        luminance_histogram_entropy: entropy,
    }
}

/// Sobel gradient magnitude (simplified, edge-replicate).
fn sobel_magnitude(luma: &[f32], w: usize, h: usize) -> Vec<f32> {
    let at = |x: isize, y: isize| -> f32 {
        let cx = x.clamp(0, w as isize - 1) as usize;
        let cy = y.clamp(0, h as isize - 1) as usize;
        luma[cy * w + cx]
    };

    let mut mag = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let ix = x as isize;
            let iy = y as isize;
            let gx = -at(ix - 1, iy - 1) + at(ix + 1, iy - 1)
                   - 2.0 * at(ix - 1, iy) + 2.0 * at(ix + 1, iy)
                   - at(ix - 1, iy + 1) + at(ix + 1, iy + 1);
            let gy = -at(ix - 1, iy - 1) - 2.0 * at(ix, iy - 1) - at(ix + 1, iy - 1)
                   + at(ix - 1, iy + 1) + 2.0 * at(ix, iy + 1) + at(ix + 1, iy + 1);
            mag[y * w + x] = (gx * gx + gy * gy).sqrt();
        }
    }
    mag
}

/// Local variance in a 5×5 window (edge-replicate).
fn local_variance_5x5(luma: &[f32], w: usize, h: usize) -> Vec<f32> {
    let half = 2isize;
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0f32;
            let mut sum_sq = 0.0f32;
            let mut count = 0u32;
            for dy in -half..=half {
                for dx in -half..=half {
                    let cx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
                    let cy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                    let v = luma[cy * w + cx];
                    sum += v;
                    sum_sq += v * v;
                    count += 1;
                }
            }
            let mean = sum / count as f32;
            out[y * w + x] = (sum_sq / count as f32 - mean * mean).max(0.0);
        }
    }
    out
}

/// Variance of the Laplacian response as a frequency-content proxy.
fn laplacian_variance(luma: &[f32], w: usize, h: usize) -> f32 {
    if w < 3 || h < 3 {
        return 0.0;
    }
    let at = |x: usize, y: usize| -> f32 { luma[y * w + x] };

    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0u64;

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = at(x + 1, y) + at(x - 1, y) + at(x, y + 1) + at(x, y - 1)
                    - 4.0 * at(x, y);
            sum += lap as f64;
            sum_sq += (lap * lap) as f64;
            count += 1;
        }
    }
    if count == 0 {
        return 0.0;
    }
    let mean = sum / count as f64;
    ((sum_sq / count as f64) - mean * mean).max(0.0) as f32
}

/// Shannon entropy of a 64-bin luminance histogram.
fn histogram_entropy(luma: &[f32]) -> f32 {
    let mut bins = [0u32; HIST_BINS];
    for &v in luma {
        let idx = ((v.clamp(0.0, 1.0) * (HIST_BINS - 1) as f32).round()) as usize;
        bins[idx.min(HIST_BINS - 1)] += 1;
    }
    let n = luma.len() as f32;
    if n <= 0.0 { return 0.0; }
    let mut entropy = 0.0f32;
    for &count in &bins {
        if count > 0 {
            let p = count as f32 / n;
            entropy -= p * p.log2();
        }
    }
    entropy
}

// ---------------------------------------------------------------------------
// Heuristic evaluator
// ---------------------------------------------------------------------------

/// Hand-crafted quality evaluator using gradient correlation and
/// feature-based heuristics.
pub struct HeuristicEvaluator;

impl QualityEvaluator for HeuristicEvaluator {
    fn evaluate(
        &self,
        base: &LinearRgbImage,
        sharpened: &LinearRgbImage,
        strength: f32,
    ) -> QualityEvaluation {
        let features_base = extract_features(base);
        let features_sharp = extract_features(sharpened);

        // Gradient correlation between base and sharpened
        let base_luma = extract_luminance(base);
        let sharp_luma = extract_luminance(sharpened);
        let grad_corr = gradient_correlation(&base_luma, &sharp_luma, base.width() as usize, base.height() as usize);

        // Out-of-range penalty
        let oor = sharpened.pixels().iter()
            .filter(|&&v| !(0.0..=1.0).contains(&v))
            .count() as f32 / sharpened.pixels().len().max(1) as f32;
        let oor_penalty = (oor * 100.0).min(1.0); // 1% oor → full penalty

        // Quality = weighted combination
        // High correlation → good detail preservation
        // Low oor → few artifacts
        // Moderate strength → preferred
        let strength_penalty = if strength > 2.0 { (strength - 2.0) * 0.1 } else { 0.0 };
        let quality = (grad_corr * 0.6 + (1.0 - oor_penalty) * 0.3 + (1.0 - strength_penalty.min(1.0)) * 0.1)
            .clamp(0.0, 1.0);

        // Confidence based on image complexity
        let confidence = (features_base.edge_density * 2.0 + features_base.luminance_histogram_entropy / 6.0)
            .clamp(0.1, 0.9);

        QualityEvaluation {
            predicted_quality_score: quality,
            suggested_strength: self.suggest_strength(base, 0.8),
            confidence,
            features: features_sharp,
        }
    }

    fn suggest_strength(
        &self,
        base: &LinearRgbImage,
        _target_quality: f32,
    ) -> Option<f32> {
        let features = extract_features(base);

        // Piecewise-linear mapping from edge density to suggested strength:
        // High edge density → lower strength (detail-rich images need less)
        // Low edge density → higher strength (smooth images need more)
        let s = if features.edge_density < 0.05 {
            // Very smooth → suggest moderate-high strength
            1.0
        } else if features.edge_density < 0.15 {
            // Moderate detail → linear interpolation
            1.0 - (features.edge_density - 0.05) * 5.0 // 1.0 → 0.5
        } else if features.edge_density < 0.40 {
            // High detail → lower strength
            0.5 - (features.edge_density - 0.15) * 1.0 // 0.5 → 0.25
        } else {
            // Very high detail → minimum
            0.25
        };

        Some(s.clamp(0.1, 2.0))
    }
}

/// Compute Pearson correlation between Sobel gradient magnitudes of two images.
fn gradient_correlation(
    luma_a: &[f32],
    luma_b: &[f32],
    w: usize,
    h: usize,
) -> f32 {
    let grads_a = sobel_magnitude(luma_a, w, h);
    let grads_b = sobel_magnitude(luma_b, w, h);

    let n = grads_a.len() as f64;
    if n == 0.0 { return 0.0; }

    let mean_a = grads_a.iter().copied().map(|v| v as f64).sum::<f64>() / n;
    let mean_b = grads_b.iter().copied().map(|v| v as f64).sum::<f64>() / n;

    let mut cov = 0.0f64;
    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;

    for (&a, &b) in grads_a.iter().zip(grads_b.iter()) {
        let da = a as f64 - mean_a;
        let db = b as f64 - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < 1e-12 { return 1.0; } // both constant → perfect correlation
    (cov / denom) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(w: u32, h: u32, data: Vec<f32>) -> LinearRgbImage {
        LinearRgbImage::new(w, h, data).unwrap()
    }

    fn solid_image(w: u32, h: u32, v: f32) -> LinearRgbImage {
        make_image(w, h, vec![v; (w * h * 3) as usize])
    }

    fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
        let mut data = vec![0.0f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = x as f32 / w.max(1) as f32;
                data[idx + 1] = y as f32 / h.max(1) as f32;
                data[idx + 2] = 0.5;
            }
        }
        make_image(w, h, data)
    }

    #[test]
    fn features_are_finite() {
        let img = gradient_image(16, 16);
        let f = extract_features(&img);
        assert!(f.edge_density.is_finite());
        assert!(f.mean_gradient_magnitude.is_finite());
        assert!(f.gradient_variance.is_finite());
        assert!(f.mean_local_variance.is_finite());
        assert!(f.local_variance_variance.is_finite());
        assert!(f.laplacian_variance.is_finite());
        assert!(f.luminance_histogram_entropy.is_finite());
    }

    #[test]
    fn features_differ_for_different_images() {
        let solid = extract_features(&solid_image(16, 16, 0.5));
        let gradient = extract_features(&gradient_image(16, 16));
        assert!(solid.edge_density < gradient.edge_density + 0.01);
        assert!(solid.luminance_histogram_entropy < gradient.luminance_histogram_entropy + 0.01);
    }

    #[test]
    fn evaluator_produces_valid_score() {
        let base = gradient_image(16, 16);
        let sharpened = gradient_image(16, 16); // Same image = no artifacts
        let eval = HeuristicEvaluator;
        let result = eval.evaluate(&base, &sharpened, 0.5);
        assert!(result.predicted_quality_score >= 0.0);
        assert!(result.predicted_quality_score <= 1.0);
        assert!(result.confidence >= 0.0);
        assert!(result.confidence <= 1.0);
    }

    #[test]
    fn suggest_strength_in_range() {
        let eval = HeuristicEvaluator;
        let img = gradient_image(16, 16);
        let s = eval.suggest_strength(&img, 0.8).unwrap();
        assert!(s >= 0.1);
        assert!(s <= 2.0);
    }

    #[test]
    fn gradient_correlation_identical_is_one() {
        let luma = vec![0.1, 0.5, 0.9, 0.3, 0.7, 0.2, 0.8, 0.4, 0.6];
        let corr = gradient_correlation(&luma, &luma, 3, 3);
        assert!((corr - 1.0).abs() < 1e-5, "corr={corr}");
    }

    #[test]
    fn histogram_entropy_uniform() {
        // All same value → entropy = 0
        let luma = vec![0.5; 100];
        let e = histogram_entropy(&luma);
        assert!(e.abs() < 1e-5, "entropy={e}");
    }

    #[test]
    fn histogram_entropy_spread() {
        // Spread values → higher entropy
        let luma: Vec<f32> = (0..64).map(|i| i as f32 / 63.0).collect();
        let e = histogram_entropy(&luma);
        assert!(e > 3.0, "entropy={e}"); // log2(64) = 6, uniform distribution
    }
}
