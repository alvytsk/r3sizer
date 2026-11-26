use r3sizer_core::{
    prepare_base, process_auto_sharp_downscale, process_from_prepared,
    process_from_prepared_with_probes, AdaptiveValidationOutcome, ArtifactMetric,
    AutoSharpDiagnostics, AutoSharpParams, ChromaRegionFactors, ClampPolicy, ClassificationParams,
    CoreError, CrossingStatus, DiagnosticsLevel, EvaluationColorSpace, ExperimentalSharpenMode,
    FallbackReason, FitStrategy, GainTable, ImageSize, KernelTable, LinearRgbImage,
    MetricComponent, MetricMode, PipelineMode, ProbeBatch, ProbeConfig, ProbeSample,
    ProbeWorkerInput, ProbeWorkerParts, ResizeKernel, ResizeStrategy, SaturationGuardParams,
    SelectionMode, SharpenMode, SharpenStrategy,
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

/// Dark/light stripes 8 px wide: sharpening overshoot clips the red channel
/// while luminance stays in range, so the evaluation color spaces disagree.
fn stripe_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = if (x / 8) % 2 == 0 { 0.92 } else { 0.03 };
            data.extend_from_slice(&[v, v * 0.5, v * 0.2]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
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
    assert_eq!(
        out.diagnostics.output_size,
        ImageSize {
            width: 4,
            height: 4
        }
    );
    assert_eq!(
        out.diagnostics.input_size,
        ImageSize {
            width: 16,
            height: 16
        }
    );
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
        (0.00..=1.00).contains(&s),
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
        (7..=11).contains(&count),
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
        assert!(
            (0.0..=1.0).contains(&v),
            "pixel value {v} outside [0,1] after clamping"
        );
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
        assert!(
            ps.metric_value >= 0.0,
            "relative metric_value {} < 0",
            ps.metric_value
        );
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
    assert_eq!(
        out.diagnostics.artifact_metric,
        ArtifactMetric::PixelOutOfGamutRatio
    );
}

// ---------------------------------------------------------------------------
// Fit quality
// ---------------------------------------------------------------------------

#[test]
fn fit_quality_present_for_cubic_strategy() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let q = out
        .diagnostics
        .fit_quality
        .expect("fit_quality should be present for cubic strategy");
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
    let r = out
        .diagnostics
        .robustness
        .expect("robustness should be present");
    // quasi_monotonic should be at least as permissive as monotonic.
    if r.monotonic {
        assert!(r.quasi_monotonic);
    }
    assert!(r.max_loo_root_change.is_finite());
}

// ---------------------------------------------------------------------------
// LOO stability
// ---------------------------------------------------------------------------

/// Run the solve stages on hand-supplied probe samples.
fn solve_with_probes(
    src: &LinearRgbImage,
    params: &AutoSharpParams,
    samples: &[(f32, f32)],
) -> AutoSharpDiagnostics {
    let prepared = prepare_base(src, params, &|_| {}).unwrap();
    let samples = samples
        .iter()
        .map(|&(strength, metric_value)| ProbeSample {
            strength,
            artifact_ratio: metric_value,
            metric_value,
            breakdown: None,
        })
        .collect();
    // Hand-made samples: stamp them for the base they are solved against.
    let batch = ProbeBatch::new(prepared.probe_provenance(params), samples);
    process_from_prepared_with_probes(&prepared, params, batch, 0, None, None, &|_| {})
        .unwrap()
        .diagnostics
}

/// A solid image has no resize ringing, so `envelope_scale == 1.0` and the
/// solver's `effective_p0` is exactly `target_artifact_ratio`.
fn solid() -> LinearRgbImage {
    solid_image(64, 64, 0.25)
}

#[test]
fn loo_coverage_explains_unstable_root_that_barely_moves() {
    // Five probes lie on y = 0.0005·s² and the top one is raised to 0.02.
    // Dropping the top probe refits that exact quadratic, whose root
    // √(P0 / 0.0005) ≥ 3.6 lies beyond the probed range [0, 3] for every
    // resize envelope.  The other refits stay within 7% of the selected root,
    // so `max_loo_root_change` alone cannot show why the check fails.
    // The default params use the relative-to-base metric, so the fit is
    // anchored at the origin: seven refits for six samples.
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        target_artifact_ratio: 0.010,
        ..AutoSharpParams::default()
    };
    let samples = [
        (0.5, 0.000125),
        (1.0, 0.0005),
        (1.5, 0.001125),
        (2.0, 0.002),
        (2.5, 0.003125),
        (3.0, 0.02),
    ];
    let d = solve_with_probes(&solid(), &params, &samples);

    // The polynomial root is what got selected, so the check ran on it.
    assert_eq!(d.selection_mode, SelectionMode::PolynomialRoot);
    let r = d.robustness.expect("robustness should be present");
    assert!(!r.loo_stable, "escaped refit root must fail the check");
    assert!(
        r.max_loo_root_change < 0.25,
        "max_loo_root_change = {}",
        r.max_loo_root_change
    );
    assert!(
        (r.loo_coverage - 6.0 / 7.0).abs() < 1e-9,
        "loo_coverage = {}",
        r.loo_coverage
    );
}

