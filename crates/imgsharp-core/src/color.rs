/// sRGB ↔ linear RGB conversion.
///
/// Implements the IEC 61966-2-1 (sRGB standard) piecewise transfer function.
/// All image processing in this library operates on linear RGB values so that
/// pixel blending (downscaling, filtering) is physically correct.
use crate::types::LinearRgbImage;

// ---------------------------------------------------------------------------
// Per-component transfer functions
// ---------------------------------------------------------------------------

/// Convert a single sRGB-encoded component in [0, 1] to linear light.
///
/// Formula (IEC 61966-2-1):
/// ```text
/// linear = v / 12.92                        if v <= 0.04045
/// linear = ((v + 0.055) / 1.055)^2.4        otherwise
/// ```
#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055_f32).powf(2.4)
    }
}

/// Convert a single linear-light component in [0, 1] to sRGB-encoded.
///
/// Formula (IEC 61966-2-1):
/// ```text
/// srgb = v * 12.92                          if v <= 0.0031308
/// srgb = 1.055 * v^(1/2.4) - 0.055         otherwise
/// ```
///
/// Input is clamped to [0, 1] before encoding to avoid NaN from powf on
/// negative values. Clamping here is intentional: this function is called
/// only at the final output stage after the ClampPolicy has been applied.
#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

// ---------------------------------------------------------------------------
// Whole-image transforms (in-place)
// ---------------------------------------------------------------------------

/// Convert every component of `img` from sRGB to linear light, in place.
pub fn image_srgb_to_linear(img: &mut LinearRgbImage) {
    for v in img.pixels_mut() {
        *v = srgb_to_linear(*v);
    }
}

/// Convert every component of `img` from linear light to sRGB, in place.
///
/// See [`linear_to_srgb`] for clamping behaviour.
pub fn image_linear_to_srgb(img: &mut LinearRgbImage) {
    for v in img.pixels_mut() {
        *v = linear_to_srgb(*v);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn black_and_white_are_fixed_points() {
        assert_abs_diff_eq!(srgb_to_linear(0.0), 0.0, epsilon = 1e-7);
        assert_abs_diff_eq!(srgb_to_linear(1.0), 1.0, epsilon = 1e-7);
        assert_abs_diff_eq!(linear_to_srgb(0.0), 0.0, epsilon = 1e-7);
        assert_abs_diff_eq!(linear_to_srgb(1.0), 1.0, epsilon = 1e-7);
    }

    #[test]
    fn srgb_128_approx_linear_0216() {
        // sRGB 128/255 ≈ 0.50196; linear ≈ 0.2158
        let srgb = 128.0_f32 / 255.0;
        let lin = srgb_to_linear(srgb);
        assert!(lin > 0.214 && lin < 0.218, "got {lin}");
    }

    #[test]
    fn round_trip_various_values() {
        for &v in &[0.0_f32, 0.04045, 0.5, 1.0] {
            let roundtrip = linear_to_srgb(srgb_to_linear(v));
            assert_abs_diff_eq!(roundtrip, v, epsilon = 1e-5);
        }
    }

    #[test]
    fn piecewise_boundary_continuity() {
        // Both branches should agree very close to the boundary 0.04045.
        let below = srgb_to_linear(0.04044);
        let above = srgb_to_linear(0.04046);
        // Smooth — difference should be tiny (linear region slope ≈ 0.0772).
        assert!((above - below).abs() < 1e-4, "discontinuity at boundary: {below} vs {above}");
    }
}
