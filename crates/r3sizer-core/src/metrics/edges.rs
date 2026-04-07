//! Shared cross-edge profile infrastructure for halo and overshoot metrics.
//!
//! Computes Sobel gradient on original luminance, selects edge pixels,
//! and samples a 1D profile along the gradient direction in the diff image.

/// Default gradient magnitude threshold for edge detection.
pub const DEFAULT_EDGE_THRESHOLD: f32 = 0.05;

/// Epsilon floor for gradient magnitude to avoid division by zero.
const GRADIENT_EPSILON: f32 = 1e-6;

/// A cross-edge profile sampled along the local gradient direction.
#[derive(Debug, Clone)]
pub struct EdgeProfile {
    /// Gradient magnitude at this edge pixel (local edge-strength proxy).
    pub gradient_magnitude: f32,
    /// 5 diff samples along the gradient direction, centered on the edge pixel.
    pub diff_samples: [f32; 5],
}

/// Extract edge profiles from original and sharpened luminance images.
///
/// Returns an empty Vec if no edge pixels exceed `edge_threshold`.
pub fn extract_edge_profiles(
    luma_original: &[f32],
    luma_sharpened: &[f32],
    width: usize,
    height: usize,
    edge_threshold: f32,
) -> Vec<EdgeProfile> {
    if width < 3 || height < 3 {
        return Vec::new();
    }

    // Use the shared optimized Sobel from classifier (split border/interior loops).
    let (grad_mag, grad_dx, grad_dy) = crate::classifier::sobel_gradient_full(luma_original, width, height);

    let mut profiles = Vec::new();

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let idx = y * width + x;
            let mag = grad_mag[idx];
            if mag < edge_threshold.max(GRADIENT_EPSILON) {
                continue;
            }

            // Normalized gradient direction (points across the edge).
            let nx = grad_dx[idx] / mag;
            let ny = grad_dy[idx] / mag;

            // Sample 5 points along gradient direction: offsets -2, -1, 0, +1, +2.
            let mut diff_samples = [0.0_f32; 5];
            for (i, offset) in [-2.0_f32, -1.0, 0.0, 1.0, 2.0].iter().enumerate() {
                let sx = x as f32 + offset * nx;
                let sy = y as f32 + offset * ny;
                let orig = bilinear_sample(luma_original, width, height, sx, sy);
                let sharp = bilinear_sample(luma_sharpened, width, height, sx, sy);
                diff_samples[i] = sharp - orig;
            }

            profiles.push(EdgeProfile {
                gradient_magnitude: mag,
                diff_samples,
            });
        }
    }

    profiles
}

/// Bilinear interpolation on a single-channel image.
fn bilinear_sample(
    data: &[f32],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
) -> f32 {
    let x0 = (x.floor() as isize).clamp(0, width as isize - 1) as usize;
    let y0 = (y.floor() as isize).clamp(0, height as isize - 1) as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);

    let fx = (x - x0 as f32).clamp(0.0, 1.0);
    let fy = (y - y0 as f32).clamp(0.0, 1.0);

    let v00 = data[y0 * width + x0];
    let v10 = data[y0 * width + x1];
    let v01 = data[y1 * width + x0];
    let v11 = data[y1 * width + x1];

    v00 * (1.0 - fx) * (1.0 - fy)
        + v10 * fx * (1.0 - fy)
        + v01 * (1.0 - fx) * fy
        + v11 * fx * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_produce_no_edge_profiles() {
        let luma = vec![0.5_f32; 8 * 8];
        let profiles = extract_edge_profiles(&luma, &luma, 8, 8, DEFAULT_EDGE_THRESHOLD);
        assert!(profiles.is_empty());
    }

    #[test]
    fn vertical_edge_produces_profiles() {
        let mut luma = vec![0.0_f32; 8 * 8];
        for y in 0..8 {
            for x in 4..8 {
                luma[y * 8 + x] = 1.0;
            }
        }
        let mut sharpened = luma.clone();
        for y in 0..8 {
            sharpened[y * 8 + 4] = 1.3;
            sharpened[y * 8 + 3] = -0.1;
        }
        let profiles = extract_edge_profiles(&luma, &sharpened, 8, 8, DEFAULT_EDGE_THRESHOLD);
        assert!(!profiles.is_empty(), "should detect edge profiles");
        for p in &profiles {
            assert!(p.gradient_magnitude > 0.0);
        }
    }

    #[test]
    fn image_too_small_returns_empty() {
        let luma = vec![0.5_f32; 2 * 2];
        let profiles = extract_edge_profiles(&luma, &luma, 2, 2, DEFAULT_EDGE_THRESHOLD);
        assert!(profiles.is_empty());
    }

    #[test]
    fn bilinear_sample_integer_coords() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        assert!((bilinear_sample(&data, 2, 2, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 0.0, 1.0) - 3.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 1.0, 1.0) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn bilinear_sample_midpoint() {
        let data = vec![0.0, 1.0, 0.0, 1.0];
        let mid = bilinear_sample(&data, 2, 2, 0.5, 0.5);
        assert!((mid - 0.5).abs() < 1e-6);
    }

    #[test]
    fn sobel_detects_horizontal_edge() {
        let mut luma = vec![0.0_f32; 5 * 5];
        for y in 3..5 {
            for x in 0..5 {
                luma[y * 5 + x] = 1.0;
            }
        }
        let (mag, _dx, _dy) = crate::classifier::sobel_gradient_full(&luma, 5, 5);
        let edge_mag = mag[2 * 5 + 2];
        assert!(edge_mag > 0.1, "edge magnitude should be significant: {edge_mag}");
    }
}