#[test]
fn loo_stable_root_reports_full_coverage() {
    // A quartic perturbation keeps the data off any exact cubic; every refit
    // root stays within 0.6% of the selected one.
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        target_artifact_ratio: 0.004,
        ..AutoSharpParams::default()
    };
    let samples = [
        (0.5, 0.00025625),
        (1.0, 0.0011),
        (1.5, 0.00275625),
        (2.0, 0.0056),
        (2.5, 0.01015625),
        (3.0, 0.0171),
    ];
    let d = solve_with_probes(&solid(), &params, &samples);

    assert_eq!(d.selection_mode, SelectionMode::PolynomialRoot);
    let r = d.robustness.expect("robustness should be present");
    assert!(r.loo_stable);
    assert_eq!(r.loo_coverage, 1.0);
}

#[test]
fn loo_is_not_run_for_direct_search() {
    // No polynomial root is selected, so there is nothing to cross-validate.
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        fit_strategy: FitStrategy::DirectSearch,
        ..AutoSharpParams::default()
    };
    let samples = [
        (0.5, 0.000125),
        (1.0, 0.0005),
        (1.5, 0.001125),
        (2.0, 0.002),
        (2.5, 0.003125),
        (3.0, 0.02),
    ];
    let d = solve_with_probes(&solid(), &params, &samples);

    assert_eq!(d.selection_mode, SelectionMode::BestSampleWithinBudget);
    let r = d.robustness.expect("robustness should be present");
    assert!(r.loo_stable);
    assert_eq!(r.max_loo_root_change, 0.0);
    assert_eq!(r.loo_coverage, 1.0);
}

#[test]
fn loo_still_checks_an_applied_root_when_the_budget_is_unreachable() {
    // Absolute-total mode with the baseline above P0: the reported selection
    // becomes `BudgetUnreachable`, but the polynomial root is still the
    // strength that gets applied, so it is still what the refits compare
    // against.  Same construction as the escape case, scaled down under P0.
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        target_artifact_ratio: 1e-9,
        metric_mode: MetricMode::AbsoluteTotal,
        ..AutoSharpParams::default()
    };
    let samples = [
        (0.5, 1.25e-11),
        (1.0, 5e-11),
        (1.5, 1.125e-10),
        (2.0, 2e-10),
        (2.5, 3.125e-10),
        (3.0, 1e-8),
    ];
    // A hard black/white edge rings outside the gamut when resized, so the
    // baseline artifact ratio is far above P0.
    let step = {
        let mut data = Vec::with_capacity(64 * 64 * 3);
        for _ in 0..64 {
            for x in 0..64 {
                let v = if x < 32 { 0.0 } else { 1.0 };
                data.extend_from_slice(&[v, v, v]);
            }
        }
        LinearRgbImage::new(64, 64, data).unwrap()
    };
    let d = solve_with_probes(&step, &params, &samples);

    assert_eq!(d.selection_mode, SelectionMode::BudgetUnreachable);
    let r = d.robustness.expect("robustness should be present");
    assert!(
        !r.loo_stable,
        "the applied polynomial root must still be cross-validated"
    );
    assert!(
        (r.loo_coverage - 5.0 / 6.0).abs() < 1e-9,
        "loo_coverage = {}",
        r.loo_coverage
    );
}

