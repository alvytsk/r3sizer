use imgsharp_core::{
    AutoSharpParams, ClampPolicy, CrossingStatus, FitStrategy, ImageSize, LinearRgbImage,
    MetricMode, ProbeConfig, SelectionMode, SharpenMode,
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
        probe_strengths: ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]),
        target_artifact_ratio: 0.001,
        enable_contrast_leveling: false,
        sharpen_sigma: 1.0,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
        sharpen_mode: SharpenMode::Lightness,
        metric_mode: MetricMode::RelativeToBase,
    }
}

fn default_params_rgb(tw: u32, th: u32) -> AutoSharpParams {
    AutoSharpParams {
        sharpen_mode: SharpenMode::Rgb,
        metric_mode: MetricMode::AbsoluteTotal,
        ..default_params(tw, th)
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
        probe_strengths: ProbeConfig::Explicit(vec![0.1, 0.2, 0.3, 0.4]),
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
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let s = out.diagnostics.selected_strength;
    assert!(
        s >= 0.05 && s <= 3.0,
        "selected_strength {s} outside probe range [0.05, 3.0]"
    );
}

#[test]
fn probe_sample_count_matches_config() {
    let src = gradient_image(16, 16);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.probe_samples.len(), 7);
}

#[test]
fn probe_samples_are_finite() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    for ps in &out.diagnostics.probe_samples {
        assert!(ps.strength.is_finite(), "non-finite strength");
        assert!(ps.artifact_ratio.is_finite(), "non-finite artifact_ratio");
        assert!(ps.metric_value.is_finite(), "non-finite metric_value");
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

// ---------------------------------------------------------------------------
// Baseline measurement
// ---------------------------------------------------------------------------

#[test]
fn baseline_artifact_ratio_is_finite() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    assert!(out.diagnostics.baseline_artifact_ratio.is_finite());
    assert!(out.diagnostics.baseline_artifact_ratio >= 0.0);
}

#[test]
fn solid_image_has_zero_baseline() {
    let src = solid_image(8, 8, 0.5);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    assert_eq!(out.diagnostics.baseline_artifact_ratio, 0.0);
}

// ---------------------------------------------------------------------------
// Metric modes
// ---------------------------------------------------------------------------

#[test]
fn relative_mode_metric_values_are_nonnegative() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        metric_mode: MetricMode::RelativeToBase,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for ps in &out.diagnostics.probe_samples {
        assert!(ps.metric_value >= 0.0, "relative metric_value {} < 0", ps.metric_value);
    }
}

#[test]
fn absolute_mode_metric_equals_artifact_ratio() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        metric_mode: MetricMode::AbsoluteTotal,
        sharpen_mode: SharpenMode::Rgb,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for ps in &out.diagnostics.probe_samples {
        assert_eq!(ps.metric_value, ps.artifact_ratio);
    }
}

// ---------------------------------------------------------------------------
// Sharpen modes
// ---------------------------------------------------------------------------

#[test]
fn lightness_mode_produces_valid_result() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        sharpen_mode: SharpenMode::Lightness,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.sharpen_mode, SharpenMode::Lightness);
    assert_eq!(out.image.width(), 4);
}

#[test]
fn rgb_mode_produces_valid_result() {
    let src = gradient_image(16, 16);
    let params = default_params_rgb(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.sharpen_mode, SharpenMode::Rgb);
    assert_eq!(out.image.width(), 4);
}

// ---------------------------------------------------------------------------
// New diagnostics fields
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_have_valid_status_enums() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    let d = &out.diagnostics;

    assert!(matches!(d.fit_status, imgsharp_core::FitStatus::Success));
    assert!(matches!(
        d.crossing_status,
        CrossingStatus::Found | CrossingStatus::NotFoundInRange
    ));
    assert!(matches!(
        d.selection_mode,
        SelectionMode::PolynomialRoot
            | SelectionMode::BestSampleWithinBudget
            | SelectionMode::LeastBadSample
            | SelectionMode::BudgetUnreachable
    ));
}

#[test]
fn budget_reachable_consistent_with_selection_mode() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    let d = &out.diagnostics;

    if matches!(d.selection_mode, SelectionMode::BudgetUnreachable) {
        assert!(!d.budget_reachable);
    }
    if matches!(d.selection_mode, SelectionMode::PolynomialRoot) {
        assert!(d.budget_reachable);
    }
}
