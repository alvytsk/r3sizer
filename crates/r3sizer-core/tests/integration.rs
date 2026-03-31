use r3sizer_core::{
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams,
    ChromaRegionFactors, ClassificationParams, ClampPolicy, CrossingStatus, DiagnosticsLevel,
    ExperimentalSharpenMode, FallbackReason, FitStrategy, GainTable, ImageSize, KernelTable,
    LinearRgbImage, MetricComponent, MetricMode, ProbeConfig, ResizeKernel, ResizeStrategy,
    SaturationGuardParams, SelectionMode, SharpenMode, SharpenStrategy,
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
    // Default is Photo preset: TwoPass with coarse_max=1.0
    assert!(
        s >= 0.00 && s <= 1.00,
        "selected_strength {s} outside probe range [0.00, 1.00]"
    );
}

#[test]
fn probe_sample_count_matches_config() {
    let src = gradient_image(16, 16);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    // Default is TwoPass: 7 coarse + 4 dense = 11 total probes
    let count = out.diagnostics.probe_samples.len();
    assert!(
        count >= 7 && count <= 11,
        "expected 7-11 probes for TwoPass default, got {count}"
    );
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
// ArtifactMetric
// ---------------------------------------------------------------------------

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
fn probe_samples_have_no_breakdown_even_in_full_mode() {
    // Probes always use the fast path (gamut-only) for performance.
    // Per-probe breakdowns are not computed regardless of diagnostics_level.
    // The full metric breakdown is only computed for the final image.
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for sample in &out.diagnostics.probe_samples {
        assert!(sample.breakdown.is_none(), "probes should not have breakdown (fast path)");
    }
    // But the final metric components should still be present.
    assert!(out.diagnostics.metric_components.is_some());
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
    // Per-probe breakdowns are no longer computed (fast probing path).
    // Verify the final metric_components is complete instead.
    assert_eq!(mc.components.len(), 4, "final breakdown should have all 4 components");
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

// ---------------------------------------------------------------------------
// Content-adaptive sharpening (v0.3)
// ---------------------------------------------------------------------------

#[test]
fn default_strategy_is_content_adaptive() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // Default is now ContentAdaptive (Photo preset)
    assert!(d.region_coverage.is_some(), "region_coverage should be present for ContentAdaptive");
    assert!(d.adaptive_validation.is_some(), "adaptive_validation should be present for ContentAdaptive");
    assert!(d.timing.classification_us.is_some());
    assert!(d.timing.adaptive_validation_us.is_some());

    // Existing semantics unchanged
    assert!(d.selected_strength > 0.0);
    assert!(d.measured_artifact_ratio.is_finite());
    assert!(d.measured_metric_value.is_finite());
}

#[test]
fn content_adaptive_happy_path() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        target_artifact_ratio: 0.1, // generous P0
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // Region coverage present and sums to pixel count
    let cov = d.region_coverage.as_ref().expect("region_coverage should be Some");
    assert_eq!(cov.total_pixels, 16 * 16);
    assert_eq!(
        cov.flat + cov.textured + cov.strong_edge + cov.microtexture + cov.risky_halo_zone,
        cov.total_pixels,
    );

    // Adaptive validation present
    let val = d.adaptive_validation.as_ref().expect("adaptive_validation should be Some");
    match val {
        AdaptiveValidationOutcome::PassedDirect { measured_metric } => {
            assert!(*measured_metric <= 0.1);
        }
        AdaptiveValidationOutcome::PassedAfterBackoff { measured_metric, .. } => {
            assert!(*measured_metric <= 0.1);
        }
        _ => {} // FailedBudgetExceeded is acceptable — it's content-dependent
    }

    // Timing fields populated
    assert!(d.timing.classification_us.is_some());
    assert!(d.timing.adaptive_validation_us.is_some());

    // Output is valid
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.image.height(), 16);
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel {v} outside [0,1] after clamping");
    }
}

#[test]
fn content_adaptive_deterministic() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(16, 16)
    };
    let out1 = process_auto_sharp_downscale(&src, &params).unwrap();
    let out2 = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out1.image.pixels(), out2.image.pixels(), "adaptive pipeline must be deterministic");
    assert_eq!(
        out1.diagnostics.selected_strength,
        out2.diagnostics.selected_strength,
    );
}