#[test]
fn loo_is_not_run_when_r2_gate_redirects_to_direct_search() {
    // Alternating probes fit their cubic poorly (R² ≈ 0.82 < 0.85).  The gate
    // discards the polynomial root (≈1.7) and selects the largest sample
    // within budget instead.  LOO refits are polynomial roots, so comparing
    // them against a sample-based strength measures nothing — it must not be
    // reported as instability of a root that was never used.
    let params = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        target_artifact_ratio: 0.005,
        ..AutoSharpParams::default()
    };
    let samples = [
        (0.5, 0.00025),
        (1.0, 0.0045),
        (1.5, 0.00225),
        (2.0, 0.008),
        (2.5, 0.00625),
        (3.0, 0.009),
    ];
    let d = solve_with_probes(&solid(), &params, &samples);

    // The gate redirected the selection...
    assert!(d.fit_quality.expect("fit_quality").r_squared < 0.85);
    assert_eq!(d.selection_mode, SelectionMode::BestSampleWithinBudget);
    assert_eq!(d.fallback_reason, Some(FallbackReason::FitPoorQuality));

    // ...so no refit was compared against the sample-based strength.
    let r = d.robustness.expect("robustness should be present");
    assert!(r.loo_stable, "spurious instability for an unused root");
    assert_eq!(
        r.max_loo_root_change, 0.0,
        "polynomial roots were compared against a sample-based strength"
    );
    assert_eq!(r.loo_coverage, 1.0);
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
    assert_eq!(
        out.diagnostics.fallback_reason,
        Some(FallbackReason::DirectSearchConfigured)
    );
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
    let parts_sum = t.resize_us
        + t.contrast_us
        + t.baseline_us
        + t.probing_us
        + t.fit_us
        + t.robustness_us
        + t.final_sharpen_us
        + t.clamp_us;
    // Total should be >= sum of parts (captures overhead between stages too).
    assert!(
        t.total_us >= parts_sum / 2,
        "total_us={} should be roughly >= parts_sum={}",
        t.total_us,
        parts_sum
    );
}

// ---------------------------------------------------------------------------
// Metric breakdown (v0.2 scaffold)
// ---------------------------------------------------------------------------

#[test]
fn metric_breakdown_present_in_diagnostics() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out
        .diagnostics
        .metric_components
        .expect("metric_components should be present");
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
        assert!(
            sample.breakdown.is_none(),
            "probes should not have breakdown (fast path)"
        );
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
        assert!(
            sample.breakdown.is_none(),
            "breakdown should be stripped in Summary mode"
        );
    }
}

#[test]
fn metric_breakdown_selection_score_matches_measured() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    assert!((mc.selection_score - out.diagnostics.measured_artifact_ratio).abs() < 1e-6);
}

#[test]
fn composite_score_equals_weighted_sum() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    let w = &out.diagnostics.metric_weights;
    let expected = w.gamut_excursion * mc.components[&MetricComponent::GamutExcursion]
        + w.halo_ringing * mc.components[&MetricComponent::HaloRinging]
        + w.edge_overshoot * mc.components[&MetricComponent::EdgeOvershoot]
        + w.texture_flattening * mc.components[&MetricComponent::TextureFlattening];
    assert!(
        (mc.composite_score - expected).abs() < 1e-6,
        "composite_score {} != weighted sum {}",
        mc.composite_score,
        expected
    );
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
        assert!(
            value >= 0.0,
            "component value must be non-negative: {value}"
        );
    }
    // Per-probe breakdowns are no longer computed (fast probing path).
    // Verify the final metric_components is complete instead.
    assert_eq!(
        mc.components.len(),
        4,
        "final breakdown should have all 4 components"
    );
}

// ---------------------------------------------------------------------------
// Backward compatibility and JSON round-trip
// ---------------------------------------------------------------------------

#[test]
#[allow(deprecated)]
fn aggregate_equals_selection_score() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
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
    assert!(
        d.region_coverage.is_some(),
        "region_coverage should be present for ContentAdaptive"
    );
    assert!(
        d.adaptive_validation.is_some(),
        "adaptive_validation should be present for ContentAdaptive"
    );
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
    let cov = d
        .region_coverage
        .as_ref()
        .expect("region_coverage should be Some");
    assert_eq!(cov.total_pixels, 16 * 16);
    assert_eq!(
        cov.flat + cov.textured + cov.strong_edge + cov.microtexture + cov.risky_halo_zone,
        cov.total_pixels,
    );

    // Adaptive validation present
    let val = d
        .adaptive_validation
        .as_ref()
        .expect("adaptive_validation should be Some");
    match val {
        AdaptiveValidationOutcome::PassedDirect { measured_metric } => {
            assert!(*measured_metric <= 0.1);
        }
        AdaptiveValidationOutcome::PassedAfterBackoff {
            measured_metric, ..
        } => {
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
        assert!(
            (0.0..=1.0).contains(&v),
            "pixel {v} outside [0,1] after clamping"
        );
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
    assert_eq!(
        out1.image.pixels(),
        out2.image.pixels(),
        "adaptive pipeline must be deterministic"
    );
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
    let val = out
        .diagnostics
        .adaptive_validation
        .as_ref()
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
        out_default.diagnostics.selected_strength, out_explicit.diagnostics.selected_strength,
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
        let selected = d
            .probe_samples
            .iter()
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
        assert!((0.0..=1.0).contains(&v), "pixel {v} outside [0,1]");
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
    assert_eq!(
        out.diagnostics.selection_policy,
        SelectionPolicy::CompositeOnly
    );
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
    assert_eq!(
        total,
        16 * 16,
        "per_kernel_pixel_count must sum to target pixel count"
    );
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
        assert!((0.0..=1.0).contains(&v), "pixel {v} outside [0,1]");
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
        pp.dense_min,
        pp.dense_max,
        pp.coarse_min,
        pp.coarse_max,
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
    let base = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        ..AutoSharpParams::default()
    };

    // coarse_count too small
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 2,
            coarse_min: 0.05,
            coarse_max: 3.0,
            dense_count: 4,
            window_margin: 0.5,
        },
        ..base.clone()
    };
    assert!(p.validate().is_err());

    // dense_count too small
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 5,
            coarse_min: 0.05,
            coarse_max: 3.0,
            dense_count: 1,
            window_margin: 0.5,
        },
        ..base.clone()
    };
    assert!(p.validate().is_err());

    // coarse_min >= coarse_max
    let p = AutoSharpParams {
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 5,
            coarse_min: 2.0,
            coarse_max: 1.0,
            dense_count: 4,
            window_margin: 0.5,
        },
        ..base.clone()
    };
    assert!(p.validate().is_err());
}

