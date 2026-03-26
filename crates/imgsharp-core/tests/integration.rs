use imgsharp_core::{
    AutoSharpParams, ClampPolicy, FitStrategy, ImageSize, LinearRgbImage, ProbeConfig,
    process_auto_sharp_downscale,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = x as f32 / (w - 1).max(1) as f32;
            let g = y as f32 / (h - 1).max(1) as f32;
            data.extend_from_slice(&[r, g, 0.5]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn checkerboard(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = if (x + y) % 2 == 0 { 0.9 } else { 0.1 };
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn solid_image(w: u32, h: u32, value: f32) -> LinearRgbImage {
    LinearRgbImage::new(w, h, vec![value; (w * h * 3) as usize]).unwrap()
}

fn default_params(tw: u32, th: u32) -> AutoSharpParams {
    AutoSharpParams {
        target_width: tw,
        target_height: th,
        probe_strengths: ProbeConfig::Range { min: 0.5, max: 3.0, count: 7 },
        target_artifact_ratio: 0.001,
        enable_contrast_leveling: false,
        sharpen_sigma: 1.0,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
    }
}

// ---------------------------------------------------------------------------
// Output dimension tests
// ---------------------------------------------------------------------------

#[test]
fn gradient_downscale_output_dimensions() {
    let src = gradient_image(16, 16);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    assert_eq!(out.diagnostics.output_size, ImageSize { width: 4, height: 4 });
    assert_eq!(out.diagnostics.input_size, ImageSize { width: 16, height: 16 });
}

#[test]
fn checkerboard_downscale_no_panic() {
    let src = checkerboard(8, 8);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    // fallback_used field must be a bool (either is fine for a checkerboard).
    let _ = out.diagnostics.fallback_used;
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn one_by_one_image_no_panic() {
    let src = solid_image(1, 1, 0.5);
    let params = AutoSharpParams {
        target_width: 1,
        target_height: 1,
        probe_strengths: ProbeConfig::Range { min: 0.5, max: 3.0, count: 5 },
        ..default_params(1, 1)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 1);
    assert_eq!(out.image.height(), 1);
}

#[test]
fn all_black_image_no_panic() {
    let src = solid_image(8, 8, 0.0);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    assert_eq!(out.image.width(), 4);
}

#[test]
fn all_white_image_no_panic() {
    let src = solid_image(8, 8, 1.0);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    assert_eq!(out.image.width(), 4);
}

// ---------------------------------------------------------------------------
// Diagnostics consistency
// ---------------------------------------------------------------------------

#[test]
fn selected_strength_within_probe_range() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        probe_strengths: ProbeConfig::Range { min: 0.5, max: 3.0, count: 7 },
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let s = out.diagnostics.selected_strength;
    assert!(
        s >= 0.5 && s <= 3.0,
        "selected_strength {s} outside probe range [0.5, 3.0]"
    );
}

#[test]
fn probe_sample_count_matches_config() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        probe_strengths: ProbeConfig::Range { min: 0.5, max: 4.0, count: 9 },
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.probe_samples.len(), 9);
}

#[test]
fn probe_samples_are_finite() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    for ps in &out.diagnostics.probe_samples {
        assert!(ps.strength.is_finite(), "non-finite strength");
        assert!(ps.artifact_ratio.is_finite(), "non-finite artifact_ratio");
    }
}

#[test]
fn output_pixels_all_clamped_after_clamp_policy() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        output_clamp: ClampPolicy::Clamp,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel value {v} outside [0,1] after clamping");
    }
}

// ---------------------------------------------------------------------------
// Direct search strategy
// ---------------------------------------------------------------------------

#[test]
fn direct_search_strategy_produces_valid_result() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        fit_strategy: FitStrategy::DirectSearch,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.diagnostics.fit_coefficients, None);
}

// ---------------------------------------------------------------------------
// Contrast leveling (placeholder, just verify it doesn't crash)
// ---------------------------------------------------------------------------

#[test]
fn contrast_leveling_enabled_no_panic() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        enable_contrast_leveling: true,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
}
