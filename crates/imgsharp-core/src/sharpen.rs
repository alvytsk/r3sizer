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
fn gaussian_blur(src: &LinearRgbImage, kernel: &[f32]) -> LinearRgbImage {
    let w = src.width() as usize;
    let h = src.height() as usize;
    let radius = kernel.len() / 2;

    // --- Horizontal pass ---
    let mut horiz = vec![0.0f32; w * h * 3];
    for y in 0..h {
        let row = src.row(y as u32);
        for x in 0..w {
            for c in 0..3 {
                let mut acc = 0.0f32;
                for (ki, &kv) in kernel.iter().enumerate() {
                    let xi = (x as isize + ki as isize - radius as isize)
                        .clamp(0, w as isize - 1) as usize;
                    acc += row[xi * 3 + c] * kv;
                }
                horiz[(y * w + x) * 3 + c] = acc;
            }
        }
    }

    // --- Vertical pass ---
    let mut vert = vec![0.0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut acc = 0.0f32;
                for (ki, &kv) in kernel.iter().enumerate() {
                    let yi = (y as isize + ki as isize - radius as isize)
                        .clamp(0, h as isize - 1) as usize;
                    acc += horiz[(yi * w + x) * 3 + c] * kv;
                }
                vert[(y * w + x) * 3 + c] = acc;
            }
        }
    }

    LinearRgbImage::new(src.width(), src.height(), vert).unwrap()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
    if sigma <= 0.0 {
        return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
    }

    let kernel = gaussian_kernel(sigma);
    let blurred = gaussian_blur(src, &kernel);

    let src_px = src.pixels();
    let blur_px = blurred.pixels();
    let mut out = Vec::with_capacity(src_px.len());
    for (s, b) in src_px.iter().zip(blur_px.iter()) {
        out.push(s + amount * (s - b));
    }

    LinearRgbImage::new(src.width(), src.height(), out)
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
}
