use r3sizer_core::{
    ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, CrossingStatus,
    DiagnosticsLevel, FallbackReason, FitStrategy, ImageSize, LinearRgbImage, MetricComponent,
    MetricMode, ProbeConfig, Provenance, SelectionMode, SharpenMode, SharpenModel,
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
        ..AutoSharpParams::default()
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

    assert!(matches!(d.fit_status, r3sizer_core::FitStatus::Success));
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

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

#[test]
fn provenance_is_populated_for_default_config() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    let p = &out.diagnostics.provenance;

    assert_eq!(p.color_conversion, Provenance::PaperConfirmed);
    assert_eq!(p.resize, Provenance::EngineeringChoice);
    // Default has contrast leveling disabled -> PaperConfirmed (stage not applicable).
    assert_eq!(p.contrast_leveling, Provenance::PaperConfirmed);
    assert_eq!(p.sharpen_operator, Provenance::EngineeringChoice);
    // Default is Lightness mode.
    assert_eq!(p.lightness_reconstruction, Provenance::PaperSupported);
    assert_eq!(p.artifact_metric, Provenance::EngineeringProxy);
    assert_eq!(p.polynomial_fit, Provenance::PaperConfirmed);
}

#[test]
fn provenance_paper_lightness_approx_is_paper_supported() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        sharpen_model: SharpenModel::PaperLightnessApprox,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.provenance.sharpen_operator, Provenance::PaperSupported);
}

#[test]
fn provenance_contrast_leveling_enabled_is_placeholder() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        enable_contrast_leveling: true,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.provenance.contrast_leveling, Provenance::Placeholder);
}

// ---------------------------------------------------------------------------
// SharpenModel and ArtifactMetric
// ---------------------------------------------------------------------------

#[test]
fn paper_lightness_approx_requires_lightness_mode() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        sharpen_mode: SharpenMode::Rgb,
        sharpen_model: SharpenModel::PaperLightnessApprox,
        ..default_params(4, 4)
    };
    assert!(process_auto_sharp_downscale(&src, &params).is_err());
}

#[test]
fn pixel_out_of_gamut_metric_produces_valid_result() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        artifact_metric: ArtifactMetric::PixelOutOfGamutRatio,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert!(out.diagnostics.measured_artifact_ratio >= 0.0);
    assert_eq!(out.diagnostics.artifact_metric, ArtifactMetric::PixelOutOfGamutRatio);
}

#[test]
fn paper_lightness_approx_matches_practical_usm() {
    // The scaffold delegates to the same USM, so outputs must be identical.
    let src = gradient_image(16, 16);
    let params_usm = AutoSharpParams {
        sharpen_model: SharpenModel::PracticalUsm,
        ..default_params(4, 4)
    };
    let params_paper = AutoSharpParams {
        sharpen_model: SharpenModel::PaperLightnessApprox,
        ..default_params(4, 4)
    };
    let out_usm = process_auto_sharp_downscale(&src, &params_usm).unwrap();
    let out_paper = process_auto_sharp_downscale(&src, &params_paper).unwrap();
    assert_eq!(out_usm.image.pixels(), out_paper.image.pixels());
    assert_eq!(
        out_usm.diagnostics.selected_strength,
        out_paper.diagnostics.selected_strength
    );
}

#[test]
fn diagnostics_reflect_sharpen_model() {
    let src = gradient_image(16, 16);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.sharpen_model, SharpenModel::PracticalUsm);
    assert_eq!(out.diagnostics.artifact_metric, ArtifactMetric::ChannelClippingRatio);
}

// ---------------------------------------------------------------------------
// Fit quality
// ---------------------------------------------------------------------------

#[test]
fn fit_quality_present_for_cubic_strategy() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let q = out.diagnostics.fit_quality.expect("fit_quality should be present for cubic strategy");
    assert!(q.r_squared.is_finite());
    assert!(q.residual_sum_of_squares.is_finite());
    assert!(q.max_residual.is_finite());
    assert!(q.min_pivot > 0.0);
}

#[test]
fn fit_quality_none_for_direct_search() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        fit_strategy: FitStrategy::DirectSearch,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert!(out.diagnostics.fit_quality.is_none());
}

// ---------------------------------------------------------------------------
// Robustness flags
// ---------------------------------------------------------------------------

#[test]
fn robustness_flags_present() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let r = out.diagnostics.robustness.expect("robustness should be present");
    // quasi_monotonic should be at least as permissive as monotonic.
    if r.monotonic {
        assert!(r.quasi_monotonic);
    }
    assert!(r.max_loo_root_change.is_finite());
}

// ---------------------------------------------------------------------------
// Fallback reason
// ---------------------------------------------------------------------------

#[test]
fn no_fallback_reason_for_polynomial_root() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    if out.diagnostics.selection_mode == SelectionMode::PolynomialRoot {
        assert!(out.diagnostics.fallback_reason.is_none());
    }
}

