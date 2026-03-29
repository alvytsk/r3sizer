//! Alternative color handling — chroma-guarded sharpening (v0.4 experimental).
//!
//! Branch D: extends the sharpening path with chroma monitoring and provides
//! alternative color spaces for artifact evaluation.
//!
//! Provenance: `EngineeringChoice`.

use crate::color::{extract_luminance, luminance_from_linear_srgb, reconstruct_rgb_from_lightness};
use crate::sharpen::{make_kernel, unsharp_mask_single_channel_with_kernel};
use crate::types::{ChromaGuardDiagnostics, EvaluationColorSpace, LinearRgbImage};
use crate::CoreError;

// ---------------------------------------------------------------------------
// Chroma-guarded sharpening
// ---------------------------------------------------------------------------

/// Sharpen luminance, then apply soft chroma clamping where chroma shift exceeds
/// the threshold.
///
/// Algorithm:
/// 1. Sharpen luminance via existing USM path.
/// 2. Reconstruct RGB via multiplicative k = L'/L.
/// 3. Per pixel: measure chroma shift |(R'-L')-(R-L)| etc.
///    If shift > `max_chroma_shift * original_chroma_mag`, soft-clamp by
///    interpolating toward the original chroma.
pub fn sharpen_with_chroma_guard(
    src: &LinearRgbImage,
    amount: f32,
    sigma: f32,
    max_chroma_shift: f32,
) -> Result<(LinearRgbImage, ChromaGuardDiagnostics), CoreError> {
    let luminance = extract_luminance(src);
    let kernel = make_kernel(sigma)?;
    let sharpened_luma = unsharp_mask_single_channel_with_kernel(
        &luminance,
        src.width() as usize,
        src.height() as usize,
        amount,
        &kernel,
    );

    // Reconstruct RGB from sharpened luminance
    let reconstructed = reconstruct_rgb_from_lightness(src, &sharpened_luma);

    // Apply chroma guard
    let src_data = src.pixels();
    let rec_data = reconstructed.pixels();
    let n_pixels = (src.width() as usize) * (src.height() as usize);
    let mut out_data = vec![0.0f32; n_pixels * 3];

    let mut clamped_count: u32 = 0;
    let mut total_shift = 0.0f64;
    let mut max_shift = 0.0f32;

    for i in 0..n_pixels {
        let idx = i * 3;
        let (r, g, b) = (src_data[idx], src_data[idx + 1], src_data[idx + 2]);
        let (r2, g2, b2) = (rec_data[idx], rec_data[idx + 1], rec_data[idx + 2]);

        let l_orig = luminance_from_linear_srgb(r, g, b);
        let l_new = luminance_from_linear_srgb(r2, g2, b2);

        // Original chroma = (channel - luminance)
        let cr_orig = r - l_orig;
        let cg_orig = g - l_orig;
        let cb_orig = b - l_orig;

        let cr_new = r2 - l_new;
        let cg_new = g2 - l_new;
        let cb_new = b2 - l_new;

        // Chroma shift magnitude
        let dr = cr_new - cr_orig;
        let dg = cg_new - cg_orig;
        let db = cb_new - cb_orig;
        let shift = (dr * dr + dg * dg + db * db).sqrt();

        // Original chroma magnitude
        let orig_mag = (cr_orig * cr_orig + cg_orig * cg_orig + cb_orig * cb_orig).sqrt();
        let threshold = max_chroma_shift * orig_mag.max(1e-6);

        total_shift += shift as f64;
        if shift > max_shift {
            max_shift = shift;
        }

        if shift > threshold {
            // Soft clamp: blend toward original chroma
            let blend = threshold / shift;
            let cr_clamped = cr_orig + blend * (cr_new - cr_orig);
            let cg_clamped = cg_orig + blend * (cg_new - cg_orig);
            let cb_clamped = cb_orig + blend * (cb_new - cb_orig);

            out_data[idx] = l_new + cr_clamped;
            out_data[idx + 1] = l_new + cg_clamped;
            out_data[idx + 2] = l_new + cb_clamped;
            clamped_count += 1;
        } else {
            out_data[idx] = r2;
            out_data[idx + 1] = g2;
            out_data[idx + 2] = b2;
        }
    }

    let image = LinearRgbImage::new(src.width(), src.height(), out_data)?;
    let diag = ChromaGuardDiagnostics {
        pixels_clamped_fraction: clamped_count as f32 / n_pixels.max(1) as f32,
        mean_chroma_shift: if n_pixels > 0 { (total_shift / n_pixels as f64) as f32 } else { 0.0 },
        max_chroma_shift: max_shift,
    };

    Ok((image, diag))
}

// ---------------------------------------------------------------------------
// Alternative evaluation color spaces
// ---------------------------------------------------------------------------

/// sRGB→XYZ matrix (D65 reference white). Row-major: X, Y, Z.
const SRGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];

/// D65 reference white (2° observer).
const D65: [f32; 3] = [0.95047, 1.0, 1.08883];

/// Simplified CIE Lab f(t) without the linear segment.
#[inline]
fn lab_f(t: f32) -> f32 {
    if t > 0.008856 {
        t.cbrt()
    } else {
        7.787 * t + 16.0 / 116.0
    }
}

