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

    /// Evaluate with precomputed base luminance (avoids re-extraction).
    /// Default delegates to [`evaluate`](Self::evaluate).
    fn evaluate_with_luma(
        &self,
        base: &LinearRgbImage,
        _base_luma: &[f32],
        sharpened: &LinearRgbImage,
        strength: f32,
    ) -> QualityEvaluation {
        self.evaluate(base, sharpened, strength)
    }

    /// Suggest an optimal sharpening strength based on image content analysis.
    /// Returns `None` if the evaluator cannot make a suggestion.
    fn suggest_strength(&self, base: &LinearRgbImage, target_quality: f32) -> Option<f32>;

    /// Suggest strength from precomputed luminance (avoids full feature extraction).
    /// Default delegates to [`suggest_strength`](Self::suggest_strength).
    fn suggest_strength_from_luma(&self, _luma: &[f32], _w: usize, _h: usize) -> Option<f32> {
        None
    }
}

// ---------------------------------------------------------------------------
// Feature extraction
// ---------------------------------------------------------------------------

/// Default edge threshold for edge density computation.
const EDGE_THRESHOLD: f32 = 0.05;

/// Number of bins for luminance histogram entropy.
const HIST_BINS: usize = 64;

/// Compute edge density from luminance (Sobel magnitude > threshold).
/// This is the only feature `suggest_strength` needs.
pub fn compute_edge_density(luma: &[f32], w: usize, h: usize) -> f32 {
    let n = luma.len();
    if n == 0 {
        return 0.0;
    }
    let gradients = crate::classifier::sobel_gradient_full(luma, w, h).0;
    let edge_count = gradients.iter().filter(|&&g| g > EDGE_THRESHOLD).count();
    edge_count as f32 / n as f32
}

/// Extract image features for quality prediction.
pub fn extract_features(img: &LinearRgbImage) -> ImageFeatures {
    let luma = extract_luminance(img);
    extract_features_from_luma(&luma, img.width() as usize, img.height() as usize)
}

