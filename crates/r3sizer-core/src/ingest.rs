//! Streaming (striped) ingest for very large images.
//!
//! [`StripedPreReducer`] accepts sequential full-width row stripes of sRGB
//! RGBA8 and accumulates an area-weighted pre-reduce directly into the linear
//! intermediate image (~2x the final target) — the same intermediate the
//! staged-shrink path in [`crate::resize`] produces with a bilinear pass.
//! This bounds peak memory to one stripe plus the intermediate, instead of
//! the full source image.

use crate::color::SRGB_U8_TO_LINEAR;
use crate::types::LinearRgbImage;
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
struct AxisWeights {
    spans: Vec<(u32, f32, f32)>,
}

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

/// Streaming area-weighted pre-reducer.
///
/// Feed full-width sRGB RGBA8 row stripes strictly top-to-bottom via
/// [`push_srgb8_rows`](Self::push_srgb8_rows), then call
/// [`finish`](Self::finish) to obtain the linear intermediate image.
/// Alpha is ignored (input is treated as straight, non-premultiplied RGBA).
#[derive(Debug)]
pub struct StripedPreReducer {
    src: ImageSize,
    intermediate: ImageSize,
    x_weights: AxisWeights,
    y_weights: AxisWeights,
    /// Interleaved RGB accumulators, len = inter_w * inter_h * 3.
    acc: Vec<f32>,
    /// Per-pixel weight accumulator, len = inter_w * inter_h. Normalizing by
    /// the accumulated weight in `finish()` avoids edge errors when source
    /// dimensions do not divide evenly into the intermediate.
    weight: Vec<f32>,
    next_row: u32,
}

impl StripedPreReducer {
    /// `intermediate` must come from [`compute_intermediate_size`] (same
    /// logic as the staged-shrink path, ~2x the final target).
    pub fn new(src: ImageSize, intermediate: ImageSize) -> Result<Self, CoreError> {
        if src.width == 0 || src.height == 0 || intermediate.width == 0 || intermediate.height == 0
        {
            return Err(CoreError::EmptyImage);
        }
        if intermediate.width > src.width || intermediate.height > src.height {
            return Err(CoreError::InvalidParams(format!(
                "intermediate {}x{} exceeds source {}x{}",
                intermediate.width, intermediate.height, src.width, src.height
            )));
        }
        let n = intermediate.width as usize * intermediate.height as usize;
        Ok(Self {
            src,
            intermediate,
            x_weights: axis_weights(src.width, intermediate.width),
            y_weights: axis_weights(src.height, intermediate.height),
            acc: vec![0.0; n * 3],
            weight: vec![0.0; n],
            next_row: 0,
        })
    }

    pub fn source_size(&self) -> ImageSize {
        self.src
    }

    pub fn intermediate_size(&self) -> ImageSize {
        self.intermediate
    }

    pub fn rows_pushed(&self) -> u32 {
        self.next_row
    }

    /// Push the next `rows` full-width rows of sRGB RGBA8 pixels.
    ///
    /// Rows are consumed strictly sequentially top-to-bottom; supplying more
    /// rows than remain in the source is an error.
    pub fn push_srgb8_rows(&mut self, rgba: &[u8], rows: u32) -> Result<(), CoreError> {
        if rows == 0 {
            return Err(CoreError::InvalidParams("stripe with zero rows".into()));
        }
        let remaining = self.src.height - self.next_row;
        if rows > remaining {
            return Err(CoreError::IngestRowOverflow { pushed: rows, remaining });
        }
        let row_bytes = self.src.width as usize * 4;
        let expected_len = row_bytes * rows as usize;
        if rgba.len() != expected_len {
            return Err(CoreError::BufferLengthMismatch {
                expected_len,
                got_len: rgba.len(),
            });
        }

        let iw = self.intermediate.width as usize;
        for r in 0..rows as usize {
            let y = self.next_row as usize + r;
            let (jy, wy0, wy1) = self.y_weights.spans[y];
            let row = &rgba[r * row_bytes..(r + 1) * row_bytes];
            for (x, px) in row.chunks_exact(4).enumerate() {
                let rgb = [
                    SRGB_U8_TO_LINEAR[px[0] as usize],
                    SRGB_U8_TO_LINEAR[px[1] as usize],
                    SRGB_U8_TO_LINEAR[px[2] as usize],
                ];
                let (jx, wx0, wx1) = self.x_weights.spans[x];
                // Up to 2x2 destination cells; second row/col weight may be 0.
                accumulate(&mut self.acc, &mut self.weight, iw, jx, jy, wx0 * wy0, rgb);
                if wx1 > 0.0 {
                    accumulate(&mut self.acc, &mut self.weight, iw, jx + 1, jy, wx1 * wy0, rgb);
                }
                if wy1 > 0.0 {
                    accumulate(&mut self.acc, &mut self.weight, iw, jx, jy + 1, wx0 * wy1, rgb);
                    if wx1 > 0.0 {
                        accumulate(&mut self.acc, &mut self.weight, iw, jx + 1, jy + 1, wx1 * wy1, rgb);
                    }
                }
            }
        }
        self.next_row += rows;
        Ok(())
    }

