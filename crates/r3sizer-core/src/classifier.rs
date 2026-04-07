//! Content-adaptive region classification for v0.3 adaptive sharpening.
//!
//! Self-contained module — does not depend on `metrics/`.
//! Uses the same CIE Y luminance coefficients as `color.rs`
//! (intentionally co-owned; shared constants extracted if duplication spreads).

use crate::{ClassificationParams, GainMap, GainTable, LinearRgbImage, RegionClass, RegionMap};

// ---------------------------------------------------------------------------
// Luminance coefficients (co-owned with color.rs)
// ---------------------------------------------------------------------------

/// CIE Y luminance from linear sRGB. Same formula as `color::luminance_from_linear_srgb`.
#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ---------------------------------------------------------------------------
// Classification rule (exposed for direct testing)
// ---------------------------------------------------------------------------

/// Classify a single pixel from pre-computed features.
///
/// Priority order (part of the public contract):
/// 1. `g >= gradient_high && v >= variance_high` → `RiskyHaloZone`
/// 2. `g >= gradient_high`                       → `StrongEdge`
/// 3. `v >= variance_high && g < gradient_low`   → `Microtexture`
/// 4. `v >= variance_low || g >= gradient_low`    → `Textured`
/// 5. else                                        → `Flat`
pub(crate) fn classify_features(
    gradient_mag: f32,
    variance: f32,
    params: &ClassificationParams,
) -> RegionClass {
    if gradient_mag >= params.gradient_high_threshold && variance >= params.variance_high_threshold {
        RegionClass::RiskyHaloZone
    } else if gradient_mag >= params.gradient_high_threshold {
        RegionClass::StrongEdge
    } else if variance >= params.variance_high_threshold && gradient_mag < params.gradient_low_threshold {
        RegionClass::Microtexture
    } else if variance >= params.variance_low_threshold || gradient_mag >= params.gradient_low_threshold {
        RegionClass::Textured
    } else {
        RegionClass::Flat
    }
}

// ---------------------------------------------------------------------------
// Pass 1: Sobel gradient (unnormalized, edge-replicate)
// ---------------------------------------------------------------------------

/// Shared Sobel gradient returning magnitude, dx, and dy.
///
/// Split into border (clamped) and interior (unchecked) loops for performance.
/// The interior loop has no bounds checks, enabling auto-vectorization.
///
/// Used by both the classifier (magnitude only) and the edge profiler (all three).
pub(crate) fn sobel_gradient_full(
    luma: &[f32],
    width: usize,
    height: usize,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let n = width * height;
    let mut mag = vec![0.0_f32; n];
    let mut dx = vec![0.0_f32; n];
    let mut dy = vec![0.0_f32; n];

    // Clamped version for border pixels.
    let clamp_x = |x: isize| -> usize { (x.max(0) as usize).min(width - 1) };
    let clamp_y = |y: isize| -> usize { (y.max(0) as usize).min(height - 1) };
    let px = |x: isize, y: isize| -> f32 { luma[clamp_y(y) * width + clamp_x(x)] };

    #[inline(always)]
    fn sobel_clamped(px: impl Fn(isize, isize) -> f32, xi: isize, yi: isize) -> (f32, f32, f32) {
        let gx = -px(xi - 1, yi - 1) + px(xi + 1, yi - 1)
            - 2.0 * px(xi - 1, yi) + 2.0 * px(xi + 1, yi)
            - px(xi - 1, yi + 1) + px(xi + 1, yi + 1);
        let gy = -px(xi - 1, yi - 1) - 2.0 * px(xi, yi - 1) - px(xi + 1, yi - 1)
            + px(xi - 1, yi + 1) + 2.0 * px(xi, yi + 1) + px(xi + 1, yi + 1);
        ((gx * gx + gy * gy).sqrt(), gx, gy)
    }

    // Top border row (y == 0)
    #[allow(clippy::needless_range_loop)]
    if height > 0 {
        for x in 0..width {
            let (m, gx, gy) = sobel_clamped(px, x as isize, 0);
            mag[x] = m;
            dx[x] = gx;
            dy[x] = gy;
        }
    }

    // Left and right border columns + interior for rows 1..height-1
    for y in 1..height.saturating_sub(1) {
        // Left border pixel
        let idx = y * width;
        let (m, gx, gy) = sobel_clamped(px, 0, y as isize);
        mag[idx] = m;
        dx[idx] = gx;
        dy[idx] = gy;

        // Interior: direct indexing, no bounds checks
        let prev_row = &luma[(y - 1) * width..y * width];
        let curr_row = &luma[y * width..(y + 1) * width];
        let next_row = &luma[(y + 1) * width..(y + 2) * width];
        let out_mag = &mut mag[y * width..(y + 1) * width];
        let out_dx = &mut dx[y * width..(y + 1) * width];
        let out_dy = &mut dy[y * width..(y + 1) * width];
        for x in 1..width.saturating_sub(1) {
            let tl = prev_row[x - 1];
            let tc = prev_row[x];
            let tr = prev_row[x + 1];
            let ml = curr_row[x - 1];
            let mr = curr_row[x + 1];
            let bl = next_row[x - 1];
            let bc = next_row[x];
            let br = next_row[x + 1];

            let gx = -tl + tr - 2.0 * ml + 2.0 * mr - bl + br;
            let gy = -tl - 2.0 * tc - tr + bl + 2.0 * bc + br;
            out_mag[x] = (gx * gx + gy * gy).sqrt();
            out_dx[x] = gx;
            out_dy[x] = gy;
        }

        // Right border pixel
        if width > 1 {
            let idx = y * width + width - 1;
            let (m, gx, gy) = sobel_clamped(px, (width - 1) as isize, y as isize);
            mag[idx] = m;
            dx[idx] = gx;
            dy[idx] = gy;
        }
    }

    // Bottom border row (y == height - 1), if height > 1
    if height > 1 {
        let y = height - 1;
        for x in 0..width {
            let idx = y * width + x;
            let (m, gx, gy) = sobel_clamped(px, x as isize, y as isize);
            mag[idx] = m;
            dx[idx] = gx;
            dy[idx] = gy;
        }
    }

    (mag, dx, dy)
}

