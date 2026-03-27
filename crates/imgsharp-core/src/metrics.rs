/// Artifact ratio metric P(s).
///
/// After applying sharpening with strength `s`, some channel values may fall
/// outside the valid [0, 1] range.  This is the measurable indicator of
/// sharpening artifacts used by the automatic selection algorithm.
///
/// **Operational definition (current implementation):**
///
/// ```text
/// P(s) = (number of f32 channel values strictly outside [0, 1])
///        -------------------------------------------------------
///                  total number of channel values
/// ```
///
/// For an RGB image of width W and height H the denominator is W × H × 3.
///
/// **Note:** values exactly equal to 0.0 or 1.0 are *not* counted as artifacts.
///
/// **Possible future refinements:**
/// - count pixels rather than channels (any-channel criterion)
/// - evaluate in a perceptual or orthogonal colour space
/// - add a halo-penalty weighting
use crate::types::LinearRgbImage;

/// Compute the artifact ratio for `img`.
///
/// Returns a value in `[0, 1]`. Returns `0.0` for an empty image.
pub fn artifact_ratio(img: &LinearRgbImage) -> f32 {
    let pixels = img.pixels();
    let total = pixels.len();
    if total == 0 {
        return 0.0;
    }
    // Use integer accumulation instead of filter+count so LLVM can
    // auto-vectorize the comparison → mask → add pattern.
    let out_of_range: u32 = pixels
        .iter()
        .map(|&v| (!(0.0..=1.0).contains(&v)) as u32)
        .sum();
    out_of_range as f32 / total as f32
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LinearRgbImage;

    fn solid(width: u32, height: u32, value: f32) -> LinearRgbImage {
        let n = (width * height * 3) as usize;
        LinearRgbImage::new(width, height, vec![value; n]).unwrap()
    }

    #[test]
    fn all_zero_is_clean() {
        let img = solid(10, 10, 0.0);
        assert_eq!(artifact_ratio(&img), 0.0);
    }

    #[test]
    fn all_one_is_clean() {
        let img = solid(10, 10, 1.0);
        assert_eq!(artifact_ratio(&img), 0.0);
    }

    #[test]
    fn all_mid_is_clean() {
        let img = solid(10, 10, 0.5);
        assert_eq!(artifact_ratio(&img), 0.0);
    }

    #[test]
    fn one_out_of_range_component() {
        // 2×2 RGB image → 12 components total; make exactly 1 out-of-range.
        let mut data = vec![0.5_f32; 12];
        data[0] = -0.001; // strictly below 0
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 12.0;
        assert!((artifact_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn all_components_above_one() {
        let img = solid(4, 4, 1.001);
        assert_eq!(artifact_ratio(&img), 1.0);
    }

    #[test]
    fn all_components_below_zero() {
        let img = solid(4, 4, -0.5);
        assert_eq!(artifact_ratio(&img), 1.0);
    }

    #[test]
    fn boundary_values_not_counted() {
        // 0.0 and 1.0 exactly are valid.
        let mut data = vec![0.0_f32; 6];
        data[3] = 1.0;
        let img = LinearRgbImage::new(1, 2, data).unwrap();
        assert_eq!(artifact_ratio(&img), 0.0);
    }
}
