/// Downscale an image in linear RGB space.
///
/// **Engineering note:** The exact downscale kernel used in the original papers
/// is not confirmed from available sources.  This implementation uses Lanczos3
/// resampling via `fast_image_resize` (SIMD-accelerated on x86-64) as a
/// high-quality, well-documented standard.  If the paper-exact kernel is
/// identified in the future, only this module needs to change.
///
/// Resize is performed on the raw f32 linear pixel buffer.  No gamma correction
/// is applied here; callers are responsible for ensuring the image is already in
/// linear light before calling [`downscale`].
use fast_image_resize as fir;
use fir::pixels::F32x3;

use crate::{CoreError, LinearRgbImage, ImageSize};

/// Minimum shrink ratio (max of X and Y) that triggers the two-stage path.
const STAGED_SHRINK_THRESHOLD: f64 = 3.0;

/// Downscale `src` to `target` size using Lanczos3 resampling.
///
/// For shrink ratios above [`STAGED_SHRINK_THRESHOLD`] a two-stage path is
/// used: a fast bilinear pre-reduce brings the image to ~2× the target, then a
/// final Lanczos3 pass produces the output.  This follows the same principle as
/// libvips' `gap` parameter.
///
/// Returns a clone-sized result; `src` is not mutated.
/// Returns `Err` if `target` has a zero dimension.
pub fn downscale(src: &LinearRgbImage, target: ImageSize) -> Result<LinearRgbImage, CoreError> {
    downscale_with_info(src, target).map(|(img, _)| img)
}

/// Like [`downscale`] but also returns `true` when the staged-shrink path was
/// used (for diagnostics).
pub fn downscale_with_info(
    src: &LinearRgbImage,
    target: ImageSize,
) -> Result<(LinearRgbImage, bool), CoreError> {
    if target.width == 0 || target.height == 0 {
        return Err(CoreError::InvalidParams("target dimensions must be non-zero".into()));
    }

    // Fast path: no resize needed.
    if target.width == src.width() && target.height == src.height() {
        return Ok((src.clone(), false));
    }

    let shrink_x = src.width() as f64 / target.width as f64;
    let shrink_y = src.height() as f64 / target.height as f64;
    let max_shrink = shrink_x.max(shrink_y);

    if max_shrink >= STAGED_SHRINK_THRESHOLD {
        // Two-stage: fast bilinear pre-reduce to ~2× target, then Lanczos3.
        let pre_factor = (max_shrink / 2.0).floor().max(1.0);
        let pre_w = ((src.width() as f64 / pre_factor).round() as u32).max(target.width);
        let pre_h = ((src.height() as f64 / pre_factor).round() as u32).max(target.height);

        let pre = fir_resize(src, pre_w, pre_h, fir::ResizeAlg::Convolution(fir::FilterType::Bilinear))?;
        let out = fir_resize(&pre, target.width, target.height, fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3))?;
        Ok((out, true))
    } else {
        let out = fir_resize(src, target.width, target.height, fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3))?;
        Ok((out, false))
    }
}

/// Resize a `LinearRgbImage` using `fast_image_resize`.
///
/// Uses the typed API (`resize_typed<F32x3>`) so only F32x3 convolution code
/// is monomorphized — the u8/u16 pixel-type code is never compiled, saving
/// ~250 KB in the WASM binary.
fn fir_resize(
    src: &LinearRgbImage,
    dst_w: u32,
    dst_h: u32,
    alg: fir::ResizeAlg,
) -> Result<LinearRgbImage, CoreError> {
    let src_pixels = f32_slice_as_f32x3(src.pixels());
    let src_image = fir::images::TypedImageRef::<F32x3>::new(
        src.width(),
        src.height(),
        src_pixels,
    ).map_err(|e| CoreError::InvalidParams(format!("fir source image: {e}")))?;

    let mut dst_image = fir::images::TypedImage::<F32x3>::new(dst_w, dst_h);

    let mut resizer = fir::Resizer::new();
    resizer.resize_typed(&src_image, &mut dst_image, Some(&fir::ResizeOptions::new().resize_alg(alg)))
        .map_err(|e| CoreError::InvalidParams(format!("fir resize: {e}")))?;

    let dst_floats = f32x3_slice_to_vec(dst_image.pixels());
    LinearRgbImage::new(dst_w, dst_h, dst_floats)
}

/// View `&[f32]` (length divisible by 3) as `&[F32x3]` without copying.
fn f32_slice_as_f32x3(floats: &[f32]) -> &[F32x3] {
    assert_eq!(floats.len() % 3, 0, "buffer length not a multiple of 3");
    let pixel_count = floats.len() / 3;
    let ptr = floats.as_ptr() as *const F32x3;
    // SAFETY: F32x3 is repr(transparent) over [f32; 3] — same layout, no padding.
    // The lifetime is tied to `floats`.
    unsafe { std::slice::from_raw_parts(ptr, pixel_count) }
}