/// Convenience wrapper: returns only gradient magnitude.
fn sobel_gradient_magnitude(luma: &[f32], width: usize, height: usize) -> Vec<f32> {
    sobel_gradient_full(luma, width, height).0
}

// ---------------------------------------------------------------------------
// Pass 2: Local variance (edge-replicate)
// ---------------------------------------------------------------------------

/// Per-pixel variance of luminance in a square window.
///
/// `window_size` must be odd and >= 3 (validated by ClassificationParams).
/// Border handling: edge-replicate.
///
/// Uses summed area tables (SAT) for O(1) per-pixel variance instead of
/// the naive O(W²) scan. The luma buffer is padded with replicated edges
/// so that every pixel sees a full `window_size × window_size` window.
fn local_variance(luma: &[f32], width: usize, height: usize, window_size: usize) -> Vec<f32> {
    let half = window_size / 2;
    let count = (window_size * window_size) as f64;
    let inv_count = 1.0 / count;

    // Build edge-replicated padded buffer.
    let pw = width + 2 * half;  // padded width
    let ph = height + 2 * half; // padded height
    let mut padded = vec![0.0_f32; pw * ph];
    for py in 0..ph {
        let sy = py.saturating_sub(half).min(height - 1);
        for px in 0..pw {
            let sx = px.saturating_sub(half).min(width - 1);
            padded[py * pw + px] = luma[sy * width + sx];
        }
    }

    // Build SATs for sum and sum-of-squares.
    // SAT is (ph+1) × (pw+1) with a zero-padded top row and left column.
    let sat_w = pw + 1;
    let sat_h = ph + 1;
    let mut sat_sum = vec![0.0_f64; sat_w * sat_h];
    let mut sat_sq = vec![0.0_f64; sat_w * sat_h];

    for y in 0..ph {
        for x in 0..pw {
            let v = padded[y * pw + x] as f64;
            let idx = (y + 1) * sat_w + (x + 1);
            sat_sum[idx] = v
                + sat_sum[y * sat_w + (x + 1)]
                + sat_sum[(y + 1) * sat_w + x]
                - sat_sum[y * sat_w + x];
            sat_sq[idx] = v * v
                + sat_sq[y * sat_w + (x + 1)]
                + sat_sq[(y + 1) * sat_w + x]
                - sat_sq[y * sat_w + x];
        }
    }

    // Query: for pixel (x, y) in the original image, the window in padded
    // coordinates is [x, x + window_size) × [y, y + window_size).
    // In SAT coordinates (1-indexed): top-left = (y, x), bottom-right = (y + window_size, x + window_size).
    let n = width * height;
    let mut var = vec![0.0_f32; n];

    for y in 0..height {
        for x in 0..width {
            let y0 = y;
            let x0 = x;
            let y1 = y + window_size;
            let x1 = x + window_size;

            let s = sat_sum[y1 * sat_w + x1]
                - sat_sum[y0 * sat_w + x1]
                - sat_sum[y1 * sat_w + x0]
                + sat_sum[y0 * sat_w + x0];
            let sq = sat_sq[y1 * sat_w + x1]
                - sat_sq[y0 * sat_w + x1]
                - sat_sq[y1 * sat_w + x0]
                + sat_sq[y0 * sat_w + x0];

            let mean = s * inv_count;
            var[y * width + x] = (sq * inv_count - mean * mean).max(0.0) as f32;
        }
    }

    var
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Classify every pixel of a linear RGB image into region classes.
///
/// Four passes over a luminance channel extracted internally:
/// 0. Luminance extraction
/// 1. Sobel gradient magnitude (unnormalized, edge-replicate border)
/// 2. Local variance (square window, edge-replicate border)
/// 3. Per-pixel classification via [`classify_features`]
pub fn classify(
    image: &LinearRgbImage,
    params: &ClassificationParams,
) -> RegionMap {
    let w = image.width() as usize;
    let h = image.height() as usize;

    // Pass 0: luminance extraction
    let luma: Vec<f32> = image
        .pixels()
        .chunks_exact(3)
        .map(|rgb| luminance(rgb[0], rgb[1], rgb[2]))
        .collect();

    // Pass 1: Sobel gradient magnitude
    let grad = sobel_gradient_magnitude(&luma, w, h);

    // Pass 2: local variance
    let var = local_variance(&luma, w, h, params.variance_window);

    // Pass 3: per-pixel classification
    let data: Vec<RegionClass> = grad
        .iter()
        .zip(var.iter())
        .map(|(&g, &v)| classify_features(g, v, params))
        .collect();

    RegionMap::new(image.width(), image.height(), data).unwrap()
}

/// Produce a per-pixel gain map from a region map and gain table.
pub fn gain_map_from_region_map(
    region_map: &RegionMap,
    gain_table: &GainTable,
) -> GainMap {
    let data: Vec<f32> = region_map
        .data()
        .iter()
        .map(|&c| gain_table.gain_for(c))
        .collect();
    GainMap::new(region_map.width, region_map.height, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegionCoverage;

    fn default_params() -> ClassificationParams {
        ClassificationParams::default()
    }

    fn make_solid_image(w: u32, h: u32, value: f32) -> LinearRgbImage {
        LinearRgbImage::new(w, h, vec![value; (w * h * 3) as usize]).unwrap()
    }

    // -----------------------------------------------------------------------
    // Layer (a): pure rule tests for classify_features
    // -----------------------------------------------------------------------

    #[test]
    fn flat_low_gradient_low_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.01, 0.0005, &p),
            RegionClass::Flat,
        );
    }

    #[test]
    fn textured_moderate_gradient() {
        let p = default_params();
        // gradient >= gradient_low (0.05), variance below variance_low
        assert_eq!(
            classify_features(0.10, 0.0005, &p),
            RegionClass::Textured,
        );
    }

    #[test]
    fn textured_moderate_variance() {
        let p = default_params();
        // gradient < gradient_low, variance >= variance_low (0.001)
        assert_eq!(
            classify_features(0.01, 0.005, &p),
            RegionClass::Textured,
        );
    }

    #[test]
    fn strong_edge_high_gradient_low_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.50, 0.005, &p),
            RegionClass::StrongEdge,
        );
    }

    #[test]
    fn microtexture_high_variance_low_gradient() {
        let p = default_params();
        // variance >= variance_high (0.01) AND gradient < gradient_low (0.05)
        assert_eq!(
            classify_features(0.02, 0.015, &p),
            RegionClass::Microtexture,
        );
    }

    #[test]
    fn risky_halo_zone_high_gradient_high_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.50, 0.015, &p),
            RegionClass::RiskyHaloZone,
        );
    }

    #[test]
    fn risky_halo_takes_priority_over_strong_edge() {
        let p = default_params();
        // Both gradient and variance are high -> RiskyHaloZone, not StrongEdge
        assert_eq!(
            classify_features(1.0, 0.1, &p),
            RegionClass::RiskyHaloZone,
        );
    }

    #[test]
    fn moderate_gradient_high_variance_is_textured_not_microtexture() {
        let p = default_params();
        // variance >= variance_high but gradient >= gradient_low (not < gradient_low)
        // so Microtexture rule does not match; falls through to Textured
        assert_eq!(
            classify_features(0.10, 0.015, &p),
            RegionClass::Textured,
        );
    }

    // -----------------------------------------------------------------------
    // gain_map_from_region_map
    // -----------------------------------------------------------------------

    #[test]
    fn gain_map_matches_table_lookup() {
        let map = RegionMap::new(2, 1, vec![
            RegionClass::Flat, RegionClass::StrongEdge,
        ]).unwrap();
        let gt = GainTable::v03_default();
        let gm = gain_map_from_region_map(&map, &gt);
        assert_eq!(gm.width, 2);
        assert_eq!(gm.height, 1);
        assert!((gm.get(0, 0) - 0.75).abs() < 1e-6);
        assert!((gm.get(1, 0) - 1.00).abs() < 1e-6);
    }

    // -----------------------------------------------------------------------
    // Layer (b): feature extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn sobel_on_uniform_returns_zeros() {
        let luma = vec![0.5_f32; 8 * 8];
        let grad = sobel_gradient_magnitude(&luma, 8, 8);
        for &g in &grad {
            assert!(g.abs() < 1e-6, "expected ~0 gradient on uniform image, got {g}");
        }
    }

    #[test]
    fn sobel_on_vertical_edge_detects_edge() {
        let mut luma = vec![0.0_f32; 8 * 8];
        for y in 0..8_usize {
            for x in 4..8_usize {
                luma[y * 8 + x] = 1.0;
            }
        }
        let grad = sobel_gradient_magnitude(&luma, 8, 8);
        // Interior edge pixels near x=3..5 should have high gradient
        let edge_grad = grad[3 * 8 + 3]; // row 3, just left of edge
        assert!(edge_grad > 0.5, "expected significant gradient at edge, got {edge_grad}");
    }

    #[test]
    fn variance_on_uniform_returns_zeros() {
        let luma = vec![0.5_f32; 8 * 8];
        let var = local_variance(&luma, 8, 8, 5);
        for &v in &var {
            assert!(v.abs() < 1e-6, "expected ~0 variance on uniform image, got {v}");
        }
    }

    #[test]
    fn classify_solid_image_all_flat() {
        let img = make_solid_image(8, 8, 0.5);
        let params = default_params();
        let map = classify(&img, &params);
        assert_eq!(map.width, 8);
        assert_eq!(map.height, 8);
        let cov = RegionCoverage::from_region_map(&map);
        assert_eq!(cov.flat, 64);
        assert_eq!(cov.total_pixels, 64);
    }

    #[test]
    fn classify_step_edge_contains_strong_edge() {
        // Left half = 0.0, right half = 1.0
        let w = 16_u32;
        let h = 8_u32;
        let mut data = vec![0.0_f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in (w / 2)..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = 1.0;
                data[idx + 1] = 1.0;
                data[idx + 2] = 1.0;
            }
        }
        let img = LinearRgbImage::new(w, h, data).unwrap();
        let map = classify(&img, &default_params());
        let cov = RegionCoverage::from_region_map(&map);
        assert!(cov.strong_edge > 0 || cov.risky_halo_zone > 0,
            "expected some StrongEdge or RiskyHaloZone pixels at the step edge");
        assert_eq!(
            cov.flat + cov.textured + cov.strong_edge + cov.microtexture + cov.risky_halo_zone,
            cov.total_pixels,
        );
    }

    #[test]
    fn classify_border_shapes_no_panic() {
        let p = default_params();
        // 1x1
        let img = make_solid_image(1, 1, 0.5);
        let _ = classify(&img, &p);
        // 1xN
        let img = make_solid_image(1, 8, 0.5);
        let _ = classify(&img, &p);
        // Nx1
        let img = make_solid_image(8, 1, 0.5);
        let _ = classify(&img, &p);
        // 2x2
        let img = make_solid_image(2, 2, 0.5);
        let _ = classify(&img, &p);
    }

    #[test]
    fn classify_is_deterministic() {
        let img = make_solid_image(8, 8, 0.5);
        let p = default_params();
        let map1 = classify(&img, &p);
        let map2 = classify(&img, &p);
        assert_eq!(map1.data(), map2.data());
    }
}