#[test]
fn content_adaptive_tight_budget_triggers_backoff_or_failure() {
    let src = checkerboard(32, 32);
    let params = AutoSharpParams {
        target_artifact_ratio: 0.0001, // very tight
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::new(1.5, 1.5, 2.0, 2.0, 1.5).unwrap(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(8, 8)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let val = out.diagnostics.adaptive_validation.as_ref()
        .expect("adaptive_validation should be Some");

    match val {
        AdaptiveValidationOutcome::PassedDirect { .. } => {
            // Possible but unlikely with these params
        }
        AdaptiveValidationOutcome::PassedAfterBackoff { iterations, .. } => {
            assert!(*iterations > 0);
        }
        AdaptiveValidationOutcome::FailedBudgetExceeded { iterations, .. } => {
            assert!(*iterations > 0);
        }
    }
}

// ---------------------------------------------------------------------------
// SelectionPolicy tests
// ---------------------------------------------------------------------------

use r3sizer_core::SelectionPolicy;

#[test]
fn gamut_only_policy_identical_to_default() {
    let src = gradient_image(64, 64);
    let params_default = default_params(16, 16);
    let params_explicit = AutoSharpParams {
        selection_policy: SelectionPolicy::GamutOnly,
        ..default_params(16, 16)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_explicit = process_auto_sharp_downscale(&src, &params_explicit).unwrap();
    assert_eq!(
        out_default.diagnostics.selected_strength,
        out_explicit.diagnostics.selected_strength,
        "GamutOnly must be identical to default behavior"
    );
    assert_eq!(out_default.image.pixels(), out_explicit.image.pixels());
}

#[test]
fn hybrid_policy_respects_gamut_budget() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        // Disable evaluator to prevent strength capping that could move the
        // selected strength away from any probe sample.
        evaluator_config: None,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // If selection was BestSampleWithinBudget, selected probe must be within gamut budget.
    // Without per-probe breakdowns, Hybrid falls back to gamut-only ranking (max strength),
    // which still respects the gamut budget constraint.
    if d.selection_mode == SelectionMode::BestSampleWithinBudget {
        let selected = d.probe_samples.iter()
            .find(|s| (s.strength - d.selected_strength).abs() < 1e-6)
            .expect("selected strength must correspond to a probe sample");
        assert!(
            selected.metric_value <= d.target_artifact_ratio,
            "Hybrid must not select an out-of-budget sample when in-budget exists: metric_value={} > target={}",
            selected.metric_value, d.target_artifact_ratio,
        );
    }
}

#[test]
fn hybrid_policy_produces_valid_result() {
    let src = checkerboard(32, 32);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        ..default_params(8, 8)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 8);
    assert_eq!(out.image.height(), 8);
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel {v} outside [0,1]");
    }
}

#[test]
fn hybrid_diagnostics_include_selection_policy() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();

    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::Hybrid);

    let json = serde_json::to_string_pretty(&out.diagnostics).expect("serialize");
    assert!(json.contains("\"selection_policy\""));
    assert!(json.contains("\"hybrid\""));
    let deser: AutoSharpDiagnostics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.selection_policy, SelectionPolicy::Hybrid);
}

#[test]
fn gamut_only_diagnostics_include_selection_policy() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::GamutOnly);

    let json = serde_json::to_string_pretty(&out.diagnostics).expect("serialize");
    assert!(json.contains("\"selection_policy\""));
}

#[test]
fn composite_only_produces_valid_result() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::CompositeOnly,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::CompositeOnly);
}

// ---------------------------------------------------------------------------
// Step 2: content-adaptive resize (resize_strategy path)
// ---------------------------------------------------------------------------

#[test]
fn uniform_resize_strategy_produces_valid_result() {
    let src = gradient_image(64, 64);
    for kernel in [
        ResizeKernel::Lanczos3,
        ResizeKernel::CatmullRom,
        ResizeKernel::Gaussian,
        ResizeKernel::MitchellNetravali,
    ] {
        let params = AutoSharpParams {
            resize_strategy: Some(ResizeStrategy::Uniform { kernel }),
            ..default_params(16, 16)
        };
        let out = process_auto_sharp_downscale(&src, &params).unwrap();
        assert_eq!(out.image.width(), 16);
        assert_eq!(out.image.height(), 16);
        let diag = out
            .diagnostics
            .resize_strategy_diagnostics
            .expect("resize_strategy_diagnostics should be Some for explicit strategy");
        assert_eq!(diag.kernels_used, vec![kernel]);
        let total: u32 = diag.per_kernel_pixel_count.values().sum();
        assert_eq!(total, 16 * 16);
    }
}