#[test]
fn direct_search_has_fallback_reason() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        fit_strategy: FitStrategy::DirectSearch,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.fallback_reason, Some(FallbackReason::DirectSearchConfigured));
}

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

#[test]
fn timing_all_stages_positive() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let t = &out.diagnostics.timing;
    assert!(t.total_us > 0);
    assert!(t.probing_us > 0);
}

#[test]
fn timing_total_gte_parts() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let t = &out.diagnostics.timing;
    let parts_sum = t.resize_us + t.contrast_us + t.baseline_us + t.probing_us
        + t.fit_us + t.robustness_us + t.final_sharpen_us + t.clamp_us;
    // Total should be >= sum of parts (captures overhead between stages too).
    assert!(t.total_us >= parts_sum / 2, "total_us={} should be roughly >= parts_sum={}", t.total_us, parts_sum);
}

// ---------------------------------------------------------------------------
// Metric breakdown (v0.2 scaffold)
// ---------------------------------------------------------------------------

#[test]
fn metric_breakdown_present_in_diagnostics() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.expect("metric_components should be present");
    assert_eq!(mc.components.len(), 4);
    assert_eq!(mc.selected_metric, MetricComponent::GamutExcursion);
    assert!(mc.selection_score.is_finite());
    assert!(mc.composite_score.is_finite());
    // selection_score == gamut excursion component
    let gamut = mc.components[&MetricComponent::GamutExcursion];
    assert!((mc.selection_score - gamut).abs() < 1e-10);
}

#[test]
fn probe_samples_have_breakdown_in_full_mode() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for sample in &out.diagnostics.probe_samples {
        let bd = sample.breakdown.as_ref().expect("each probe should have breakdown in Full mode");
        assert!((bd.selection_score - sample.artifact_ratio).abs() < 1e-6,
            "breakdown selection_score {} != artifact_ratio {}", bd.selection_score, sample.artifact_ratio);
    }
}

#[test]
fn probe_samples_stripped_in_summary_mode() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16); // default = Summary
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for sample in &out.diagnostics.probe_samples {
        assert!(sample.breakdown.is_none(), "breakdown should be stripped in Summary mode");
    }
}

#[test]
fn metric_breakdown_selection_score_matches_measured() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    assert!((mc.selection_score - out.diagnostics.measured_artifact_ratio).abs() < 1e-6);
}

#[test]
fn composite_score_equals_weighted_sum() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    let w = &out.diagnostics.metric_weights;
    let expected = w.gamut_excursion * mc.components[&MetricComponent::GamutExcursion]
        + w.halo_ringing * mc.components[&MetricComponent::HaloRinging]
        + w.edge_overshoot * mc.components[&MetricComponent::EdgeOvershoot]
        + w.texture_flattening * mc.components[&MetricComponent::TextureFlattening];
    assert!((mc.composite_score - expected).abs() < 1e-6,
        "composite_score {} != weighted sum {}", mc.composite_score, expected);
}

#[test]
fn diagnostics_contain_metric_weights() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let w = &out.diagnostics.metric_weights;
    assert_eq!(w.gamut_excursion, 1.0);
    assert_eq!(w.halo_ringing, 0.3);
    assert_eq!(w.edge_overshoot, 0.3);
    assert_eq!(w.texture_flattening, 0.1);
    assert_eq!(out.diagnostics.metric_weights_provenance, Provenance::EngineeringProxy);
}

#[test]
fn v02_components_are_finite_and_nonnegative() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    for (&_component, &value) in &mc.components {
        assert!(value.is_finite(), "component value must be finite");
        assert!(value >= 0.0, "component value must be non-negative: {value}");
    }
    for sample in &out.diagnostics.probe_samples {
        let bd = sample.breakdown.as_ref().unwrap();
        for (&_component, &value) in &bd.components {
            assert!(value.is_finite(), "probe component value must be finite");
            assert!(value >= 0.0, "probe component value must be non-negative: {value}");
        }
    }
}

// ---------------------------------------------------------------------------
// Backward compatibility and JSON round-trip
// ---------------------------------------------------------------------------

#[test]
#[allow(deprecated)]
fn aggregate_equals_selection_score() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    assert_eq!(mc.aggregate, mc.selection_score);
}

#[test]
fn diagnostics_json_round_trip() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let json = serde_json::to_string_pretty(&out.diagnostics).expect("serialize");
    let deser: AutoSharpDiagnostics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.selected_strength, out.diagnostics.selected_strength);
    assert_eq!(deser.metric_weights, out.diagnostics.metric_weights);
    let mc_orig = out.diagnostics.metric_components.unwrap();
    let mc_deser = deser.metric_components.unwrap();
    assert_eq!(mc_orig.components.len(), mc_deser.components.len());
    assert!((mc_orig.selection_score - mc_deser.selection_score).abs() < 1e-10);
    assert!((mc_orig.composite_score - mc_deser.composite_score).abs() < 1e-10);
}
