//! Texture flattening metric: detect changes in fine-scale local variance,
//! penalizing both flattening and over-enhancement.
//!
//! Measures local fine-scale energy changes, not guaranteed semantic texture.

/// Default variance threshold for classifying a pixel as textured.
pub const DEFAULT_TEXTURE_THRESHOLD: f32 = 0.001;

/// Window half-size for local variance computation (5x5 window).
const HALF_WIN: usize = 2;

/// Compute the texture flattening score.
///
/// Score = mean of `|log2(var_sharpened / var_original)|` over textured pixels.
/// Returns 0.0 if no textured pixels are found.
pub fn texture_flattening_score(
    luma_original: &[f32],
    luma_sharpened: &[f32],
    width: usize,
    height: usize,
    texture_threshold: f32,
) -> f32 {
    if width < 2 * HALF_WIN + 1 || height < 2 * HALF_WIN + 1 {
        return 0.0;
    }

    let var_orig = local_variance(luma_original, width, height);
    let var_sharp = local_variance(luma_sharpened, width, height);

    let inner_w = width - 2 * HALF_WIN;
    let inner_h = height - 2 * HALF_WIN;

    let mut total_log_ratio = 0.0_f64;
    let mut textured_count = 0u32;

    for iy in 0..inner_h {
        for ix in 0..inner_w {
            let idx = iy * inner_w + ix;
            let vo = var_orig[idx];
            if vo <= texture_threshold {
                continue;
            }
            let vs = var_sharp[idx];
            let ratio = if vs < 1e-12 { 1e-12 / vo as f64 } else { vs as f64 / vo as f64 };
            total_log_ratio += ratio.log2().abs();
            textured_count += 1;
        }
    }

    if textured_count == 0 {
        return 0.0;
    }

    (total_log_ratio / textured_count as f64) as f32
}

/// Compute local variance in 5x5 windows.
///
/// Returns a Vec of length `(width - 2*HALF_WIN) * (height - 2*HALF_WIN)`.
fn local_variance(data: &[f32], width: usize, height: usize) -> Vec<f32> {
    let inner_w = width - 2 * HALF_WIN;
    let inner_h = height - 2 * HALF_WIN;
    let win_size = (2 * HALF_WIN + 1) * (2 * HALF_WIN + 1);
    let inv_n = 1.0 / win_size as f32;
    let mut result = Vec::with_capacity(inner_w * inner_h);

    for cy in HALF_WIN..height - HALF_WIN {
        for cx in HALF_WIN..width - HALF_WIN {
            let mut sum = 0.0_f32;
            let mut sum_sq = 0.0_f32;
            for wy in (cy - HALF_WIN)..=(cy + HALF_WIN) {
                for wx in (cx - HALF_WIN)..=(cx + HALF_WIN) {
                    let v = data[wy * width + wx];
                    sum += v;
                    sum_sq += v * v;
                }
            }
            let mean = sum * inv_n;
            let variance = (sum_sq * inv_n - mean * mean).max(0.0);
            result.push(variance);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_score_zero() {
        let luma = vec![0.5_f32; 8 * 8];
        let score = texture_flattening_score(&luma, &luma, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn solid_image_no_textured_pixels() {
        let orig = vec![0.5_f32; 8 * 8];
        let sharp = vec![0.6_f32; 8 * 8];
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn image_too_small_returns_zero() {
        let luma = vec![0.5_f32; 4 * 4];
        let score = texture_flattening_score(&luma, &luma, 4, 4, DEFAULT_TEXTURE_THRESHOLD);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn doubled_variance_gives_positive_score() {
        let mut orig = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                orig[y * 8 + x] = if (x + y) % 2 == 0 { 0.6 } else { 0.4 };
            }
        }
        let mut sharp = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                sharp[y * 8 + x] = if (x + y) % 2 == 0 { 0.7 } else { 0.3 };
            }
        }
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert!(score > 0.5, "score should be roughly 1.0 for doubled variance: {score}");
    }

    #[test]
    fn flattening_gives_positive_score() {
        let mut orig = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                orig[y * 8 + x] = if (x + y) % 2 == 0 { 0.7 } else { 0.3 };
            }
        }
        let sharp = vec![0.5_f32; 8 * 8];
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert!(score > 0.0, "flattening should produce positive score: {score}");
    }

    #[test]
    fn score_is_finite() {
        let mut orig = vec![0.5_f32; 10 * 10];
        for i in 0..100 {
            orig[i] = (i as f32 / 100.0) * 0.8 + 0.1;
        }
        let sharp = orig.iter().map(|&v| v * 1.5).collect::<Vec<_>>();
        let score = texture_flattening_score(&orig, &sharp, 10, 10, DEFAULT_TEXTURE_THRESHOLD);
        assert!(score.is_finite(), "score must be finite: {score}");
    }
}
