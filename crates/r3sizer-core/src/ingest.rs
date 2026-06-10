//! Streaming (striped) ingest for very large images.
//!
//! [`StripedPreReducer`] accepts sequential full-width row stripes of sRGB
//! RGBA8 and accumulates an area-weighted pre-reduce directly into the linear
//! intermediate image (~2x the final target) — the same intermediate the
//! staged-shrink path in [`crate::resize`] produces with a bilinear pass.
//! This bounds peak memory to one stripe plus the intermediate, instead of
//! the full source image.

use crate::types::ImageSize;

/// Intermediate (pre-reduce) size for a staged downscale: ~2x the target.
///
/// This is the exact same math the staged-shrink path in `resize.rs` uses,
/// so an image ingested in stripes lands on the identical intermediate
/// dimensions as one resized monolithically.
pub fn compute_intermediate_size(src: ImageSize, target: ImageSize) -> ImageSize {
    let shrink_x = src.width as f64 / target.width as f64;
    let shrink_y = src.height as f64 / target.height as f64;
    let max_shrink = shrink_x.max(shrink_y);
    let pre_factor = (max_shrink / 2.0).floor().max(1.0);
    ImageSize {
        width: ((src.width as f64 / pre_factor).round() as u32).max(target.width),
        height: ((src.height as f64 / pre_factor).round() as u32).max(target.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intermediate_size_matches_staged_shrink_math() {
        // 6000x4000 -> 800x600: max_shrink = 7.5, pre_factor = floor(3.75) = 3
        let inter = compute_intermediate_size(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 800, height: 600 },
        );
        assert_eq!(inter, ImageSize { width: 2000, height: 1333 });
    }

    #[test]
    fn intermediate_size_never_smaller_than_target() {
        // Extremely wide: 40000x800 -> 1600x100. max_shrink = 25, pre = 12.
        // 800/12 = 66.7 -> clamped up to target height 100.
        let inter = compute_intermediate_size(
            ImageSize { width: 40000, height: 800 },
            ImageSize { width: 1600, height: 100 },
        );
        assert_eq!(inter, ImageSize { width: 3333, height: 100 });
    }
}