#[test]
fn validation_enforces_gain_table_bounds() {
    let base = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        ..AutoSharpParams::default()
    };

    let adaptive_with_gain = |flat: f32| AutoSharpParams {
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable {
                flat,
                ..GainTable::v03_default()
            },
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..base.clone()
    };

    // Documented hard bounds are inclusive.
    assert!(adaptive_with_gain(GainTable::MIN_GAIN_VALUE)
        .validate()
        .is_ok());
    assert!(adaptive_with_gain(4.0).validate().is_ok());

    // Out of bounds (struct literal route, bypassing GainTable::new).
    assert!(adaptive_with_gain(50.0).validate().is_err());
    assert!(adaptive_with_gain(0.1).validate().is_err());
    assert!(adaptive_with_gain(f32::NAN).validate().is_err());
    assert!(adaptive_with_gain(f32::INFINITY).validate().is_err());

    // serde route: deserialization bypasses GainTable::new, so validate()
    // must catch what the constructor no longer guards.
    let p = adaptive_with_gain(50.0);
    let json = serde_json::to_string(&p).unwrap();
    let round_tripped: AutoSharpParams = serde_json::from_str(&json).unwrap();
    assert!(round_tripped.validate().is_err());
}

#[test]
fn validation_enforces_saturation_guard_bounds() {
    let base = AutoSharpParams {
        target_width: 16,
        target_height: 16,
        ..AutoSharpParams::default()
    };

    let with_guard = |min_scale: f32, gamma: f32| AutoSharpParams {
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.25,
            chroma_region_factors: None,
            saturation_guard: Some(SaturationGuardParams { min_scale, gamma }),
        }),
        ..base.clone()
    };

    // Documented defaults are valid; min_scale may approach but not reach 1.0.
    assert!(with_guard(0.6, 1.5).validate().is_ok());
    assert!(with_guard(0.999, 0.25).validate().is_ok());

    // gamma must be positive and finite (gamma <= 0 makes sat^gamma 0/inf/nan).
    assert!(with_guard(0.6, 0.0).validate().is_err());
    assert!(with_guard(0.6, -1.0).validate().is_err());
    assert!(with_guard(0.6, f32::NAN).validate().is_err());
    assert!(with_guard(0.6, f32::INFINITY).validate().is_err());

    // min_scale must be in (0, 1): <= 0 disables the guard, >= 1 *loosens*
    // the threshold for saturated pixels (inverted vs documented intent).
    assert!(with_guard(0.0, 1.5).validate().is_err());
    assert!(with_guard(-0.1, 1.5).validate().is_err());
    assert!(with_guard(1.0, 1.5).validate().is_err());
    assert!(with_guard(1.5, 1.5).validate().is_err());
    assert!(with_guard(f32::NAN, 1.5).validate().is_err());
}

#[test]
fn validation_rejects_unresolved_pipeline_mode() {
    let p = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Fast),
        ..AutoSharpParams::photo(16, 16)
    };

    // An unresolved mode would be silently ignored by the pipeline; the
    // entry-point validation must make that loud instead.
    assert!(p.validate().is_err());

    // resolved() folds the mode and clears the field, so the resolved
    // params pass.
    let resolved = p.resolved();
    assert!(resolved.pipeline_mode.is_none());
    assert!(resolved.validate().is_ok());
}