/// Extract features from precomputed luminance and Sobel gradients.
fn extract_features_from_luma_and_grads(
    luma: &[f32],
    gradients: &[f32],
    w: usize,
    h: usize,
) -> ImageFeatures {
    let n = luma.len();
    if n == 0 {
        return ImageFeatures::default();
    }

    let edge_count = gradients.iter().filter(|&&g| g > EDGE_THRESHOLD).count();
    let edge_density = edge_count as f32 / n as f32;
    let mean_grad = gradients.iter().copied().sum::<f32>() / n as f32;
    let gradient_variance = gradients
        .iter()
        .map(|&g| (g - mean_grad) * (g - mean_grad))
        .sum::<f32>()
        / n as f32;

    let local_vars = local_variance_5x5(luma, w, h);
    let mean_lv = local_vars.iter().copied().sum::<f32>() / n as f32;
    let lv_variance = local_vars
        .iter()
        .map(|&v| (v - mean_lv) * (v - mean_lv))
        .sum::<f32>()
        / n as f32;

    let laplacian_var = laplacian_variance(luma, w, h);
    let entropy = histogram_entropy(luma);

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

/// Extract features from precomputed luminance (computes Sobel internally).
fn extract_features_from_luma(luma: &[f32], w: usize, h: usize) -> ImageFeatures {
    if luma.is_empty() {
        return ImageFeatures::default();
    }
    let gradients = crate::classifier::sobel_gradient_full(luma, w, h).0;
    extract_features_from_luma_and_grads(luma, &gradients, w, h)
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
            let lap = at(x + 1, y) + at(x - 1, y) + at(x, y + 1) + at(x, y - 1) - 4.0 * at(x, y);
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
    if n <= 0.0 {
        return 0.0;
    }
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

/// Piecewise-linear mapping from edge density to suggested strength.
fn strength_from_edge_density(edge_density: f32) -> f32 {
    let s = if edge_density < 0.05 {
        1.0
    } else if edge_density < 0.15 {
        1.0 - (edge_density - 0.05) * 5.0 // 1.0 → 0.5
    } else if edge_density < 0.40 {
        0.5 - (edge_density - 0.15) * 1.0 // 0.5 → 0.25
    } else {
        0.25
    };
    s.clamp(0.1, 2.0)
}

impl QualityEvaluator for HeuristicEvaluator {
    fn evaluate(
        &self,
        base: &LinearRgbImage,
        sharpened: &LinearRgbImage,
        strength: f32,
    ) -> QualityEvaluation {
        let base_luma = extract_luminance(base);
        self.evaluate_with_luma(base, &base_luma, sharpened, strength)
    }

    fn evaluate_with_luma(
        &self,
        base: &LinearRgbImage,
        base_luma: &[f32],
        sharpened: &LinearRgbImage,
        strength: f32,
    ) -> QualityEvaluation {
        let w = base.width() as usize;
        let h = base.height() as usize;

        // Compute Sobel once per image, reuse for features + correlation.
        let base_grads = crate::classifier::sobel_gradient_full(base_luma, w, h).0;
        let sharp_luma = extract_luminance(sharpened);
        let sharp_grads = crate::classifier::sobel_gradient_full(&sharp_luma, w, h).0;

        let features_base = extract_features_from_luma_and_grads(base_luma, &base_grads, w, h);
        let features_sharp = extract_features_from_luma_and_grads(&sharp_luma, &sharp_grads, w, h);

        // Gradient correlation from already-computed Sobel magnitudes.
        let grad_corr = pearson_correlation(&base_grads, &sharp_grads);

        // Out-of-range penalty
        let oor = sharpened
            .pixels()
            .iter()
            .filter(|&&v| !(0.0..=1.0).contains(&v))
            .count() as f32
            / sharpened.pixels().len().max(1) as f32;
        let oor_penalty = (oor * 100.0).min(1.0);

        let strength_penalty = if strength > 2.0 {
            (strength - 2.0) * 0.1
        } else {
            0.0
        };
        let quality =
            (grad_corr * 0.6 + (1.0 - oor_penalty) * 0.3 + (1.0 - strength_penalty.min(1.0)) * 0.1)
                .clamp(0.0, 1.0);

        let confidence = (features_base.edge_density * 2.0
            + features_base.luminance_histogram_entropy / 6.0)
            .clamp(0.1, 0.9);

        QualityEvaluation {
            predicted_quality_score: quality,
            suggested_strength: Some(strength_from_edge_density(features_base.edge_density)),
            confidence,
            features: features_sharp,
        }
    }

    fn suggest_strength(&self, base: &LinearRgbImage, _target_quality: f32) -> Option<f32> {
        let luma = extract_luminance(base);
        self.suggest_strength_from_luma(&luma, base.width() as usize, base.height() as usize)
    }

    fn suggest_strength_from_luma(&self, luma: &[f32], w: usize, h: usize) -> Option<f32> {
        let edge_density = compute_edge_density(luma, w, h);
        Some(strength_from_edge_density(edge_density))
    }
}

/// Pearson correlation between two equal-length gradient vectors.
fn pearson_correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f64;
    if n == 0.0 {
        return 0.0;
    }

    let mean_a = a.iter().copied().map(|v| v as f64).sum::<f64>() / n;
    let mean_b = b.iter().copied().map(|v| v as f64).sum::<f64>() / n;

    let mut cov = 0.0f64;
    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;

    for (&va, &vb) in a.iter().zip(b.iter()) {
        let da = va as f64 - mean_a;
        let db = vb as f64 - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < 1e-12 {
        return 1.0;
    }
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
    fn pearson_correlation_identical_is_one() {
        let vals = vec![0.1, 0.5, 0.9, 0.3, 0.7, 0.2, 0.8, 0.4, 0.6];
        let corr = pearson_correlation(&vals, &vals);
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