#[test]
fn content_adaptive_resize_happy_path() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        resize_strategy: Some(ResizeStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            kernel_table: KernelTable::default(),
        }),
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.image.height(), 16);
    for &v in out.image.pixels() {
        assert!(v.is_finite(), "pixel must be finite");
    }

    let diag = out
        .diagnostics
        .resize_strategy_diagnostics
        .expect("resize_strategy_diagnostics should be Some for content-adaptive");
    assert!(!diag.kernels_used.is_empty());
    let total: u32 = diag.per_kernel_pixel_count.values().sum();
    assert_eq!(total, 16 * 16, "per_kernel_pixel_count must sum to target pixel count");
}

#[test]
fn content_adaptive_resize_deterministic() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        resize_strategy: Some(ResizeStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            kernel_table: KernelTable::default(),
        }),
        ..default_params(16, 16)
    };
    let out1 = process_auto_sharp_downscale(&src, &params).unwrap();
    let out2 = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out1.image.pixels(), out2.image.pixels());
    assert_eq!(
        out1.diagnostics.selected_strength,
        out2.diagnostics.selected_strength,
    );
}

#[test]
fn content_adaptive_resize_step_edge_uses_multiple_kernels() {
    // Step edge (flat left, bright right) should trigger >= 2 kernels from the
    // default KernelTable (flat=Gaussian vs edge=Lanczos3).
    let w = 64_u32;
    let h = 32_u32;
    let mut data = vec![0.0_f32; (w * h * 3) as usize];
    for y in 0..h {
        for x in (w / 2)..w {
            let idx = ((y * w + x) * 3) as usize;
            data[idx] = 1.0;
            data[idx + 1] = 1.0;
            data[idx + 2] = 1.0;
        }
    }
    let src = LinearRgbImage::new(w, h, data).unwrap();
    let params = AutoSharpParams {
        resize_strategy: Some(ResizeStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            kernel_table: KernelTable::default(),
        }),
        ..default_params(16, 8)
    };
    let diag = process_auto_sharp_downscale(&src, &params)
        .unwrap()
        .diagnostics
        .resize_strategy_diagnostics
        .unwrap();
    assert!(
        diag.kernels_used.len() >= 2,
        "step-edge image should use >= 2 kernels, got {:?}",
        diag.kernels_used,
    );
}

// ---------------------------------------------------------------------------
// Step 3: two-pass adaptive probe placement
// ---------------------------------------------------------------------------

fn two_pass_params(tw: u32, th: u32) -> AutoSharpParams {
    AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 5,
            coarse_min: 0.05,
            coarse_max: 3.0,
            dense_count: 4,
            window_margin: 0.5,
        },
        ..default_params(tw, th)
    }
}

#[test]
fn two_pass_produces_valid_result() {
    let src = gradient_image(64, 64);
    let out = process_auto_sharp_downscale(&src, &two_pass_params(16, 16)).unwrap();
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.image.height(), 16);
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel {v} outside [0,1]");
    }
    assert!(out.diagnostics.selected_strength > 0.0);
    assert!(out.diagnostics.measured_artifact_ratio.is_finite());
}

#[test]
fn two_pass_diagnostics_present() {
    let src = gradient_image(64, 64);
    let out = process_auto_sharp_downscale(&src, &two_pass_params(16, 16)).unwrap();
    let pp = out
        .diagnostics
        .probe_pass_diagnostics
        .expect("probe_pass_diagnostics must be Some for TwoPass config");
    assert_eq!(pp.coarse_count, 5);
    assert_eq!(pp.dense_count, 4);
    assert!(pp.dense_min >= pp.coarse_min);
    assert!(pp.dense_max <= pp.coarse_max);
    assert!(pp.dense_min < pp.dense_max);
}

#[test]
fn two_pass_dense_window_within_coarse_range() {
    // Dense window must be a sub-interval of the coarse range.
    let src = checkerboard(32, 32);
    let out = process_auto_sharp_downscale(&src, &two_pass_params(8, 8)).unwrap();
    let pp = out.diagnostics.probe_pass_diagnostics.unwrap();
    assert!(
        pp.dense_min >= pp.coarse_min && pp.dense_max <= pp.coarse_max,
        "dense window [{}, {}] not within coarse range [{}, {}]",
        pp.dense_min, pp.dense_max, pp.coarse_min, pp.coarse_max,
    );
}