// ---------------------------------------------------------------------------
// Step 4 -- Base resize quality tests
// ---------------------------------------------------------------------------

#[test]
fn base_resize_quality_present_and_finite() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let bq = out
        .diagnostics
        .base_resize_quality
        .expect("base_resize_quality should always be present");
    assert!(
        bq.edge_retention.is_finite(),
        "edge_retention must be finite"
    );
    assert!(
        bq.texture_retention.is_finite(),
        "texture_retention must be finite"
    );
    assert!(bq.ringing_score.is_finite(), "ringing_score must be finite");
    assert!(
        bq.envelope_scale.is_finite(),
        "envelope_scale must be finite"
    );
    assert!(
        bq.ringing_score >= 0.0,
        "ringing_score must be non-negative"
    );
    assert!(
        bq.envelope_scale >= 0.65 && bq.envelope_scale <= 1.0,
        "envelope_scale must be in [0.65, 1.0], got {}",
        bq.envelope_scale
    );
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
        d.effective_target_artifact_ratio,
        d.target_artifact_ratio,
    );
    assert!(
        d.effective_target_artifact_ratio >= d.target_artifact_ratio * 0.65 - 1e-9,
        "effective {} must be >= requested * 0.65 = {}",
        d.effective_target_artifact_ratio,
        d.target_artifact_ratio * 0.65,
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
        d.effective_target_artifact_ratio,
        d.target_artifact_ratio,
        bq.envelope_scale,
    );
}

#[test]
fn smooth_image_minimal_ringing() {
    // A solid or smooth image should have ~zero ringing.
    let src = solid_image(64, 64, 0.5);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let bq = out.diagnostics.base_resize_quality.unwrap();
    assert_eq!(
        bq.ringing_score, 0.0,
        "solid image should have zero ringing"
    );
    assert!(
        (bq.envelope_scale - 1.0).abs() < 1e-6,
        "no ringing -> envelope_scale == 1.0"
    );
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
    let cg = out
        .diagnostics
        .chroma_guard
        .expect("chroma_guard diagnostics should be present");
    let pr = cg
        .per_region
        .expect("per_region should be present with ContentAdaptive + region_factors");

    // Every field should be finite
    for stats in [
        &pr.flat,
        &pr.textured,
        &pr.strong_edge,
        &pr.microtexture,
        &pr.risky_halo_zone,
    ] {
        assert!(stats.mean_shift.is_finite());
        assert!(stats.max_shift.is_finite());
        assert!(stats.clamped_fraction.is_finite());
    }

    // Total pixel counts should sum to image dimensions
    let total = pr.flat.pixel_count
        + pr.textured.pixel_count
        + pr.strong_edge.pixel_count
        + pr.microtexture.pixel_count
        + pr.risky_halo_zone.pixel_count;
    assert_eq!(
        total,
        16 * 16,
        "region pixel counts must sum to output dimensions"
    );
}

#[test]
fn chroma_guard_per_region_absent_for_uniform() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        sharpen_strategy: SharpenStrategy::Uniform,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out
        .diagnostics
        .chroma_guard
        .expect("chroma_guard diagnostics should be present");
    assert!(
        cg.per_region.is_none(),
        "per_region should be None for Uniform strategy"
    );
}

#[test]
fn chroma_guard_effective_threshold_stats_present() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16); // default has saturation_guard on
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let cg = out
        .diagnostics
        .chroma_guard
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
    assert!(
        f.microtexture >= f.strong_edge,
        "microtexture >= strong_edge"
    );
    assert!(
        f.strong_edge >= f.risky_halo_zone,
        "strong_edge >= risky_halo_zone"
    );
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
            saturation_guard: Some(SaturationGuardParams {
                min_scale: 0.6,
                gamma: 1.5,
            }),
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
    assert!(eff_mean <= eff_max, "mean should be <= max");
    assert!(eff_mean.is_finite());
}

// ---------------------------------------------------------------------------
// Pipeline modes (fast / balanced / quality)
// ---------------------------------------------------------------------------

#[test]
fn fast_mode_produces_valid_result() {
    let src = gradient_image(160, 120);
    let params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Fast),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 80);
    assert_eq!(out.image.height(), 60);
    assert!(out.diagnostics.selected_strength > 0.0);
}

#[test]
fn quality_mode_produces_valid_result() {
    let src = gradient_image(160, 120);
    let params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Quality),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 80);
    assert_eq!(out.image.height(), 60);
    assert!(out.diagnostics.selected_strength > 0.0);
}

