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
// Luminance (CIE Y from linear sRGB)
// ---------------------------------------------------------------------------

/// Compute CIE Y luminance from linear sRGB components.
///
/// Coefficients are from the sRGB -> CIEXYZ conversion matrix (IEC 61966-2-1):
/// `L = 0.2126 R + 0.7152 G + 0.0722 B`
///
/// This is confirmed: the Y row of the sRGB-to-XYZ matrix.
#[inline]
pub fn luminance_from_linear_srgb(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Extract a single-channel luminance image from linear RGB.
///
/// Returns a `Vec<f32>` of length `width * height`.
pub fn extract_luminance(img: &LinearRgbImage) -> Vec<f32> {
    img.pixels()
        .chunks_exact(3)
        .map(|rgb| luminance_from_linear_srgb(rgb[0], rgb[1], rgb[2]))
        .collect()
}

/// Reconstruct RGB from original linear RGB and sharpened luminance.
///
/// Uses a multiplicative ratio: `k = L'/L; R' = k*R, G' = k*G, B' = k*B`.
/// For near-black pixels (`L < epsilon`), original RGB is preserved to avoid
/// division by zero.
///
/// **Engineering approximation** -- the reconstruction formula is a strong inference
/// from the paper, not a confirmed exact formula.
pub fn reconstruct_rgb_from_lightness(
    original: &LinearRgbImage,
    sharpened_luminance: &[f32],
) -> LinearRgbImage {
    const EPSILON: f32 = 1e-6;
    debug_assert_eq!(
        sharpened_luminance.len(),
        (original.width() as usize) * (original.height() as usize),
    );
    let out: Vec<f32> = original
        .pixels()
        .chunks_exact(3)
        .zip(sharpened_luminance.iter())
        .flat_map(|(rgb, &l_sharp)| {
            let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
            let l_orig = luminance_from_linear_srgb(r, g, b);
            if l_orig.abs() < EPSILON {
                [r, g, b]
            } else {
                let k = l_sharp / l_orig;
                [k * r, k * g, k * b]
            }
        })
        .collect();
    LinearRgbImage::new(original.width(), original.height(), out).unwrap()
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
    fn luminance_coefficients_sum_to_one() {
        assert_abs_diff_eq!(0.2126_f32 + 0.7152 + 0.0722, 1.0, epsilon = 1e-6);
    }

    #[test]
    fn achromatic_pixel_luminance_equals_component() {
        // For R = G = B = v, luminance should equal v.
        for &v in &[0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            assert_abs_diff_eq!(luminance_from_linear_srgb(v, v, v), v, epsilon = 1e-5);
        }
    }

    #[test]
    fn luminance_known_values() {
        // Pure red
        assert_abs_diff_eq!(luminance_from_linear_srgb(1.0, 0.0, 0.0), 0.2126, epsilon = 1e-4);
        // Pure green
        assert_abs_diff_eq!(luminance_from_linear_srgb(0.0, 1.0, 0.0), 0.7152, epsilon = 1e-4);
        // Pure blue
        assert_abs_diff_eq!(luminance_from_linear_srgb(0.0, 0.0, 1.0), 0.0722, epsilon = 1e-4);
    }

    #[test]
    fn extract_and_reconstruct_achromatic_roundtrip() {
        // For achromatic pixels, reconstruction should be near-identity.
        let data = vec![0.5f32; 4 * 4 * 3]; // 4x4 mid-gray
        let img = LinearRgbImage::new(4, 4, data).unwrap();
        let lum = extract_luminance(&img);
        let reconstructed = reconstruct_rgb_from_lightness(&img, &lum);
        for (a, b) in img.pixels().iter().zip(reconstructed.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn reconstruct_preserves_black_pixels() {
        // Black pixels (L~0) should be passed through, not produce NaN/Inf.
        let data = vec![0.0f32; 2 * 2 * 3];
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let sharpened_l = vec![0.1f32; 4]; // L' > 0, but original L = 0
        let out = reconstruct_rgb_from_lightness(&img, &sharpened_l);
        for &v in out.pixels() {
            assert!(v.is_finite(), "non-finite pixel in reconstructed black image");
            assert_abs_diff_eq!(v, 0.0, epsilon = 1e-6);
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