/// Convert linear RGB to approximate CIE Lab (L*, a*, b*).
#[inline]
pub fn linear_rgb_to_lab_approx(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let x = SRGB_TO_XYZ[0][0] * r + SRGB_TO_XYZ[0][1] * g + SRGB_TO_XYZ[0][2] * b;
    let y = SRGB_TO_XYZ[1][0] * r + SRGB_TO_XYZ[1][1] * g + SRGB_TO_XYZ[1][2] * b;
    let z = SRGB_TO_XYZ[2][0] * r + SRGB_TO_XYZ[2][1] * g + SRGB_TO_XYZ[2][2] * b;

    let fx = lab_f(x / D65[0]);
    let fy = lab_f(y / D65[1]);
    let fz = lab_f(z / D65[2]);

    let l_star = 116.0 * fy - 16.0;
    let a_star = 500.0 * (fx - fy);
    let b_star = 200.0 * (fy - fz);

    (l_star, a_star, b_star)
}

/// Evaluate artifact ratio in the specified color space.
///
/// Returns the fraction of "out-of-range" values according to the color space:
/// - `Rgb`: standard channel clipping ratio (fraction of channels outside [0,1]).
/// - `LumaOnly`: fraction of luminance values outside [0,1].
/// - `LabApprox`: fraction of pixels with L* outside [0,100] or a*/b* beyond ±128.
pub fn evaluate_in_color_space(
    img: &LinearRgbImage,
    color_space: EvaluationColorSpace,
) -> f32 {
    match color_space {
        EvaluationColorSpace::Rgb => {
            crate::metrics::channel_clipping_ratio(img)
        }
        EvaluationColorSpace::LumaOnly => {
            let luma = extract_luminance(img);
            let out = luma.iter().filter(|&&v| !(0.0..=1.0).contains(&v)).count();
            out as f32 / luma.len().max(1) as f32
        }
        EvaluationColorSpace::LabApprox => {
            let data = img.pixels();
            let n_pixels = data.len() / 3;
            if n_pixels == 0 { return 0.0; }
            let mut out = 0u32;
            for pixel in data.chunks_exact(3) {
                let (l, a, b) = linear_rgb_to_lab_approx(pixel[0], pixel[1], pixel[2]);
                if !(0.0..=100.0).contains(&l) || !(-128.0..=128.0).contains(&a) || !(-128.0..=128.0).contains(&b) {
                    out += 1;
                }
            }
            out as f32 / n_pixels as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(w: u32, h: u32, data: Vec<f32>) -> LinearRgbImage {
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn lab_white_is_100_0_0() {
        let (l, a, b) = linear_rgb_to_lab_approx(1.0, 1.0, 1.0);
        assert!((l - 100.0).abs() < 1.0, "L*={l}");
        assert!(a.abs() < 1.0, "a*={a}");
        assert!(b.abs() < 2.0, "b*={b}"); // slight numerical drift expected
    }

    #[test]
    fn lab_black_is_0_0_0() {
        let (l, a, b) = linear_rgb_to_lab_approx(0.0, 0.0, 0.0);
        assert!(l.abs() < 1.0, "L*={l}");
        assert!(a.abs() < 1.0, "a*={a}");
        assert!(b.abs() < 1.0, "b*={b}");
    }

    #[test]
    fn chroma_guard_no_sharpening_is_near_identity() {
        let img = make_image(4, 4, vec![0.5; 48]);
        let (out, diag) = sharpen_with_chroma_guard(&img, 0.0, 1.0, 0.1).unwrap();
        for (&a, &b) in img.pixels().iter().zip(out.pixels().iter()) {
            assert!((a - b).abs() < 1e-5);
        }
        assert!((diag.pixels_clamped_fraction).abs() < 1e-6);
    }

    #[test]
    fn chroma_guard_diagnostics_finite() {
        let mut data = vec![0.5f32; 4 * 4 * 3];
        // Add a high-contrast edge
        for i in 0..4 {
            let idx = i * 3;
            data[idx] = 1.0;
            data[idx + 1] = 0.0;
            data[idx + 2] = 0.0;
        }
        let img = make_image(4, 4, data);
        let (_, diag) = sharpen_with_chroma_guard(&img, 1.0, 1.0, 0.1).unwrap();
        assert!(diag.pixels_clamped_fraction.is_finite());
        assert!(diag.mean_chroma_shift.is_finite());
        assert!(diag.max_chroma_shift.is_finite());
    }

    #[test]
    fn evaluate_rgb_matches_metrics() {
        let img = make_image(2, 1, vec![0.5, 1.5, 0.1, -0.1, 0.8, 0.7]);
        let ratio = evaluate_in_color_space(&img, EvaluationColorSpace::Rgb);
        let expected = crate::metrics::channel_clipping_ratio(&img);
        assert!((ratio - expected).abs() < 1e-6);
    }

    #[test]
    fn evaluate_luma_only_valid_range() {
        let img = make_image(2, 1, vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
        let ratio = evaluate_in_color_space(&img, EvaluationColorSpace::LumaOnly);
        assert!((ratio).abs() < 1e-6);
    }

    #[test]
    fn evaluate_lab_approx_valid_range() {
        let img = make_image(2, 1, vec![0.5, 0.5, 0.5, 0.3, 0.4, 0.2]);
        let ratio = evaluate_in_color_space(&img, EvaluationColorSpace::LabApprox);
        assert!((ratio).abs() < 1e-6); // all valid in-gamut pixels
    }
}