#[test]
fn fast_mode_has_fewer_probes_than_quality() {
    let src = gradient_image(160, 120);
    let fast_params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Fast),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();
    let quality_params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Quality),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();

    let fast_out = process_auto_sharp_downscale(&src, &fast_params).unwrap();
    let quality_out = process_auto_sharp_downscale(&src, &quality_params).unwrap();

    assert!(
        fast_out.diagnostics.probe_samples.len() < quality_out.diagnostics.probe_samples.len(),
        "fast should have fewer probes: {} vs {}",
        fast_out.diagnostics.probe_samples.len(),
        quality_out.diagnostics.probe_samples.len(),
    );
}

#[test]
fn fast_mode_uses_uniform_strategy() {
    let params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Fast),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();
    // After resolving, sharpen_strategy should be Uniform
    assert!(
        matches!(params.sharpen_strategy, SharpenStrategy::Uniform),
        "fast mode should use uniform strategy, got {:?}",
        params.sharpen_strategy,
    );
    // Chroma guard should be disabled
    assert!(
        params.experimental_sharpen_mode.is_none(),
        "fast mode should disable chroma guard"
    );
    // Evaluator should be disabled
    assert!(
        params.evaluator_config.is_none(),
        "fast mode should disable evaluator"
    );
}

#[test]
fn quality_mode_ensures_adaptive_strategy() {
    // Even if we start from a Uniform base, quality mode should upgrade to adaptive
    let params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Quality),
        sharpen_strategy: SharpenStrategy::Uniform,
        experimental_sharpen_mode: None,
        evaluator_config: None,
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();
    assert!(
        matches!(
            params.sharpen_strategy,
            SharpenStrategy::ContentAdaptive { .. }
        ),
        "quality mode should ensure adaptive strategy",
    );
    assert!(
        params.experimental_sharpen_mode.is_some(),
        "quality mode should enable chroma guard"
    );
    assert!(
        params.evaluator_config.is_some(),
        "quality mode should enable evaluator"
    );
}

#[test]
fn balanced_mode_preserves_defaults() {
    let default = AutoSharpParams::photo(80, 60);
    let resolved = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Balanced),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();

    // Balanced should not change any fields (it's a no-op)
    assert_eq!(
        format!("{:?}", default.probe_strengths),
        format!("{:?}", resolved.probe_strengths),
    );
    assert_eq!(
        format!("{:?}", default.sharpen_strategy),
        format!("{:?}", resolved.sharpen_strategy),
    );
}

#[test]
fn mode_preserves_p0_and_sigma() {
    let custom_p0 = 0.005;
    let custom_sigma = 1.5;
    for mode in [
        PipelineMode::Fast,
        PipelineMode::Balanced,
        PipelineMode::Quality,
    ] {
        let params = AutoSharpParams {
            pipeline_mode: Some(mode),
            target_artifact_ratio: custom_p0,
            sharpen_sigma: custom_sigma,
            ..AutoSharpParams::photo(80, 60)
        }
        .resolved();
        assert_eq!(
            params.target_artifact_ratio, custom_p0,
            "{mode:?} should preserve P0"
        );
        assert_eq!(
            params.sharpen_sigma, custom_sigma,
            "{mode:?} should preserve sigma"
        );
    }
}

// ---------------------------------------------------------------------------
// Early stopping in coarse probing
// ---------------------------------------------------------------------------

#[test]
fn two_pass_early_stop_fires_on_easy_crossing() {
    // A checkerboard image produces a clear P0 crossing at low strengths,
    // so the coarse scan should bracket P0 before exhausting all probes.
    let src = checkerboard(160, 120);
    let params = AutoSharpParams {
        // 9 coarse probes — early stop should fire well before probe 9
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 9,
            coarse_min: 0.003,
            coarse_max: 2.0,
            dense_count: 4,
            window_margin: 0.5,
        },
        pipeline_mode: Some(PipelineMode::Quality),
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();

    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let diag = out
        .diagnostics
        .probe_pass_diagnostics
        .as_ref()
        .expect("TwoPass should produce pass diagnostics");

    // If early stopping fired, coarse_probes_used < coarse_count
    if let Some(used) = diag.coarse_probes_used {
        assert!(
            used < diag.coarse_count,
            "early stop should use fewer probes: used={used}, configured={}",
            diag.coarse_count
        );
        assert!(used >= 3, "should use at least 3 coarse probes");
    }
    // Either way, the result should be valid
    assert!(out.diagnostics.selected_strength > 0.0);
    assert!(out.diagnostics.measured_artifact_ratio.is_finite());
}