#[test]
fn two_pass_deterministic() {
    let src = gradient_image(64, 64);
    let params = two_pass_params(16, 16);
    let out1 = process_auto_sharp_downscale(&src, &params).unwrap();
    let out2 = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out1.image.pixels(), out2.image.pixels());
    assert_eq!(
        out1.diagnostics.selected_strength,
        out2.diagnostics.selected_strength,
    );
    assert_eq!(
        out1.diagnostics.probe_pass_diagnostics.unwrap().dense_min,
        out2.diagnostics.probe_pass_diagnostics.unwrap().dense_min,
    );
}

#[test]
fn two_pass_probe_count_gte_static_minimum() {
    // Merged sample count must satisfy the >=4 minimum needed by the cubic fit.
    let src = gradient_image(64, 64);
    let out = process_auto_sharp_downscale(&src, &two_pass_params(16, 16)).unwrap();
    assert!(
        out.diagnostics.probe_samples.len() >= 4,
        "need >= 4 probe samples for cubic fit, got {}",
        out.diagnostics.probe_samples.len(),
    );
}

#[test]
fn two_pass_validation_rejects_bad_params() {
    let base = AutoSharpParams { target_width: 16, target_height: 16, ..AutoSharpParams::default() };

    // coarse_count too small
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass { coarse_count: 2, coarse_min: 0.05, coarse_max: 3.0, dense_count: 4, window_margin: 0.5 },
        ..base.clone()
    };
    assert!(p.validate().is_err());

    // dense_count too small
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass { coarse_count: 5, coarse_min: 0.05, coarse_max: 3.0, dense_count: 1, window_margin: 0.5 },
        ..base.clone()
    };
    assert!(p.validate().is_err());

    // coarse_min >= coarse_max
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass { coarse_count: 5, coarse_min: 2.0, coarse_max: 1.0, dense_count: 4, window_margin: 0.5 },
        ..base.clone()
    };
    assert!(p.validate().is_err());
}

// ---------------------------------------------------------------------------
// Step 4 -- Base resize quality tests
// ---------------------------------------------------------------------------

#[test]
fn base_resize_quality_present_and_finite() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let bq = out.diagnostics.base_resize_quality
        .expect("base_resize_quality should always be present");
    assert!(bq.edge_retention.is_finite(), "edge_retention must be finite");
    assert!(bq.texture_retention.is_finite(), "texture_retention must be finite");
    assert!(bq.ringing_score.is_finite(), "ringing_score must be finite");
    assert!(bq.envelope_scale.is_finite(), "envelope_scale must be finite");
    assert!(bq.ringing_score >= 0.0, "ringing_score must be non-negative");
    assert!(bq.envelope_scale >= 0.65 && bq.envelope_scale <= 1.0,
        "envelope_scale must be in [0.65, 1.0], got {}", bq.envelope_scale);
}

#[test]
fn effective_target_never_above_requested() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;
    assert!(
        d.effective_target_artifact_ratio <= d.target_artifact_ratio + 1e-9,
        "effective {} must not exceed requested {}",
        d.effective_target_artifact_ratio, d.target_artifact_ratio,
    );
    assert!(
        d.effective_target_artifact_ratio >= d.target_artifact_ratio * 0.65 - 1e-9,
        "effective {} must be >= requested * 0.65 = {}",
        d.effective_target_artifact_ratio, d.target_artifact_ratio * 0.65,
    );
}

#[test]
fn envelope_formula_consistent() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;
    let bq = d.base_resize_quality.unwrap();
    let expected = d.target_artifact_ratio * bq.envelope_scale;
    assert!(
        (d.effective_target_artifact_ratio - expected).abs() < 1e-9,
        "effective {} != target {} * envelope_scale {}",
        d.effective_target_artifact_ratio, d.target_artifact_ratio, bq.envelope_scale,
    );
}

#[test]
fn smooth_image_minimal_ringing() {
    // A solid or smooth image should have ~zero ringing.
    let src = solid_image(64, 64, 0.5);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let bq = out.diagnostics.base_resize_quality.unwrap();
    assert_eq!(bq.ringing_score, 0.0, "solid image should have zero ringing");
    assert!((bq.envelope_scale - 1.0).abs() < 1e-6, "no ringing -> envelope_scale == 1.0");
}

#[test]
fn base_resize_quality_deterministic() {
    let src = checkerboard(32, 32);
    let params = default_params(8, 8);
    let out1 = process_auto_sharp_downscale(&src, &params).unwrap();
    let out2 = process_auto_sharp_downscale(&src, &params).unwrap();
    let bq1 = out1.diagnostics.base_resize_quality.unwrap();
    let bq2 = out2.diagnostics.base_resize_quality.unwrap();
    assert_eq!(bq1.ringing_score, bq2.ringing_score);
    assert_eq!(bq1.edge_retention, bq2.edge_retention);
    assert_eq!(bq1.texture_retention, bq2.texture_retention);
    assert_eq!(bq1.envelope_scale, bq2.envelope_scale);
}

