/// Parameterized sharpening via unsharp mask.
///
/// **Engineering note:** The exact sharpening operator used in the original
/// papers is not confirmed.  Values cited in available descriptions (e.g. 1.09,
/// 1.81, 2.17) are consistent with a linear multiplicative `amount` parameter,
/// which is precisely what unsharp mask provides.
///
/// Formula:
/// ```text
/// output[i] = input[i] + amount * (input[i] - blur(input, sigma)[i])
/// ```
///
/// **Important:** no clamping is applied.  Values outside [0, 1] are
/// intentional — the artifact-ratio metric depends on them being unclamped.
/// Clamping happens only at the final output stage in the pipeline.
use crate::{CoreError, LinearRgbImage};

// ---------------------------------------------------------------------------
// Gaussian kernel
// ---------------------------------------------------------------------------

/// Build a 1-D normalised Gaussian kernel of `radius = ceil(3σ)`.
fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil() as usize;
    let size = 2 * radius + 1;
    let mut kernel = Vec::with_capacity(size);
    let two_sigma_sq = 2.0 * sigma * sigma;
    for i in 0..size {
        let k = i as f32 - radius as f32;
        kernel.push((-k * k / two_sigma_sq).exp());
    }
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|v| *v /= sum);
    kernel
}

// ---------------------------------------------------------------------------
// Separable Gaussian blur
// ---------------------------------------------------------------------------

/// Apply a separable Gaussian blur to `src` with the given 1-D `kernel`.
/// Returns a new `LinearRgbImage`; `src` is not mutated.
pub(crate) fn gaussian_blur(src: &LinearRgbImage, kernel: &[f32]) -> LinearRgbImage {
    let w = src.width() as usize;
    let h = src.height() as usize;
    let radius = kernel.len() / 2;
    let stride = w * 3;

    let src_data = src.pixels();

    // --- Horizontal pass ---
    // Split into: left edge (clamp), interior (no bounds check), right edge (clamp).
    let mut horiz = vec![0.0f32; w * h * 3];
    for y in 0..h {
        let row_start = y * stride;
        let row = &src_data[row_start..row_start + stride];
        let out_row = &mut horiz[row_start..row_start + stride];

        // Left edge: x < radius
        for x in 0..radius.min(w) {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            for (ki, &kv) in kernel.iter().enumerate() {
                let xi = (x + ki).saturating_sub(radius).min(w - 1);
                r += row[xi * 3] * kv;
                g += row[xi * 3 + 1] * kv;
                b += row[xi * 3 + 2] * kv;
            }
            out_row[x * 3] = r;
            out_row[x * 3 + 1] = g;
            out_row[x * 3 + 2] = b;
        }

        // Interior: no bounds checks needed
        let x_start = radius.min(w);
        let x_end = w.saturating_sub(radius);
        for x in x_start..x_end {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let base = (x - radius) * 3;
            for (ki, &kv) in kernel.iter().enumerate() {
                let off = base + ki * 3;
                r += row[off] * kv;
                g += row[off + 1] * kv;
                b += row[off + 2] * kv;
            }
            out_row[x * 3] = r;
            out_row[x * 3 + 1] = g;
            out_row[x * 3 + 2] = b;
        }

        // Right edge: x >= w - radius
        for x in x_end.max(x_start)..w {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            for (ki, &kv) in kernel.iter().enumerate() {
                let xi = (x + ki).saturating_sub(radius).min(w - 1);
                r += row[xi * 3] * kv;
                g += row[xi * 3 + 1] * kv;
                b += row[xi * 3 + 2] * kv;
            }
            out_row[x * 3] = r;
            out_row[x * 3 + 1] = g;
            out_row[x * 3 + 2] = b;
        }
    }

    // --- Vertical pass ---
    // Process one row at a time, accumulating from kernel.len() row pointers.
    // This is cache-friendly: we read full rows sequentially.
    let mut vert = vec![0.0f32; w * h * 3];

    // Precompute clamped row indices for each (y, ki) to avoid per-pixel branching.
    // Top edge
    for y in 0..radius.min(h) {
        let out_start = y * stride;
        let out_row = &mut vert[out_start..out_start + stride];

        // First kernel tap: initialize
        let yi0 = y.saturating_sub(radius);
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * stride..yi0 * stride + stride];
        for i in 0..stride {
            out_row[i] = src_row0[i] * kv0;
        }
        // Remaining kernel taps: accumulate
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = (y + ki).saturating_sub(radius).min(h - 1);
            let src_row = &horiz[yi * stride..yi * stride + stride];
            for i in 0..stride {
                out_row[i] += src_row[i] * kv;
            }
        }
    }

    // Interior rows: no clamping needed
    let y_start = radius.min(h);
    let y_end = h.saturating_sub(radius);
    for y in y_start..y_end {
        let out_start = y * stride;
        let out_row = &mut vert[out_start..out_start + stride];

        let yi0 = y - radius;
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * stride..yi0 * stride + stride];
        for i in 0..stride {
            out_row[i] = src_row0[i] * kv0;
        }
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = yi0 + ki;
            let src_row = &horiz[yi * stride..yi * stride + stride];
            for i in 0..stride {
                out_row[i] += src_row[i] * kv;
            }
        }
    }

    // Bottom edge
    for y in y_end.max(y_start)..h {
        let out_start = y * stride;
        let out_row = &mut vert[out_start..out_start + stride];

        let yi0 = y.saturating_sub(radius);
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * stride..yi0 * stride + stride];
        for i in 0..stride {
            out_row[i] = src_row0[i] * kv0;
        }
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = (y + ki).saturating_sub(radius).min(h - 1);
            let src_row = &horiz[yi * stride..yi * stride + stride];
            for i in 0..stride {
                out_row[i] += src_row[i] * kv;
            }
        }
    }

    LinearRgbImage::new(src.width(), src.height(), vert).unwrap()
}