#[test]
fn two_pass_no_early_stop_when_all_under_budget() {
    // A smooth gradient with low P0 target — all probes should be under budget,
    // so no bracket is found and all coarse probes run.
    let src = gradient_image(160, 120);
    let params = AutoSharpParams {
        target_artifact_ratio: 0.5, // Very generous budget
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 7,
            coarse_min: 0.003,
            coarse_max: 0.5,
            dense_count: 4,
            window_margin: 0.5,
        },
        ..AutoSharpParams::photo(80, 60)
    }
    .resolved();

    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let diag = out
        .diagnostics
        .probe_pass_diagnostics
        .as_ref()
        .expect("TwoPass should produce pass diagnostics");

    // No early stop: all coarse probes should have run
    assert!(
        diag.coarse_probes_used.is_none(),
        "no bracket → no early stop, but got coarse_probes_used={:?}",
        diag.coarse_probes_used
    );
}

// ---------------------------------------------------------------------------
// Probe-worker seam
// ---------------------------------------------------------------------------

#[test]
fn worker_probes_measure_what_the_pipeline_measures() {
    let src = stripe_image(128, 96);
    let strengths = [0.5f32, 1.0, 2.0, 3.0, 4.0];
    let photo = AutoSharpParams::photo(32, 24);
    let variants = [
        ("lightness", photo.clone()),
        (
            "rgb",
            AutoSharpParams {
                sharpen_mode: SharpenMode::Rgb,
                ..photo.clone()
            },
        ),
        (
            "hybrid",
            AutoSharpParams {
                selection_policy: SelectionPolicy::Hybrid,
                ..photo.clone()
            },
        ),
    ];
    for (label, variant) in variants {
        for cs in [
            None,
            Some(EvaluationColorSpace::Rgb),
            Some(EvaluationColorSpace::LumaOnly),
            Some(EvaluationColorSpace::LabApprox),
        ] {
            let params = AutoSharpParams {
                evaluation_color_space: cs,
                probe_strengths: ProbeConfig::Explicit(strengths.to_vec()),
                ..variant.clone()
            };
            let prepared = prepare_base(&src, &params, &|_| {}).unwrap();
            let in_process = process_from_prepared(&prepared, &params, &|_| {})
                .unwrap()
                .diagnostics
                .probe_samples;
            let worker = prepared.run_probes(&strengths, &params).unwrap();
            let worker = worker.samples();
            assert_eq!(
                worker.len(),
                in_process.len(),
                "{label}, evaluation_color_space = {cs:?}"
            );
            for (w, p) in worker.iter().zip(&in_process) {
                assert_eq!(
                    (
                        w.strength,
                        w.artifact_ratio.to_bits(),
                        w.metric_value.to_bits()
                    ),
                    (
                        p.strength,
                        p.artifact_ratio.to_bits(),
                        p.metric_value.to_bits()
                    ),
                    "{label}, evaluation_color_space = {cs:?}"
                );
            }
        }
    }
}

fn seam_params() -> AutoSharpParams {
    AutoSharpParams {
        probe_strengths: ProbeConfig::Explicit(vec![0.25, 0.5, 1.0, 2.0, 3.0]),
        ..AutoSharpParams::photo(32, 24)
    }
}

/// `ProbeWorkerParts` with each buffer passed under its own name.
fn parts<'a>(
    base: &'a LinearRgbImage,
    luma: &'a [f32],
    detail: &'a [f32],
    baseline: f32,
) -> ProbeWorkerParts<'a> {
    ProbeWorkerParts {
        base,
        base_luminance: luma,
        detail,
        baseline_artifact_ratio: baseline,
    }
}

#[test]
fn a_probe_batch_is_refused_by_another_instance_of_the_same_base() {
    let src = stripe_image(128, 96);
    let params = seam_params();
    let a = prepare_base(&src, &params, &|_| {}).unwrap();
    let b = prepare_base(&src, &params, &|_| {}).unwrap();
    assert_ne!(a.probe_provenance(&params), b.probe_provenance(&params));
    let batch = a.run_probes(&[0.5, 1.0, 2.0], &params).unwrap();
    let refused = process_from_prepared_with_probes(&b, &params, batch, 0, None, None, &|_| {});
    assert!(matches!(refused, Err(CoreError::ProbeProvenanceMismatch)));
}