/// Copy `&[F32x3]` into a flat `Vec<f32>`.
fn f32x3_slice_to_vec(pixels: &[F32x3]) -> Vec<f32> {
    let ptr = pixels.as_ptr() as *const f32;
    let len = pixels.len() * 3;
    // SAFETY: F32x3 is repr(transparent) over [f32; 3] — reading as f32 slice is valid.
    let flat = unsafe { std::slice::from_raw_parts(ptr, len) };
    flat.to_vec()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let r = x as f32 / (w - 1).max(1) as f32;
                let g = y as f32 / (h - 1).max(1) as f32;
                let b = 0.5;
                data.extend_from_slice(&[r, g, b]);
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn output_dimensions_match_target() {
        let src = gradient_image(100, 80);
        let target = ImageSize { width: 40, height: 30 };
        let out = downscale(&src, target).unwrap();
        assert_eq!(out.width(), 40);
        assert_eq!(out.height(), 30);
    }

    #[test]
    fn same_size_returns_clone() {
        let src = gradient_image(16, 16);
        let target = ImageSize { width: 16, height: 16 };
        let out = downscale(&src, target).unwrap();
        assert_eq!(out.width(), 16);
        assert_eq!(out.height(), 16);
        // Values should be identical (clone path).
        for (a, b) in src.pixels().iter().zip(out.pixels().iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn trivial_1x1_to_1x1() {
        let data = vec![0.2, 0.4, 0.6];
        let src = LinearRgbImage::new(1, 1, data).unwrap();
        let out = downscale(&src, ImageSize { width: 1, height: 1 }).unwrap();
        assert_eq!(out.width(), 1);
        assert_eq!(out.height(), 1);
    }

    #[test]
    fn zero_target_is_error() {
        let src = gradient_image(10, 10);
        assert!(downscale(&src, ImageSize { width: 0, height: 5 }).is_err());
        assert!(downscale(&src, ImageSize { width: 5, height: 0 }).is_err());
    }

    #[test]
    fn downscale_stays_roughly_in_range_for_clean_input() {
        // A clean [0,1] input should stay close to [0,1] after Lanczos.
        // (Lanczos can introduce tiny ringing; we allow a small margin.)
        let src = gradient_image(64, 64);
        let out = downscale(&src, ImageSize { width: 16, height: 16 }).unwrap();
        for &v in out.pixels() {
            assert!(v >= -0.01 && v <= 1.01, "out-of-range value: {v}");
        }
    }

    // -----------------------------------------------------------------------
    // Staged shrink tests
    // -----------------------------------------------------------------------

    #[test]
    fn small_ratio_does_not_use_staged_shrink() {
        let src = gradient_image(100, 80);
        let (out, staged) = downscale_with_info(&src, ImageSize { width: 50, height: 40 }).unwrap();
        assert_eq!(out.width(), 50);
        assert_eq!(out.height(), 40);
        assert!(!staged, "2× ratio should not trigger staged shrink");
    }

    #[test]
    fn large_ratio_uses_staged_shrink() {
        let src = gradient_image(400, 300);
        let (out, staged) = downscale_with_info(&src, ImageSize { width: 80, height: 60 }).unwrap();
        assert_eq!(out.width(), 80);
        assert_eq!(out.height(), 60);
        assert!(staged, "5× ratio should trigger staged shrink");
    }

    #[test]
    fn staged_shrink_output_stays_in_range() {
        let src = gradient_image(640, 480);
        let (out, staged) = downscale_with_info(&src, ImageSize { width: 64, height: 48 }).unwrap();
        assert!(staged, "10× ratio should trigger staged shrink");
        for &v in out.pixels() {
            assert!(v >= -0.02 && v <= 1.02, "out-of-range value from staged shrink: {v}");
        }
    }

    #[test]
    fn staged_shrink_matches_direct_approximately() {
        // Staged shrink should produce similar (but not identical) results
        // compared to a single-pass Lanczos3 downscale.
        let src = gradient_image(400, 300);
        let target = ImageSize { width: 80, height: 60 };

        let (staged_out, did_stage) = downscale_with_info(&src, target).unwrap();
        assert!(did_stage);

        // Direct single-pass Lanczos3 for comparison.
        let direct = fir_resize(
            &src, target.width, target.height,
            fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3),
        ).unwrap();

        // Should be close — the pre-reduce is bilinear so there will be
        // minor differences, but on a smooth gradient they should be small.
        let max_diff: f32 = staged_out.pixels().iter().zip(direct.pixels().iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 0.05, "staged shrink diverges too much from direct: max_diff={max_diff}");
    }

    #[test]
    fn same_size_reports_no_staged_shrink() {
        let src = gradient_image(16, 16);
        let (_, staged) = downscale_with_info(&src, ImageSize { width: 16, height: 16 }).unwrap();
        assert!(!staged);
    }
}
