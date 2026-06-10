//! Streaming (striped) ingest for very large images.
//!
//! [`StripedPreReducer`] accepts sequential full-width row stripes of sRGB
//! RGBA8 and accumulates an area-weighted pre-reduce directly into the linear
//! intermediate image (~2x the final target) — the same intermediate the
//! staged-shrink path in [`crate::resize`] produces with a bilinear pass.
//! This bounds peak memory to one stripe plus the intermediate, instead of
//! the full source image.

use crate::{types::ImageSize, CoreError};

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

/// Check that an image qualifies for striped ingest: the shrink ratio must
/// be >= the staged-shrink threshold (3.0), otherwise the pre-reduce to
/// ~2x target is meaningless and the caller should use the monolithic path.
pub fn validate_striped_shrink(src: ImageSize, target: ImageSize) -> Result<(), CoreError> {
    if src.width == 0 || src.height == 0 || target.width == 0 || target.height == 0 {
        return Err(CoreError::EmptyImage);
    }
    let shrink_x = src.width as f64 / target.width as f64;
    let shrink_y = src.height as f64 / target.height as f64;
    let max_shrink = shrink_x.max(shrink_y);
    if max_shrink < crate::resize::STAGED_SHRINK_THRESHOLD {
        return Err(CoreError::TargetTooCloseForStripedIngest { ratio: max_shrink });
    }
    Ok(())
}

/// Per-source-index destination mapping for one axis.
///
/// `spans[i]` = (first destination index, weight into it, weight into the
/// *next* destination index — 0.0 when the source pixel does not straddle a
/// cell boundary). Weights are measured in destination-space length, so each
/// span's weights sum to `scale = dst_len / src_len`.
#[derive(Debug)]
#[allow(dead_code)] // constructed by StripedPreReducer (next commit)
struct AxisWeights {
    spans: Vec<(u32, f32, f32)>,
}

#[allow(dead_code)] // called by StripedPreReducer (next commit)
fn axis_weights(src_len: u32, dst_len: u32) -> AxisWeights {
    debug_assert!(dst_len >= 1 && dst_len <= src_len);
    let scale = dst_len as f64 / src_len as f64;
    let mut spans = Vec::with_capacity(src_len as usize);
    for i in 0..src_len {
        let start = i as f64 * scale;
        let end = (i as f64 + 1.0) * scale;
        let j0 = (start.floor() as u32).min(dst_len - 1);
        let boundary = (j0 + 1) as f64;
        if end <= boundary + 1e-12 || j0 + 1 >= dst_len {
            spans.push((j0, (end - start) as f32, 0.0));
        } else {
            spans.push((j0, (boundary - start) as f32, (end - boundary) as f32));
        }
    }
    AxisWeights { spans }
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

    #[test]
    fn validate_rejects_small_shrink_ratio() {
        let err = validate_striped_shrink(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 3000, height: 2000 }, // ratio 2.0 < 3.0
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TargetTooCloseForStripedIngest { .. }
        ));
    }

    #[test]
    fn validate_accepts_ratio_at_threshold() {
        // ratio exactly 3.0 is allowed (matches the staged-shrink branch: >= 3.0).
        validate_striped_shrink(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 2000, height: 4000 },
        )
        .unwrap();
    }

    #[test]
    fn validate_rejects_zero_dimensions() {
        let err = validate_striped_shrink(
            ImageSize { width: 0, height: 4000 },
            ImageSize { width: 100, height: 100 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::EmptyImage));
    }

    #[test]
    fn axis_weights_even_division() {
        // 4 source -> 2 dst, scale 0.5: pixels 0,1 -> cell 0; pixels 2,3 -> cell 1.
        let aw = axis_weights(4, 2);
        assert_eq!(aw.spans, vec![(0, 0.5, 0.0), (0, 0.5, 0.0), (1, 0.5, 0.0), (1, 0.5, 0.0)]);
    }

    #[test]
    fn axis_weights_uneven_split_pixel() {
        // 5 source -> 2 dst, scale 0.4: source pixel 2 straddles the boundary.
        let aw = axis_weights(5, 2);
        assert_eq!(aw.spans[2].0, 0);
        assert!((aw.spans[2].1 - 0.2).abs() < 1e-6); // into cell 0
        assert!((aw.spans[2].2 - 0.2).abs() < 1e-6); // into cell 1
    }

    #[test]
    fn axis_weights_conservation() {
        // For any (src, dst): each source span sums to `scale`, and the total
        // weight landing in each destination cell is exactly 1.0 (its width
        // in destination space).
        for (src, dst) in [(4u32, 2u32), (5, 2), (97, 13), (1000, 333), (7, 7)] {
            let scale = dst as f64 / src as f64;
            let aw = axis_weights(src, dst);
            let mut per_cell = vec![0.0f64; dst as usize];
            for &(j0, w0, w1) in &aw.spans {
                // 1e-6 not 1e-9: weights are stored as f32, which rounds
                // non-dyadic scales (e.g. 0.4) with ~1e-8 error per weight.
                assert!((w0 as f64 + w1 as f64 - scale).abs() < 1e-6);
                per_cell[j0 as usize] += w0 as f64;
                if w1 > 0.0 {
                    per_cell[j0 as usize + 1] += w1 as f64;
                }
            }
            for (j, total) in per_cell.iter().enumerate() {
                assert!(
                    (total - 1.0).abs() < 1e-6,
                    "src={src} dst={dst} cell {j}: total weight {total}"
                );
            }
        }
    }
}
