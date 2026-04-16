/// Contrast leveling post-process stage.
///
/// **Status:** The exact contrast-leveling algorithm used in the original
/// papers is not confirmed from available sources.  The pipeline architecture
/// includes this as a separate, disable-able stage so that:
///
/// 1. The overall pipeline order can be preserved when the paper formula is
///    eventually identified.
/// 2. The stage can be switched off entirely (the default) without affecting
///    any other module.
///
/// **Current implementation (placeholder):**
/// When `enabled = true`, a simple per-channel min-max stretch is applied using
/// the 1st and 99th percentile values to avoid clipping on outlier pixels.
/// This is documented as an engineering approximation and will be replaced once
/// the paper-exact method is known.
use crate::{CoreError, LinearRgbImage};

/// Parameters for the contrast leveling stage.
#[derive(Debug, Clone, Default)]
pub struct ContrastLevelingParams {
    /// When `false` (default), the stage is a zero-cost no-op.
    pub enabled: bool,
}

/// Apply contrast leveling to `img` in place.
///
/// When `params.enabled` is `false`, this function is a true no-op (returns
/// immediately without touching the pixel buffer).
///
/// **PLACEHOLDER:** When `enabled = true`, performs a per-channel 1st–99th
/// percentile stretch.  This is NOT the paper-exact formula; it is a
/// conservative stand-in that preserves the module interface for future
/// replacement.
pub fn apply_contrast_leveling(
    img: &mut LinearRgbImage,
    params: &ContrastLevelingParams,
) -> Result<(), CoreError> {
    if !params.enabled {
        return Ok(());
    }

    // --- Placeholder: per-channel percentile stretch ---
    // Operates channel-by-channel (R, G, B independently).
    let w = img.width() as usize;
    let h = img.height() as usize;
    let n = w * h;

    for ch in 0..3_usize {
        // Collect channel values.
        let mut values: Vec<f32> = (0..n).map(|px| img.pixels()[px * 3 + ch]).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let lo_idx = ((n as f32 * 0.01) as usize).min(n - 1);
        let hi_idx = ((n as f32 * 0.99) as usize).min(n - 1);
        let lo = values[lo_idx];
        let hi = values[hi_idx];
        let range = hi - lo;

        if range < 1e-6 {
            // Constant channel — nothing to stretch.
            continue;
        }

        for px in 0..n {
            let v = &mut img.pixels_mut()[px * 3 + ch];
            *v = (*v - lo) / range;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = x as f32 / (w - 1).max(1) as f32;
                let u = y as f32 / (h - 1).max(1) as f32;
                data.extend_from_slice(&[v, u, 0.5]);
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn disabled_is_noop() {
        let original = gradient_image(8, 8);
        let mut img = original.clone();
        apply_contrast_leveling(&mut img, &ContrastLevelingParams { enabled: false }).unwrap();
        // Pixel data must be byte-for-byte identical.
        assert_eq!(img.pixels(), original.pixels());
    }

    #[test]
    fn enabled_does_not_change_dimensions() {
        let mut img = gradient_image(8, 8);
        let (w, h) = (img.width(), img.height());
        apply_contrast_leveling(&mut img, &ContrastLevelingParams { enabled: true }).unwrap();
        assert_eq!(img.width(), w);
        assert_eq!(img.height(), h);
    }

    #[test]
    fn enabled_on_constant_channel_does_not_panic() {
        // All pixels are identical — the constant-channel guard should trigger.
        let data = vec![0.5f32; 8 * 8 * 3];
        let mut img = LinearRgbImage::new(8, 8, data).unwrap();
        apply_contrast_leveling(&mut img, &ContrastLevelingParams { enabled: true }).unwrap();
    }
}