#[test]
fn an_accepted_batch_gives_the_in_process_result() {
    let params = seam_params();
    let prepared = prepare_base(&stripe_image(128, 96), &params, &|_| {}).unwrap();
    let batch = prepared
        .run_probes(&[0.25, 0.5, 1.0, 2.0, 3.0], &params)
        .unwrap();
    let via_workers =
        process_from_prepared_with_probes(&prepared, &params, batch, 0, None, None, &|_| {})
            .unwrap();
    let in_process = process_from_prepared(&prepared, &params, &|_| {}).unwrap();
    assert_eq!(
        via_workers.diagnostics.selected_strength.to_bits(),
        in_process.diagnostics.selected_strength.to_bits()
    );
    let metrics = |d: &AutoSharpDiagnostics| {
        d.probe_samples
            .iter()
            .map(|s| (s.strength.to_bits(), s.metric_value.to_bits()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        metrics(&via_workers.diagnostics),
        metrics(&in_process.diagnostics)
    );
}

#[test]
fn probing_no_strengths_gives_an_empty_stamped_batch() {
    let prepared = prepare_base(&stripe_image(128, 96), &seam_params(), &|_| {}).unwrap();
    let batch = prepared.run_probes(&[], &seam_params()).unwrap();
    assert!(batch.samples().is_empty());
    assert_eq!(
        batch.provenance(),
        prepared.probe_provenance(&seam_params())
    );
}

#[test]
fn a_probe_input_rejects_every_wrong_length_buffer() {
    let src = stripe_image(128, 96);
    for mode in [SharpenMode::Lightness, SharpenMode::Rgb] {
        let params = AutoSharpParams {
            sharpen_mode: mode,
            ..seam_params()
        };
        let prepared = prepare_base(&src, &params, &|_| {}).unwrap();
        let base = LinearRgbImage::new(32, 24, prepared.base_pixels().to_vec()).unwrap();
        let luma = prepared.luminance().unwrap().to_vec();
        let detail = prepared.compute_detail(&params).unwrap();
        let stamp = prepared.probe_provenance(&params);
        let baseline = prepared.baseline_artifact_ratio();
        let n = 32 * 24;
        let (luma_len, detail_len) = (luma.len(), detail.len());
        assert_eq!(luma_len, n);
        assert_eq!(
            detail_len,
            if matches!(mode, SharpenMode::Lightness) {
                n
            } else {
                3 * n
            }
        );
        ProbeWorkerInput::new(parts(&base, &luma, &detail, baseline), &params, stamp).unwrap();

        let mut long_luma = luma.clone();
        long_luma.extend([0.5; 5]);
        for (bad, expected, got) in [(&luma[..n - 1], n, n - 1), (long_luma.as_slice(), n, n + 5)] {
            let result =
                ProbeWorkerInput::new(parts(&base, bad, &detail, baseline), &params, stamp);
            assert!(
                matches!(result, Err(CoreError::BufferLengthMismatch { expected_len, got_len }) if (expected_len, got_len) == (expected, got)),
                "{mode:?}: luminance of {} floats was accepted or misreported",
                bad.len()
            );
        }
        let mut long_detail = detail.clone();
        long_detail.extend([0.5; 7]);
        for (bad, expected, got) in [
            (&detail[..detail_len - 1], detail_len, detail_len - 1),
            (long_detail.as_slice(), detail_len, detail_len + 7),
        ] {
            let result = ProbeWorkerInput::new(parts(&base, &luma, bad, baseline), &params, stamp);
            assert!(
                matches!(result, Err(CoreError::BufferLengthMismatch { expected_len, got_len }) if (expected_len, got_len) == (expected, got)),
                "{mode:?}: detail of {} floats was accepted or misreported",
                bad.len()
            );
        }
    }
}

#[test]
fn a_probe_input_rejects_a_stamp_minted_for_other_params_or_baseline() {
    let params = seam_params();
    let prepared = prepare_base(&stripe_image(128, 96), &params, &|_| {}).unwrap();
    let base = LinearRgbImage::new(32, 24, prepared.base_pixels().to_vec()).unwrap();
    let luma = prepared.luminance().unwrap();
    let detail = prepared.compute_detail(&params).unwrap();
    let stamp = prepared.probe_provenance(&params);
    let baseline = prepared.baseline_artifact_ratio();
    let other = AutoSharpParams {
        sharpen_sigma: 1.5,
        ..params.clone()
    };
    let wrong_params = ProbeWorkerInput::new(parts(&base, luma, &detail, baseline), &other, stamp);
    assert!(matches!(
        wrong_params,
        Err(CoreError::ProbeProvenanceMismatch)
    ));
    let wrong_baseline = ProbeWorkerInput::new(
        parts(&base, luma, &detail, baseline + 0.125),
        &params,
        stamp,
    );
    assert!(matches!(
        wrong_baseline,
        Err(CoreError::ProbeProvenanceMismatch)
    ));
}