    /// Normalize the accumulators into the linear intermediate image.
    ///
    /// Errors if not all source rows were supplied.
    pub fn finish(self) -> Result<LinearRgbImage, CoreError> {
        if self.next_row != self.src.height {
            return Err(CoreError::IngestIncomplete {
                supplied: self.next_row,
                expected: self.src.height,
            });
        }
        let mut data = self.acc;
        for (i, w) in self.weight.iter().enumerate() {
            // Every cell received weight: per-cell totals are exactly 1.0
            // per axis (see axis_weights_conservation test).
            let inv = 1.0 / w;
            data[i * 3] *= inv;
            data[i * 3 + 1] *= inv;
            data[i * 3 + 2] *= inv;
        }
        LinearRgbImage::new(self.intermediate.width, self.intermediate.height, data)
    }
}

#[inline]
fn accumulate(
    acc: &mut [f32],
    weight: &mut [f32],
    iw: usize,
    jx: u32,
    jy: u32,
    w: f32,
    [r, g, b]: [f32; 3],
) {
    let idx = jy as usize * iw + jx as usize;
    acc[idx * 3] += w * r;
    acc[idx * 3 + 1] += w * g;
    acc[idx * 3 + 2] += w * b;
    weight[idx] += w;
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

    fn reducer_4x4_to_2x2() -> StripedPreReducer {
        StripedPreReducer::new(
            ImageSize { width: 4, height: 4 },
            ImageSize { width: 2, height: 2 },
        )
        .unwrap()
    }

    #[test]
    fn new_rejects_zero_dimensions() {
        let err = StripedPreReducer::new(
            ImageSize { width: 0, height: 4 },
            ImageSize { width: 2, height: 2 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::EmptyImage));
    }

    #[test]
    fn new_rejects_intermediate_larger_than_source() {
        let err = StripedPreReducer::new(
            ImageSize { width: 4, height: 4 },
            ImageSize { width: 8, height: 2 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidParams(_)));
    }

    #[test]
    fn push_rejects_wrong_buffer_length() {
        let mut r = reducer_4x4_to_2x2();
        // 2 rows of a 4-wide image need 4*2*4 = 32 bytes; give 31.
        let err = r.push_srgb8_rows(&[0u8; 31], 2).unwrap_err();
        assert!(matches!(err, CoreError::BufferLengthMismatch { .. }));
    }

    #[test]
    fn push_rejects_zero_rows() {
        let mut r = reducer_4x4_to_2x2();
        let err = r.push_srgb8_rows(&[], 0).unwrap_err();
        assert!(matches!(err, CoreError::InvalidParams(_)));
    }

    #[test]
    fn push_rejects_row_overflow() {
        let mut r = reducer_4x4_to_2x2();
        r.push_srgb8_rows(&[128u8; 4 * 3 * 4], 3).unwrap();
        // 3 of 4 rows consumed; pushing 2 more overflows.
        let err = r.push_srgb8_rows(&[128u8; 4 * 2 * 4], 2).unwrap_err();
        assert!(matches!(
            err,
            CoreError::IngestRowOverflow { pushed: 2, remaining: 1 }
        ));
    }

    #[test]
    fn finish_rejects_incomplete_supply() {
        let mut r = reducer_4x4_to_2x2();
        r.push_srgb8_rows(&[128u8; 4 * 2 * 4], 2).unwrap();
        let err = r.finish().unwrap_err();
        assert!(matches!(
            err,
            CoreError::IngestIncomplete { supplied: 2, expected: 4 }
        ));
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
