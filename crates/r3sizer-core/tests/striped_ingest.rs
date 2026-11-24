//! End-to-end parity: a "large" image processed via striped ingest selects a
//! sharpening strength close to the monolithic staged path. The intermediates
//! differ (area average vs. bilinear pre-reduce), so s* is compared with a
//! tolerance, not exactly.

use r3sizer_core::color::SRGB_U8_TO_LINEAR;
use r3sizer_core::{
    compute_intermediate_size, process_auto_sharp_downscale, validate_striped_shrink,
    AutoSharpParams, ImageSize, LinearRgbImage, SelectionMode, StripedPreReducer,
};

/// Moderate-contrast checker (96px cells, 80/175). The resize stays in gamut
/// on its own and sharpening pushes edge overshoot out of it, so the artifact
/// ratio crosses its budget well inside the probe range and the solver fits a
/// root — the value the two paths can genuinely disagree about. Smoother
/// content never leaves the budget (both paths pick the largest probe); a
/// full-contrast checker is over budget before any sharpening (both pick the
/// smallest).
fn synthetic_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = if (x / 96 + y / 96) % 2 == 0 { 175 } else { 80 };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    data
}

fn linear_from_rgba(rgba: &[u8], w: u32, h: u32) -> LinearRgbImage {
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.as_chunks::<4>().0 {
        out.push(SRGB_U8_TO_LINEAR[px[0] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[1] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[2] as usize]);
    }
    LinearRgbImage::new(w, h, out).unwrap()
}

/// Gate, reducer and pipeline together, in the order `ingest_begin` uses them.
fn assert_striped_matches_monolithic(target: ImageSize) {
    let (w, h) = (2400u32, 1600u32);
    let rgba = synthetic_rgba(w, h);
    let src = ImageSize {
        width: w,
        height: h,
    };

    // The gate `ingest_begin` applies before building the reducer.
    validate_striped_shrink(src, target).unwrap();

    // Compare the solver's output. The heuristic evaluator's cap depends only
    // on base edge density and would stand in for the fit on both paths.
    let params = AutoSharpParams {
        target_width: target.width,
        target_height: target.height,
        evaluator_config: None,
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

    // A parity claim is only meaningful if both paths selected a fitted root:
    // a fallback pick is clamped to a probe, and the heuristic evaluator's cap
    // is a function of base edge density, not of the probes.
    for (name, d) in [
        ("monolithic", &mono.diagnostics),
        ("striped", &striped.diagnostics),
    ] {
        assert!(
            matches!(d.selection_mode, SelectionMode::PolynomialRoot) && d.evaluator_cap.is_none(),
            "{name} did not select a fitted root: mode={:?}, evaluator_cap={:?}",
            d.selection_mode,
            d.evaluator_cap
        );
    }

    assert!(
        (s_striped - s_mono).abs() <= 0.3,
        "selected strength diverged: monolithic={s_mono}, striped={s_striped}"
    );
}

#[test]
fn striped_ingest_selects_similar_strength_to_monolithic() {
    assert_striped_matches_monolithic(ImageSize {
        width: 400,
        height: 267,
    }); // ratio 6
}

#[test]
fn striped_ingest_just_above_the_4x_gate_matches_monolithic() {
    // 2400x1600 -> 533x355 is ~4.5x: the window the old 3.0 gate let through
    // and no test covered, just past the 4.0x boundary.
    assert_striped_matches_monolithic(ImageSize {
        width: 533,
        height: 355,
    });
}