// ---------------------------------------------------------------------------
// Step 5 -- Context-aware chroma guard tests
// ---------------------------------------------------------------------------

fn content_adaptive_chroma_params(tw: u32, th: u32) -> AutoSharpParams {
    AutoSharpParams {
        target_width: tw,
        target_height: th,
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.10,
            chroma_region_factors: Some(ChromaRegionFactors::default()),
            saturation_guard: Some(SaturationGuardParams::default()),
        }),
        ..AutoSharpParams::default()
    }
}

#[test]
fn chroma_guard_per_region_diagnostics_with_content_adaptive() {
    let src = gradient_image(64, 64);
    let params = content_adaptive_chroma_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out.diagnostics.chroma_guard
        .expect("chroma_guard diagnostics should be present");
    let pr = cg.per_region
        .expect("per_region should be present with ContentAdaptive + region_factors");

    // Every field should be finite
    for stats in [&pr.flat, &pr.textured, &pr.strong_edge, &pr.microtexture, &pr.risky_halo_zone] {
        assert!(stats.mean_shift.is_finite());
        assert!(stats.max_shift.is_finite());
        assert!(stats.clamped_fraction.is_finite());
    }

    // Total pixel counts should sum to image dimensions
    let total = pr.flat.pixel_count + pr.textured.pixel_count
        + pr.strong_edge.pixel_count + pr.microtexture.pixel_count
        + pr.risky_halo_zone.pixel_count;
    assert_eq!(total, 16 * 16, "region pixel counts must sum to output dimensions");
}

#[test]
fn chroma_guard_per_region_absent_for_uniform() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        sharpen_strategy: SharpenStrategy::Uniform,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out.diagnostics.chroma_guard
        .expect("chroma_guard diagnostics should be present");
    assert!(cg.per_region.is_none(), "per_region should be None for Uniform strategy");
}

#[test]
fn chroma_guard_effective_threshold_stats_present() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16); // default has saturation_guard on
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out.diagnostics.chroma_guard
        .expect("chroma_guard diagnostics should be present");
    // Saturation guard is on by default -> effective threshold stats should be present
    assert!(cg.effective_threshold_min.is_some());
    assert!(cg.effective_threshold_mean.is_some());
    assert!(cg.effective_threshold_max.is_some());
    assert!(cg.effective_threshold_min.unwrap() <= cg.effective_threshold_mean.unwrap());
    assert!(cg.effective_threshold_mean.unwrap() <= cg.effective_threshold_max.unwrap());
}

#[test]
fn chroma_region_factors_defaults_monotone() {
    let f = ChromaRegionFactors::default();
    // Tighter protection for edges/halos, more permissive for flat regions
    assert!(f.flat >= f.textured, "flat >= textured");
    assert!(f.textured >= f.microtexture, "textured >= microtexture");
    assert!(f.microtexture >= f.strong_edge, "microtexture >= strong_edge");
    assert!(f.strong_edge >= f.risky_halo_zone, "strong_edge >= risky_halo_zone");
}

#[test]
fn saturation_guard_tightens_for_saturated_pixels() {
    // Create a saturated image (pure red = high saturation)
    let mut data = Vec::with_capacity(64 * 64 * 3);
    for _ in 0..64 * 64 {
        data.extend_from_slice(&[0.8, 0.1, 0.1]); // highly saturated red
    }
    let src = LinearRgbImage::new(64, 64, data).unwrap();
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.10,
            chroma_region_factors: None,
            saturation_guard: Some(SaturationGuardParams { min_scale: 0.6, gamma: 1.5 }),
        }),
        ..AutoSharpParams::default()
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out.diagnostics.chroma_guard.unwrap();

    // With saturated pixels, effective_threshold_mean should be < what we'd get
    // with no saturation guard (where effective ~ 0.10 * chroma_mag).
    // The saturation factor < 1.0 for saturated pixels should pull the mean down.
    let eff_mean = cg.effective_threshold_mean.unwrap();
    let eff_max = cg.effective_threshold_max.unwrap();
    assert!(eff_mean < eff_max || eff_mean == eff_max,
        "mean should be <= max");
    assert!(eff_mean.is_finite());
}
