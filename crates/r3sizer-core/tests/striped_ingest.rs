//! End-to-end parity: a "large" image processed via striped ingest selects a
//! sharpening strength close to the monolithic staged path. The intermediates
//! differ (area average vs. bilinear pre-reduce), so s* is compared with a
//! tolerance, not exactly.

use r3sizer_core::color::SRGB_U8_TO_LINEAR;
use r3sizer_core::{
    compute_intermediate_size, process_auto_sharp_downscale, AutoSharpParams, ImageSize,
    LinearRgbImage, StripedPreReducer,
};

/// Gradient + checker detail so probing has artifacts to measure.
fn synthetic_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let base = (x * 200 / w.max(1)) as u8;
            let checker = if (x / 3 + y / 3) % 2 == 0 { 55 } else { 0 };
            let r = base.saturating_add(checker);
            let g = 200u8.saturating_sub(base);
            let b = (y * 200 / h.max(1)) as u8;
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    data
}

fn linear_from_rgba(rgba: &[u8], w: u32, h: u32) -> LinearRgbImage {
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.chunks_exact(4) {
        out.push(SRGB_U8_TO_LINEAR[px[0] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[1] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[2] as usize]);
    }
    LinearRgbImage::new(w, h, out).unwrap()
}

#[test]
fn striped_ingest_selects_similar_strength_to_monolithic() {
    let (w, h) = (2400u32, 1600u32);
    let rgba = synthetic_rgba(w, h);
    let src = ImageSize {
        width: w,
        height: h,
    };
    let target = ImageSize {
        width: 400,
        height: 267,
    }; // ratio 6 -> staged path

    let params = AutoSharpParams {
        target_width: target.width,
        target_height: target.height,
        ..Default::default()
    };

    // Monolithic reference, built from the SAME quantized pixels.
    let full = linear_from_rgba(&rgba, w, h);
    let mono = process_auto_sharp_downscale(&full, &params).unwrap();

    // Striped: feed 64-row stripes through the reducer, then the same pipeline.
    let inter_size = compute_intermediate_size(src, target);
    let mut reducer = StripedPreReducer::new(src, inter_size).unwrap();
    let row_bytes = (w as usize) * 4;
    for chunk in rgba.chunks(row_bytes * 64) {
        let rows = (chunk.len() / row_bytes) as u32;
        reducer.push_srgb8_rows(chunk, rows).unwrap();
    }
    let intermediate = reducer.finish().unwrap();
    assert_eq!(intermediate.width(), inter_size.width);
    assert_eq!(intermediate.height(), inter_size.height);

    let striped = process_auto_sharp_downscale(&intermediate, &params).unwrap();

    assert_eq!(striped.image.width(), target.width);
    assert_eq!(striped.image.height(), target.height);

    let s_mono = mono.diagnostics.selected_strength;
    let s_striped = striped.diagnostics.selected_strength;
    assert!(
        (s_striped - s_mono).abs() <= 0.3,
        "selected strength diverged: monolithic={s_mono}, striped={s_striped}"
    );
}