// ---------------------------------------------------------------------------
// Single-channel Gaussian blur (for lightness-based sharpening)
// ---------------------------------------------------------------------------

/// Apply a separable Gaussian blur to single-channel data.
#[allow(clippy::needless_range_loop)] // x index is used for kernel offset arithmetic
pub(crate) fn gaussian_blur_single_channel(
    data: &[f32],
    width: usize,
    height: usize,
    kernel: &[f32],
) -> Vec<f32> {
    let n = width * height;
    let mut horiz = vec![0.0f32; n];
    let mut vert = vec![0.0f32; n];
    gaussian_blur_single_channel_into(data, width, height, kernel, &mut horiz, &mut vert);
    vert
}

/// Like [`gaussian_blur_single_channel`] but writes into pre-allocated scratch
/// buffers, avoiding allocation when called repeatedly (e.g. probe loop).
///
/// `horiz` and `vert` must each have length >= `width * height`.
/// On return, `vert` contains the blurred result.
#[allow(clippy::needless_range_loop)]
pub(crate) fn gaussian_blur_single_channel_into(
    data: &[f32],
    width: usize,
    height: usize,
    kernel: &[f32],
    horiz: &mut [f32],
    vert: &mut [f32],
) {
    let radius = kernel.len() / 2;

    // --- Horizontal pass ---
    for y in 0..height {
        let row = &data[y * width..(y + 1) * width];
        let out_row = &mut horiz[y * width..(y + 1) * width];

        // Left edge
        for x in 0..radius.min(width) {
            let mut acc = 0.0f32;
            for (ki, &kv) in kernel.iter().enumerate() {
                let xi = (x + ki).saturating_sub(radius).min(width - 1);
                acc += row[xi] * kv;
            }
            out_row[x] = acc;
        }
        // Interior
        let x_start = radius.min(width);
        let x_end = width.saturating_sub(radius);
        for x in x_start..x_end {
            let mut acc = 0.0f32;
            let base = x - radius;
            for (ki, &kv) in kernel.iter().enumerate() {
                acc += row[base + ki] * kv;
            }
            out_row[x] = acc;
        }
        // Right edge
        for x in x_end.max(x_start)..width {
            let mut acc = 0.0f32;
            for (ki, &kv) in kernel.iter().enumerate() {
                let xi = (x + ki).saturating_sub(radius).min(width - 1);
                acc += row[xi] * kv;
            }
            out_row[x] = acc;
        }
    }

    // --- Vertical pass ---
    // Top edge
    for y in 0..radius.min(height) {
        let out_row = &mut vert[y * width..(y + 1) * width];
        let yi0 = y.saturating_sub(radius);
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * width..(yi0 + 1) * width];
        for i in 0..width {
            out_row[i] = src_row0[i] * kv0;
        }
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = (y + ki).saturating_sub(radius).min(height - 1);
            let src_row = &horiz[yi * width..(yi + 1) * width];
            for i in 0..width {
                out_row[i] += src_row[i] * kv;
            }
        }
    }
    // Interior
    let y_start = radius.min(height);
    let y_end = height.saturating_sub(radius);
    for y in y_start..y_end {
        let out_row = &mut vert[y * width..(y + 1) * width];
        let yi0 = y - radius;
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * width..(yi0 + 1) * width];
        for i in 0..width {
            out_row[i] = src_row0[i] * kv0;
        }
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = yi0 + ki;
            let src_row = &horiz[yi * width..(yi + 1) * width];
            for i in 0..width {
                out_row[i] += src_row[i] * kv;
            }
        }
    }
    // Bottom edge
    for y in y_end.max(y_start)..height {
        let out_row = &mut vert[y * width..(y + 1) * width];
        let yi0 = y.saturating_sub(radius);
        let &kv0 = &kernel[0];
        let src_row0 = &horiz[yi0 * width..(yi0 + 1) * width];
        for i in 0..width {
            out_row[i] = src_row0[i] * kv0;
        }
        for (ki, &kv) in kernel.iter().enumerate().skip(1) {
            let yi = (y + ki).saturating_sub(radius).min(height - 1);
            let src_row = &horiz[yi * width..(yi + 1) * width];
            for i in 0..width {
                out_row[i] += src_row[i] * kv;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build and return a normalised Gaussian kernel for the given sigma.
///
/// Exposed so that callers (e.g. the pipeline probe loop) can build the kernel
/// once and reuse it across multiple calls to [`unsharp_mask_with_kernel`] /
/// [`unsharp_mask_single_channel_with_kernel`].
pub fn make_kernel(sigma: f32) -> Result<Vec<f32>, CoreError> {
    if sigma <= 0.0 {
        return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
    }
    Ok(gaussian_kernel(sigma))
}

/// Apply unsharp-mask sharpening with the given `amount` and Gaussian `sigma`.
///
/// - `amount = 0.0` → identity (no change).
/// - `amount > 0.0` → sharpening; larger values produce stronger enhancement.
///
/// The result is **not clamped**. Out-of-range values are expected and used by
/// the artifact-ratio metric.
pub fn unsharp_mask(
    src: &LinearRgbImage,
    amount: f32,
    sigma: f32,
) -> Result<LinearRgbImage, CoreError> {
    let kernel = make_kernel(sigma)?;
    Ok(unsharp_mask_with_kernel(src, amount, &kernel))
}

/// Like [`unsharp_mask`] but accepts a pre-built kernel (avoids recomputation).
pub fn unsharp_mask_with_kernel(
    src: &LinearRgbImage,
    amount: f32,
    kernel: &[f32],
) -> LinearRgbImage {
    // Compute blur, then apply USM formula in-place on the blur buffer
    // to avoid a third allocation: out[i] = src[i] + amount * (src[i] - blur[i])
    let mut blurred = gaussian_blur(src, kernel);
    let src_px = src.pixels();
    let blur_px = blurred.pixels_mut();
    let amt_plus_1 = 1.0 + amount;
    for (b, &s) in blur_px.iter_mut().zip(src_px.iter()) {
        *b = amt_plus_1 * s - amount * *b;
    }
    blurred
}

/// Apply unsharp-mask sharpening to a single-channel buffer (e.g. luminance).
///
/// - `data` must have length `width * height`.
/// - `amount = 0.0` -> identity (no change).
///
/// The result is **not clamped**. Out-of-range values may be produced and are
/// expected when used for artifact measurement via RGB reconstruction.
pub fn unsharp_mask_single_channel(
    data: &[f32],
    width: usize,
    height: usize,
    amount: f32,
    sigma: f32,
) -> Result<Vec<f32>, CoreError> {
    let kernel = make_kernel(sigma)?;
    Ok(unsharp_mask_single_channel_with_kernel(data, width, height, amount, &kernel))
}

/// Like [`unsharp_mask_single_channel`] but accepts a pre-built kernel.
pub fn unsharp_mask_single_channel_with_kernel(
    data: &[f32],
    width: usize,
    height: usize,
    amount: f32,
    kernel: &[f32],
) -> Vec<f32> {
    debug_assert_eq!(data.len(), width * height);

    // Compute blur, then apply USM in-place to reuse the allocation.
    let mut blurred = gaussian_blur_single_channel(data, width, height, kernel);
    let amt_plus_1 = 1.0 + amount;
    for (b, &s) in blurred.iter_mut().zip(data.iter()) {
        *b = amt_plus_1 * s - amount * *b;
    }
    blurred
}

/// Like [`unsharp_mask_single_channel_with_kernel`] but uses pre-allocated
/// scratch buffers to avoid allocation.
///
/// `scratch_a` and `scratch_b` must each have length >= `width * height`.
/// Returns the sharpened data in `scratch_b` (borrows from the scratch).
#[allow(dead_code)]
pub(crate) fn unsharp_mask_single_channel_with_scratch<'a>(
    data: &[f32],
    width: usize,
    height: usize,
    amount: f32,
    kernel: &[f32],
    scratch_a: &mut [f32],
    scratch_b: &'a mut [f32],
) -> &'a [f32] {
    debug_assert_eq!(data.len(), width * height);
    let n = width * height;
    debug_assert!(scratch_a.len() >= n);
    debug_assert!(scratch_b.len() >= n);

    gaussian_blur_single_channel_into(data, width, height, kernel, scratch_a, scratch_b);
    let amt_plus_1 = 1.0 + amount;
    for (b, &s) in scratch_b[..n].iter_mut().zip(data.iter()) {
        *b = amt_plus_1 * s - amount * *b;
    }
    &scratch_b[..n]
}

// ---------------------------------------------------------------------------
// Adaptive sharpening (v0.3)
// ---------------------------------------------------------------------------

use crate::GainMap;

/// Adaptive unsharp mask on the lightness channel with per-pixel gain.
///
/// Computes blur once, then applies `L'(x,y) = L(x,y) + strength * gain(x,y) * D(x,y)`
/// where `D = L - blur(L)`. Reconstructs RGB via `k = L'/L`.
///
/// **No clamping.** Out-of-range values are the artifact signal.
pub fn adaptive_sharpen_lightness(
    src: &LinearRgbImage,
    luminance: &[f32],
    strength: f32,
    gain_map: &GainMap,
    sigma: f32,
) -> Result<LinearRgbImage, CoreError> {
    debug_assert_eq!(luminance.len(), (src.width() as usize) * (src.height() as usize));
    debug_assert_eq!(gain_map.width, src.width());
    debug_assert_eq!(gain_map.height, src.height());

    let kernel = make_kernel(sigma)?;
    let w = src.width() as usize;
    let h = src.height() as usize;

    let blurred = gaussian_blur_single_channel(luminance, w, h, &kernel);

    // Detail layer: D = L - blur(L)
    let detail: Vec<f32> = luminance.iter().zip(blurred.iter())
        .map(|(&l, &b)| l - b)
        .collect();

    let sharpened_l = apply_adaptive_lightness_from_detail(luminance, &detail, strength, gain_map);
    Ok(crate::color::reconstruct_rgb_from_lightness(src, &sharpened_l))
}

/// Apply adaptive sharpening from pre-computed detail buffer.
///
/// `L'(x,y) = L(x,y) + strength * gain(x,y) * detail(x,y)`
///
/// Used by the backoff loop to avoid recomputing the Gaussian blur.
pub fn apply_adaptive_lightness_from_detail(
    luminance: &[f32],
    detail: &[f32],
    strength: f32,
    gain_map: &GainMap,
) -> Vec<f32> {
    let gain_data = gain_map.data();
    luminance.iter().zip(detail.iter()).zip(gain_data.iter())
        .map(|((&l, &d), &g)| l + strength * g * d)
        .collect()
}

/// Adaptive unsharp mask on RGB channels with per-pixel gain.
///
/// Computes blur once per channel, then applies
/// `C'(x,y) = C(x,y) + strength * gain(x,y) * (C(x,y) - blur_C(x,y))`
///
/// **No clamping.**
pub fn adaptive_sharpen_rgb(
    src: &LinearRgbImage,
    strength: f32,
    gain_map: &GainMap,
    sigma: f32,
) -> Result<LinearRgbImage, CoreError> {
    debug_assert_eq!(gain_map.width, src.width());
    debug_assert_eq!(gain_map.height, src.height());

    let kernel = make_kernel(sigma)?;
    let blurred = gaussian_blur(src, &kernel);

    let src_px = src.pixels();
    let blur_px = blurred.pixels();
    let gain_data = gain_map.data();

    let out: Vec<f32> = src_px.chunks_exact(3)
        .zip(blur_px.chunks_exact(3))
        .zip(gain_data.iter())
        .flat_map(|((s, b), &g)| {
            let eff = strength * g;
            [
                s[0] + eff * (s[0] - b[0]),
                s[1] + eff * (s[1] - b[1]),
                s[2] + eff * (s[2] - b[2]),
            ]
        })
        .collect();

    LinearRgbImage::new(src.width(), src.height(), out)
}

// ---------------------------------------------------------------------------
// Precomputed-detail API for probe-loop optimisation
// ---------------------------------------------------------------------------
//
// The unsharp-mask formula is `out = input + s * (input - blur(input))`.
// The detail signal `D = input - blur(input)` is independent of `s`, so we
// compute it once and reuse it for every probe strength.  This collapses
// probe cost from  N × (blur + apply)  to  1 × blur + N × apply,  where
// the apply step is a trivial multiply-add.

/// Compute the single-channel detail signal: `D = data - blur(data)`.
///
/// The returned buffer has the same length as `data` (`width × height`).
pub fn compute_detail_single_channel(
    data: &[f32],
    width: usize,
    height: usize,
    kernel: &[f32],
) -> Vec<f32> {
    let blurred = gaussian_blur_single_channel(data, width, height, kernel);
    data.iter().zip(blurred.iter()).map(|(&d, &b)| d - b).collect()
}

/// Compute the RGB detail signal: `D = src - blur(src)`.
///
/// Returns a flat `Vec<f32>` with `width × height × 3` elements.
pub fn compute_detail_rgb(src: &LinearRgbImage, kernel: &[f32]) -> Vec<f32> {
    let blurred = gaussian_blur(src, kernel);
    src.pixels()
        .iter()
        .zip(blurred.pixels().iter())
        .map(|(&s, &b)| s - b)
        .collect()
}

/// Apply precomputed detail at a given strength: `out = input + amount * detail`.
///
/// Equivalent to [`unsharp_mask_single_channel_with_kernel`] but skips the
/// Gaussian blur (already factored into `detail`).
pub fn apply_detail_single_channel(input: &[f32], detail: &[f32], amount: f32) -> Vec<f32> {
    input
        .iter()
        .zip(detail.iter())
        .map(|(&i, &d)| i + amount * d)
        .collect()
}

/// Like [`apply_detail_single_channel`] but writes into a pre-allocated buffer.
///
/// `out` must have length >= `input.len()`.
pub fn apply_detail_single_channel_into(input: &[f32], detail: &[f32], amount: f32, out: &mut [f32]) {
    for ((o, &i), &d) in out.iter_mut().zip(input.iter()).zip(detail.iter()) {
        *o = i + amount * d;
    }
}

/// Apply precomputed RGB detail at a given strength: `out = src + amount * detail`.
///
/// Equivalent to [`unsharp_mask_with_kernel`] but skips the Gaussian blur.
pub fn apply_detail_rgb(src: &LinearRgbImage, detail: &[f32], amount: f32) -> LinearRgbImage {
    let data: Vec<f32> = src
        .pixels()
        .iter()
        .zip(detail.iter())
        .map(|(&s, &d)| s + amount * d)
        .collect();
    LinearRgbImage::new(src.width(), src.height(), data).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn solid(w: u32, h: u32, r: f32, g: f32, b: f32) -> LinearRgbImage {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..(w * h) {
            data.extend_from_slice(&[r, g, b]);
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    fn gradient(w: u32, h: u32) -> LinearRgbImage {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let v = x as f32 / (w.max(2) - 1) as f32;
                let u = y as f32 / (h.max(2) - 1) as f32;
                data.extend_from_slice(&[v, u, 0.5]);
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn output_dimensions_preserved() {
        let src = gradient(32, 24);
        let out = unsharp_mask(&src, 1.5, 1.0).unwrap();
        assert_eq!(out.width(), 32);
        assert_eq!(out.height(), 24);
    }

    #[test]
    fn zero_amount_is_identity() {
        let src = gradient(16, 16);
        let out = unsharp_mask(&src, 0.0, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(out.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn solid_image_unchanged_by_sharpening() {
        // A perfectly flat image has zero high-frequency content; USM is a no-op.
        let src = solid(16, 16, 0.4, 0.6, 0.8);
        let out = unsharp_mask(&src, 3.0, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(out.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-4);
        }
    }

    #[test]
    fn large_amount_produces_out_of_range_values() {
        // A sharp edge in the input should produce ringing (values outside [0,1])
        // when sharpening amount is large enough.
        let mut data = vec![0.0f32; 32 * 1 * 3];
        // Create a hard edge: left half = 0, right half = 1
        for x in 16..32_usize {
            data[x * 3] = 1.0;
            data[x * 3 + 1] = 1.0;
            data[x * 3 + 2] = 1.0;
        }
        let src = LinearRgbImage::new(32, 1, data).unwrap();
        let out = unsharp_mask(&src, 5.0, 1.0).unwrap();
        let has_oob = out.pixels().iter().any(|&v| v < 0.0 || v > 1.0);
        assert!(has_oob, "expected out-of-range values for strong sharpening on an edge");
    }

    #[test]
    fn negative_sigma_returns_error() {
        let src = gradient(8, 8);
        assert!(unsharp_mask(&src, 1.0, -1.0).is_err());
        assert!(unsharp_mask(&src, 1.0, 0.0).is_err());
    }

    // -----------------------------------------------------------------------
    // Adaptive sharpening tests (v0.3)
    // -----------------------------------------------------------------------

    fn make_gain_map(w: u32, h: u32, value: f32) -> crate::GainMap {
        crate::GainMap::new(w, h, vec![value; (w * h) as usize]).unwrap()
    }

    #[test]
    fn adaptive_lightness_gain_one_matches_uniform() {
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let gain_map = make_gain_map(16, 16, 1.0);
        let adaptive = adaptive_sharpen_lightness(&src, &luma, 1.5, &gain_map, 1.0).unwrap();
        let uniform = unsharp_mask_single_channel(
            &luma, 16, 16, 1.5, 1.0,
        ).unwrap();
        let uniform_img = crate::color::reconstruct_rgb_from_lightness(&src, &uniform);
        for (a, b) in adaptive.pixels().iter().zip(uniform_img.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-4);
        }
    }

    #[test]
    fn adaptive_lightness_gain_zero_is_identity() {
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let gain_map = make_gain_map(16, 16, 0.0);
        let result = adaptive_sharpen_lightness(&src, &luma, 2.0, &gain_map, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(result.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn adaptive_rgb_gain_one_matches_uniform() {
        let src = gradient(16, 16);
        let gain_map = make_gain_map(16, 16, 1.0);
        let adaptive = adaptive_sharpen_rgb(&src, 1.5, &gain_map, 1.0).unwrap();
        let uniform = unsharp_mask(&src, 1.5, 1.0).unwrap();
        for (a, b) in adaptive.pixels().iter().zip(uniform.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-4);
        }
    }

    #[test]
    fn adaptive_rgb_gain_zero_is_identity() {
        let src = gradient(16, 16);
        let gain_map = make_gain_map(16, 16, 0.0);
        let result = adaptive_sharpen_rgb(&src, 2.0, &gain_map, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(result.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn adaptive_preserves_out_of_range_values() {
        let mut data = vec![0.0f32; 32 * 1 * 3];
        for x in 16..32_usize {
            data[x * 3] = 1.0;
            data[x * 3 + 1] = 1.0;
            data[x * 3 + 2] = 1.0;
        }
        let src = LinearRgbImage::new(32, 1, data).unwrap();
        let gain_map = make_gain_map(32, 1, 1.5);
        let out = adaptive_sharpen_rgb(&src, 5.0, &gain_map, 1.0).unwrap();
        let has_oob = out.pixels().iter().any(|&v| v < 0.0 || v > 1.0);
        assert!(has_oob, "expected out-of-range values for strong adaptive sharpening");
    }

    #[test]
    fn adaptive_output_dimensions_match() {
        let src = gradient(16, 12);
        let gain_map = make_gain_map(16, 12, 1.0);
        let out = adaptive_sharpen_rgb(&src, 1.0, &gain_map, 1.0).unwrap();
        assert_eq!(out.width(), 16);
        assert_eq!(out.height(), 12);
    }

    // -----------------------------------------------------------------------
    // Detail precomputation tests
    // -----------------------------------------------------------------------

    #[test]
    fn detail_single_channel_matches_usm() {
        // Applying precomputed detail at strength s must produce the same
        // result as the one-shot unsharp_mask_single_channel_with_kernel.
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let kernel = gaussian_kernel(1.0);
        let strength = 1.5;

        let expected = unsharp_mask_single_channel_with_kernel(&luma, 16, 16, strength, &kernel);
        let detail = compute_detail_single_channel(&luma, 16, 16, &kernel);
        let actual = apply_detail_single_channel(&luma, &detail, strength);

        for (a, b) in expected.iter().zip(actual.iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn detail_rgb_matches_usm() {
        let src = gradient(16, 16);
        let kernel = gaussian_kernel(1.0);
        let strength = 2.0;

        let expected = unsharp_mask_with_kernel(&src, strength, &kernel);
        let detail = compute_detail_rgb(&src, &kernel);
        let actual = apply_detail_rgb(&src, &detail, strength);

        for (a, b) in expected.pixels().iter().zip(actual.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn detail_single_channel_into_matches() {
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let kernel = gaussian_kernel(1.0);
        let strength = 1.0;

        let detail = compute_detail_single_channel(&luma, 16, 16, &kernel);
        let expected = apply_detail_single_channel(&luma, &detail, strength);
        let mut actual = vec![0.0f32; luma.len()];
        apply_detail_single_channel_into(&luma, &detail, strength, &mut actual);

        for (a, b) in expected.iter().zip(actual.iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-6);
        }
    }

    #[test]
    fn detail_reuse_multiple_strengths() {
        // Verify that reusing detail for multiple strengths gives consistent
        // results with independent USM calls.
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let kernel = gaussian_kernel(1.0);
        let detail = compute_detail_single_channel(&luma, 16, 16, &kernel);

        for &s in &[0.1, 0.5, 1.0, 2.0, 5.0] {
            let expected = unsharp_mask_single_channel_with_kernel(&luma, 16, 16, s, &kernel);
            let actual = apply_detail_single_channel(&luma, &detail, s);
            for (a, b) in expected.iter().zip(actual.iter()) {
                assert_abs_diff_eq!(a, b, epsilon = 1e-5);
            }
        }
    }
}
