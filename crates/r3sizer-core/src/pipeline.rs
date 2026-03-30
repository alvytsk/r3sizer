/// Pipeline orchestrator.
///
/// Coordinates all processing stages in the correct order:
///
/// 1. Validate parameters.
/// 2. Downscale to target size (linear space).
/// 3. Optional contrast leveling.
/// 4. Measure baseline artifact ratio (before sharpening).
/// 5. Probe multiple sharpening strengths, measure P(s).
/// 6. Fit cubic polynomial to probe samples.
/// 7. Solve P_hat(s*) = P0 (with fallback on failure).
/// 8. Apply final sharpening with s*.
/// 9. Measure actual artifact ratio on the final image.
/// 10. Apply clamp/normalize policy.
/// 11. Return result image + full diagnostics.
use web_time::Instant;

use crate::{
    classifier::{classify, gain_map_from_region_map},
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::{check_monotonicity, fit_cubic, fit_cubic_with_quality},
    metrics::channel_clipping_ratio,
    resize::downscale,
    sharpen::{make_kernel, unsharp_mask_with_kernel, unsharp_mask_single_channel_with_kernel},
    solve::{find_sharpness, find_sharpness_direct},
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams,
    ClampPolicy, FallbackReason, FitStatus, FitStrategy, ImageSize, LinearRgbImage,
    MetricMode, MetricWeights, ProbeConfig, ProbePassDiagnostics, ProbeSample, ProcessOutput,
    RegionCoverage, RobustnessFlags, SelectionMode, SharpenMode, SharpenStrategy,
    StageTiming, CoreError,
};

/// Pipeline-internal result of a sharpening step.
struct SharpenResult {
    image: LinearRgbImage,
    /// Luminance in linear domain, before clamp/transfer.
    /// Hard invariant: luminance.len() == image.width() * image.height()
    luminance: Vec<f32>,
}

/// Run the full automatic-sharpness downscale pipeline.
///
/// `input` should already be in linear RGB space (use `r3sizer-io` to load
/// and convert from a file).
///
/// # Errors
///
/// Returns `Err` for invalid parameters or (in pathological cases) when both
/// the cubic-root path and the probe-sample fallback have nothing to offer.
pub fn process_auto_sharp_downscale(
    input: &LinearRgbImage,
    params: &AutoSharpParams,
) -> Result<ProcessOutput, CoreError> {
    process_auto_sharp_downscale_with_progress(input, params, &|_| {})
}

/// Run the full pipeline with a progress callback invoked at each stage boundary.
///
/// The callback receives a short lowercase stage name such as `"resizing"`,
/// `"probing"`, or `"sharpening"`.  WASM callers use this to post progress
/// messages back to the main thread.
pub fn process_auto_sharp_downscale_with_progress(
    input: &LinearRgbImage,
    params: &AutoSharpParams,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    // -------------------------------------------------------------------
    // 1. Validate
    // -------------------------------------------------------------------
    on_stage("validating");
    params.validate()?;

    let pipeline_start = Instant::now();
    let input_size = input.size();
    let target = ImageSize { width: params.target_width, height: params.target_height };

    // -------------------------------------------------------------------
    // 1.5 (experimental) Input color-space ingress
    // -------------------------------------------------------------------
    let (input, _input_ingress_diag, _ingress_us) = {
        if let Some(cs) = params.input_color_space {
            let t0 = Instant::now();
            let (prepared, diag) = crate::color_space::prepare_input(input, cs)?;
            let us = t0.elapsed().as_micros() as u64;
            (std::borrow::Cow::Owned(prepared), Some(diag), Some(us))
        } else {
            (std::borrow::Cow::Borrowed(input), None, None)
        }
    };
    // -------------------------------------------------------------------
    // 2. Downscale in linear space
    // -------------------------------------------------------------------
    on_stage("resizing");
    let t0 = Instant::now();
    let (downscaled, _resize_strategy_diag) = {
        if let Some(ref strategy) = params.resize_strategy {
            let (img, diag) = crate::resize_strategy::downscale_with_strategy(&input, target, strategy)?;
            (img, Some(diag))
        } else {
            (downscale(&input, target)?, None)
        }
    };
    let resize_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 2.5. Base resize quality scoring (step 4)
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let base_resize_quality = crate::base_quality::score_base_resize(&input, &downscaled);
    let effective_p0 = params.target_artifact_ratio * base_resize_quality.envelope_scale;
    let base_quality_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 3. Optional contrast leveling
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let mut base = downscaled;
    let cl_params = ContrastLevelingParams { enabled: params.enable_contrast_leveling };
    apply_contrast_leveling(&mut base, &cl_params)?;
    let contrast_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 2.5. Region classification (ContentAdaptive only)
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let (gain_map, region_map, region_coverage, classification_us) =
        match &params.sharpen_strategy {
            SharpenStrategy::ContentAdaptive { classification, gain_table, .. } => {
                let rmap = classify(&base, classification);
                let gmap = gain_map_from_region_map(&rmap, gain_table);
                let cov = RegionCoverage::from_region_map(&rmap);
                let us = t0.elapsed().as_micros() as u64;
                (Some(gmap), Some(rmap), Some(cov), Some(us))
            }
            SharpenStrategy::Uniform => (None, None, None, None),
        };

    // -------------------------------------------------------------------
    // 4. Measure baseline artifact ratio (before any sharpening)
    // -------------------------------------------------------------------
    on_stage("baseline");
    let t0 = Instant::now();
    let measure = |img: &LinearRgbImage| -> f32 {
        if let Some(cs) = params.evaluation_color_space {
            return crate::chroma_guard::evaluate_in_color_space(img, cs);
        }
        match params.artifact_metric {
            ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
            ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
        }
    };
    let baseline_artifact_ratio = measure(&base);
    let baseline_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 5. Probe sharpening strengths
    // -------------------------------------------------------------------
    on_stage("probing");
    let t0 = Instant::now();

    // Build the Gaussian kernel once — sigma is constant across all probes.
    let kernel = make_kernel(params.sharpen_sigma)?;

    // Extract luminance once if using lightness mode (avoids recomputation per probe).
    let base_luminance = if matches!(params.sharpen_mode, SharpenMode::Lightness) {
        Some(color::extract_luminance(&base))
    } else {
        None
    };

    // Build metric override for experimental evaluation color space.
    let eval_cs_fn = params.evaluation_color_space.map(|cs| {
        move |img: &LinearRgbImage| -> f32 {
            crate::chroma_guard::evaluate_in_color_space(img, cs)
        }
    });
    let metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)> =
        eval_cs_fn.as_ref().map(|f| f as &(dyn Fn(&LinearRgbImage) -> f32 + Sync));

    // Dispatch: two-pass adaptive placement or static resolve.
    let (probe_samples, probe_pass_diagnostics) = match &params.probe_strengths {
        ProbeConfig::TwoPass {
            coarse_count, coarse_min, coarse_max, dense_count, window_margin,
        } => {
            run_two_pass_probing(
                *coarse_count, *coarse_min, *coarse_max,
                *dense_count, *window_margin,
                effective_p0,
                &base, base_luminance.as_deref(),
                params.sharpen_mode, params.metric_mode, params.artifact_metric,
                baseline_artifact_ratio, &kernel, &params.metric_weights, metric_override,
            )?
        }
        _ => {
            let strengths = params.probe_strengths.resolve()?;
            let samples = probe_strengths(
                &strengths,
                &base,
                base_luminance.as_deref(),
                params.sharpen_mode,
                params.metric_mode,
                params.artifact_metric,
                baseline_artifact_ratio,
                &kernel,
                &params.metric_weights,
                metric_override,
            )?;
            (samples, None)
        }
    };
    let probing_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 6–7. Fit + solve (or direct search)
    // -------------------------------------------------------------------
    let s_min = probe_samples.first().map(|s| s.strength as f64).unwrap_or(0.05);
    let s_max = probe_samples.last().map(|s| s.strength as f64).unwrap_or(3.0);
    let p0 = effective_p0 as f64;

    on_stage("fitting");

    // Build fit data: (strength, metric_value) pairs.
    // In RelativeToBase mode, prepend the known anchor (0.0, 0.0).
    let mut fit_data: Vec<(f64, f64)> = probe_samples
        .iter()
        .map(|ps| (ps.strength as f64, ps.metric_value as f64))
        .collect();
    if matches!(params.metric_mode, MetricMode::RelativeToBase) {
        fit_data.insert(0, (0.0, 0.0));
    }

    // Monotonicity check on probe samples.
    let (monotonic, quasi_monotonic) = check_monotonicity(&probe_samples);

    let t0 = Instant::now();
    let (solve_result, fit_status, fit_coefficients, fit_quality) = match params.fit_strategy {
        FitStrategy::DirectSearch => {
            let result = find_sharpness_direct(&probe_samples, effective_p0)?;
            (result, FitStatus::Skipped, None, None)
        }

        FitStrategy::Cubic => {
            match fit_cubic_with_quality(&fit_data) {
                Ok((poly, quality)) => {
                    let result =
                        find_sharpness(&poly, p0, s_min, s_max, &probe_samples)?;
                    (result, FitStatus::Success, Some(poly), Some(quality))
                }
                Err(fit_err) => {
                    let result = find_sharpness_direct(
                        &probe_samples,
                        effective_p0,
                    )?;
                    (
                        result,
                        FitStatus::Failed { reason: fit_err.to_string() },
                        None,
                        None,
                    )
                }
            }
        }
    };
    let fit_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // LOO stability check
    // -------------------------------------------------------------------
    on_stage("robustness");
    let t0 = Instant::now();
    let loo_stable;
    let max_loo_root_change;
    if fit_coefficients.is_some() {
        let primary_s = solve_result.selected_strength as f64;
        let (stable, max_change) =
            loo_stability(&fit_data, p0, s_min, s_max, primary_s);
        loo_stable = stable;
        max_loo_root_change = max_change;
    } else {
        loo_stable = true; // no fit → nothing to check
        max_loo_root_change = 0.0;
    }
    let robustness_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // Robustness flags
    // -------------------------------------------------------------------
    let r_squared_ok = fit_quality.is_none_or(|q| q.r_squared > 0.85);
    let well_conditioned = fit_quality.is_none_or(|q| q.min_pivot > 1e-8);

    let robustness = Some(RobustnessFlags {
        monotonic,
        quasi_monotonic,
        r_squared_ok,
        well_conditioned,
        loo_stable,
        max_loo_root_change,
    });

    // -------------------------------------------------------------------
    // Budget reachability
    // -------------------------------------------------------------------
    let budget_reachable_baseline = match params.metric_mode {
        MetricMode::AbsoluteTotal => baseline_artifact_ratio <= effective_p0,
        MetricMode::RelativeToBase => true, // by construction, relative starts at 0
    };
    let budget_reachable = budget_reachable_baseline
        && !matches!(solve_result.selection_mode, SelectionMode::LeastBadSample);

    // Override selection mode if budget is structurally unreachable due to baseline.
    let selection_mode = if !budget_reachable_baseline {
        SelectionMode::BudgetUnreachable
    } else {
        solve_result.selection_mode
    };

    // -------------------------------------------------------------------
    // Fallback reason
    // -------------------------------------------------------------------
    let fallback_reason = determine_fallback_reason(
        &selection_mode,
        &fit_status,
        budget_reachable_baseline,
        monotonic,
        loo_stable,
        params.fit_strategy,
        &solve_result.crossing_status,
    );

    // -------------------------------------------------------------------
    // 8. Final sharpening (strategy-dependent)
    // -------------------------------------------------------------------
    on_stage("sharpening");
    let t0 = Instant::now();
    let selected_strength = solve_result.selected_strength;

    // Experimental: chroma guard sharpening overrides the standard path.
    let _chroma_guard_diag;

    let (mut final_image, adaptive_validation, adaptive_validation_us) =
        match (&params.sharpen_strategy, &gain_map) {
            (SharpenStrategy::Uniform, _) | (_, None) => {
                let result = sharpen_image(
                    &base, base_luminance.as_deref(),
                    params.sharpen_mode,
                    selected_strength, &kernel,
                )?;
                (result.image, None, None)
            }
            (
                SharpenStrategy::ContentAdaptive {
                    max_backoff_iterations,
                    backoff_scale_factor,
                    ..
                },
                Some(gm),
            ) => {
                adaptive_sharpen_with_validation(
                    &base,
                    base_luminance.as_deref(),
                    params.sharpen_mode,
                    selected_strength,
                    gm,
                    params.sharpen_sigma,
                    effective_p0,
                    params.artifact_metric,
                    params.metric_mode,
                    baseline_artifact_ratio,
                    *max_backoff_iterations,
                    *backoff_scale_factor,
                )?
            }
        };
    // Experimental: if chroma guard is configured, apply it as an override.
    {
        if let Some(crate::types::ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift, chroma_region_factors, saturation_guard,
        }) = &params.experimental_sharpen_mode {
            let (guarded, cg_diag) = crate::chroma_guard::sharpen_with_chroma_guard(
                &base, selected_strength, params.sharpen_sigma, *max_chroma_shift,
                region_map.as_ref(),
                chroma_region_factors.as_ref(),
                saturation_guard.as_ref(),
            )?;
            final_image = guarded;
            _chroma_guard_diag = Some(cg_diag);
        } else {
            _chroma_guard_diag = None;
        }
    }
    let final_sharpen_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 9. Measure actual artifact ratio (pre-clamp)
    // -------------------------------------------------------------------
    let base_luma_for_metrics: Vec<f32> = match base_luminance.as_deref() {
        Some(l) => l.to_vec(),
        None => color::extract_luminance(&base),
    };
    let final_luma = color::extract_luminance(&final_image);
    let final_breakdown = crate::metrics::compute_metric_breakdown(
        &final_image,
        &base,
        &base_luma_for_metrics,
        &final_luma,
        params.artifact_metric,
        &params.metric_weights,
    );
    let measured_artifact_ratio = match metric_override {
        Some(f) => f(&final_image),
        None => final_breakdown.selection_score,
    };
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    // -------------------------------------------------------------------
    // 9.5 (experimental) Run quality evaluator
    // -------------------------------------------------------------------
    let (_evaluator_result, _evaluator_us) = {
        if let Some(ref eval_config) = params.evaluator_config {
            let t0 = Instant::now();
            let result = match eval_config {
                crate::types::EvaluatorConfig::Heuristic => {
                    let eval = crate::evaluator::HeuristicEvaluator;
                    crate::evaluator::QualityEvaluator::evaluate(&eval, &base, &final_image, selected_strength)
                }
            };
            let us = t0.elapsed().as_micros() as u64;
            (Some(result), Some(us))
        } else {
            (None, None)
        }
    };

    // -------------------------------------------------------------------
    // 10. Apply clamp policy
    // -------------------------------------------------------------------
    on_stage("finalizing");
    let t0 = Instant::now();
    match params.output_clamp {
        ClampPolicy::Clamp => {
            for v in final_image.pixels_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        ClampPolicy::Normalize => {
            let max_val = final_image
                .pixels()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            if max_val > 0.0 {
                for v in final_image.pixels_mut() {
                    // Divide by the positive maximum, then clamp negatives
                    // that can survive from sharpening ringing.  Without this
                    // clamp, negative values would produce NaN during the
                    // subsequent linear→sRGB pow(x, 1/2.4) conversion.
                    *v = (*v / max_val).max(0.0);
                }
            } else {
                // Every channel value is <= 0 (degenerate image).
                // Clamp to zero to avoid NaN in sRGB conversion.
                for v in final_image.pixels_mut() {
                    *v = 0.0;
                }
            }
        }
    }
    let clamp_us = t0.elapsed().as_micros() as u64;
    let total_us = pipeline_start.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 11. Return
    // -------------------------------------------------------------------
    // Strip per-probe breakdowns in Summary mode to reduce serialization size.
    let mut probe_samples = probe_samples;
    if matches!(params.diagnostics_level, crate::DiagnosticsLevel::Summary) {
        for sample in &mut probe_samples {
            sample.breakdown = None;
        }
    }

    let mut diagnostics = AutoSharpDiagnostics {
        input_size,
        output_size: target,
        sharpen_mode: params.sharpen_mode,
        metric_mode: params.metric_mode,
        artifact_metric: params.artifact_metric,
        target_artifact_ratio: params.target_artifact_ratio,
        baseline_artifact_ratio,
        probe_samples,
        fit_status,
        fit_coefficients,
        fit_quality,
        crossing_status: solve_result.crossing_status,
        robustness,
        selected_strength,
        selection_mode,
        fallback_reason,
        budget_reachable,
        measured_artifact_ratio,
        measured_metric_value,
        metric_components: Some(final_breakdown),
        metric_weights: params.metric_weights,
        region_coverage,
        adaptive_validation,
        timing: StageTiming {
            resize_us,
            contrast_us,
            baseline_us,
            probing_us,
            fit_us,
            robustness_us,
            final_sharpen_us,
            clamp_us,
            total_us,
            classification_us,
            adaptive_validation_us,
            ingress_us: _ingress_us,
            evaluator_us: _evaluator_us,
            base_quality_us: Some(base_quality_us),
        },
        input_ingress: _input_ingress_diag,
        resize_strategy_diagnostics: _resize_strategy_diag,
        chroma_guard: _chroma_guard_diag,
        evaluator_result: _evaluator_result,
        recommendations: Vec::new(),
        probe_pass_diagnostics,
        base_resize_quality: Some(base_resize_quality),
        effective_target_artifact_ratio: effective_p0,
    };

    diagnostics.recommendations =
        crate::recommendations::generate_recommendations(&diagnostics, params);

    Ok(ProcessOutput { image: final_image, diagnostics })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply sharpening to the base image using the configured mode, model, and a pre-built kernel.
///
/// Returns `SharpenResult` with the sharpened image and its luminance channel.
/// In lightness mode, luminance is the already-computed sharpened luma (no extra allocation).
/// In RGB mode, luminance is extracted from the result (one `Vec<f32>` per call).
fn sharpen_image(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    amount: f32,
    kernel: &[f32],
) -> Result<SharpenResult, CoreError> {
    match mode {
        SharpenMode::Rgb => {
            let image = unsharp_mask_with_kernel(base, amount, kernel);
            let luminance = color::extract_luminance(&image);
            Ok(SharpenResult { image, luminance })
        }
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;
            let sharpened_l = unsharp_mask_single_channel_with_kernel(lum, w, h, amount, kernel);
            let image = color::reconstruct_rgb_from_lightness(base, &sharpened_l);
            Ok(SharpenResult { image, luminance: sharpened_l })
        }
    }
}

/// Run all probe strengths and collect `ProbeSample`s.
///
/// Each probe allocates a temporary sharpened image (W × H × 3 × 4 bytes) plus
/// an equally-sized blur buffer inside the Gaussian pass.  In sequential mode
/// these are freed before the next probe starts; with the `parallel` feature
/// up to `rayon::current_num_threads()` images exist simultaneously.
#[allow(clippy::too_many_arguments)]
fn probe_strengths(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    weights: &MetricWeights,
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<Vec<ProbeSample>, CoreError> {
    // Base luminance for metric evaluation (needed even in RGB mode for v0.2 metrics).
    let base_luma_for_metrics: Vec<f32> = match base_luminance {
        Some(l) => l.to_vec(),
        None => color::extract_luminance(base),
    };

    let probe_one = |&s: &f32| -> Result<ProbeSample, CoreError> {
        let result = sharpen_image(base, base_luminance, sharpen_mode, s, kernel)?;
        let breakdown = crate::metrics::compute_metric_breakdown(
            &result.image,
            base,
            &base_luma_for_metrics,
            &result.luminance,
            artifact_metric,
            weights,
        );
        let p_total = match metric_override {
            Some(f) => f(&result.image),
            None => breakdown.selection_score,
        };
        let metric_value = compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
        Ok(ProbeSample { strength: s, artifact_ratio: p_total, metric_value, breakdown: Some(breakdown) })
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        strengths.par_iter().map(probe_one).collect()
    }

    #[cfg(not(feature = "parallel"))]
    {
        strengths.iter().map(probe_one).collect()
    }
}

/// Compute the metric value used for fitting, based on the configured mode.
#[inline]
fn compute_metric_value(
    p_total: f32,
    baseline: f32,
    mode: MetricMode,
) -> f32 {
    match mode {
        MetricMode::AbsoluteTotal => p_total,
        MetricMode::RelativeToBase => (p_total - baseline).max(0.0),
    }
}

// ---------------------------------------------------------------------------
// Leave-one-out stability check
// ---------------------------------------------------------------------------

/// Refit the cubic N times (dropping one data point each time), re-solve for
/// the root, and report the maximum relative change vs the primary root.
///
/// Returns `(stable, max_relative_change)`.  `stable` is true when
/// `max_relative_change < LOO_THRESHOLD`.
fn loo_stability(
    fit_data: &[(f64, f64)],
    p0: f64,
    s_min: f64,
    s_max: f64,
    primary_s: f64,
) -> (bool, f64) {
    const LOO_THRESHOLD: f64 = 0.25; // 25% relative change

    if fit_data.len() < 5 {
        // Not enough data for meaningful LOO (need >=4 after dropping one).
        return (true, 0.0);
    }

    let mut max_change = 0.0f64;
    for skip in 0..fit_data.len() {
        let subset: Vec<(f64, f64)> = fit_data
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, &v)| v)
            .collect();

        if let Ok(poly) = fit_cubic(&subset) {
            // Find the largest root in range for this refit.
            if let Ok(result) = find_sharpness(
                &poly, p0, s_min, s_max,
                &[], // empty samples — force polynomial-only path or no fallback
            ) {
                if matches!(result.selection_mode, SelectionMode::PolynomialRoot) {
                    let loo_s = result.selected_strength as f64;
                    let change = if primary_s.abs() > 1e-10 {
                        ((loo_s - primary_s) / primary_s).abs()
                    } else {
                        (loo_s - primary_s).abs()
                    };
                    if change > max_change {
                        max_change = change;
                    }
                }
            }
        }
    }

    (max_change < LOO_THRESHOLD, max_change)
}

// ---------------------------------------------------------------------------
// Fallback reason determination
// ---------------------------------------------------------------------------

/// Determine why the pipeline used a fallback instead of the polynomial root.
///
/// Returns `None` when `selection_mode == PolynomialRoot` (no fallback).
#[allow(clippy::too_many_arguments)]
fn determine_fallback_reason(
    selection_mode: &SelectionMode,
    fit_status: &FitStatus,
    budget_reachable_baseline: bool,
    monotonic: bool,
    loo_stable: bool,
    fit_strategy: FitStrategy,
    crossing_status: &crate::CrossingStatus,
) -> Option<FallbackReason> {
    if *selection_mode == SelectionMode::PolynomialRoot {
        return None;
    }

    // Priority order: most severe reason first.
    if !budget_reachable_baseline {
        return Some(FallbackReason::BudgetTooStrictForContent);
    }
    if matches!(fit_strategy, FitStrategy::DirectSearch) {
        return Some(FallbackReason::DirectSearchConfigured);
    }
    if matches!(fit_status, FitStatus::Failed { .. }) {
        return Some(FallbackReason::FitFailed);
    }
    if !monotonic {
        return Some(FallbackReason::MetricNonMonotonic);
    }
    if !loo_stable {
        return Some(FallbackReason::FitUnstable);
    }
    if matches!(crossing_status, crate::CrossingStatus::NotFoundInRange) {
        return Some(FallbackReason::RootOutOfRange);
    }

    // Catch-all for edge cases (shouldn't normally happen).
    Some(FallbackReason::RootOutOfRange)
}

// ---------------------------------------------------------------------------
// Adaptive sharpen + validate + backoff (Stage 9 + 9.5)
// ---------------------------------------------------------------------------

/// Adaptive sharpen with validation and backoff loop.
///
/// Computes detail buffers once, then applies adaptive sharpening. If the
/// result exceeds the artifact budget P0, iteratively reduces the global scale
/// factor until budget is met or max iterations reached.
///
/// Returns `(final_image, validation_outcome, validation_time_us)`.
#[allow(clippy::too_many_arguments)]
fn adaptive_sharpen_with_validation(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    global_strength: f32,
    gain_map: &crate::GainMap,
    sigma: f32,
    target_p0: f32,
    artifact_metric: ArtifactMetric,
    metric_mode: MetricMode,
    baseline_artifact_ratio: f32,
    max_backoff: u8,
    backoff_factor: f32,
) -> Result<(LinearRgbImage, Option<AdaptiveValidationOutcome>, Option<u64>), CoreError> {
    let w = base.width() as usize;
    let h = base.height() as usize;

    let measure = |img: &LinearRgbImage| -> f32 {
        let raw = match artifact_metric {
            ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
            ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
        };
        compute_metric_value(raw, baseline_artifact_ratio, metric_mode)
    };

    match sharpen_mode {
        SharpenMode::Lightness => {
            let luma = base_luminance.expect("luminance required for lightness mode");
            let kernel = make_kernel(sigma)?;
            let blurred = crate::sharpen::gaussian_blur_single_channel(luma, w, h, &kernel);
            let detail: Vec<f32> = luma.iter().zip(blurred.iter())
                .map(|(&l, &b)| l - b).collect();

            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let sharpened_l = crate::sharpen::apply_adaptive_lightness_from_detail(
                    luma, &detail, global_strength * scale, gain_map,
                );
                crate::color::reconstruct_rgb_from_lightness(base, &sharpened_l)
            };

            // Initial apply at scale=1.0
            let result = apply_at_scale(1.0);
            let p = measure(&result);

            let t_val = Instant::now();

            if p <= target_p0 {
                let val_us = t_val.elapsed().as_micros() as u64;
                return Ok((
                    result,
                    Some(AdaptiveValidationOutcome::PassedDirect { measured_metric: p }),
                    Some(val_us),
                ));
            }

            // Backoff loop
            let mut best_scale = 1.0_f32;
            let mut best_metric = p;
            let mut best_result = result;
            let mut scale = 1.0_f32;

            for i in 1..=max_backoff {
                scale *= backoff_factor;
                let result = apply_at_scale(scale);
                let p = measure(&result);

                if p < best_metric {
                    best_metric = p;
                    best_scale = scale;
                    best_result = result;
                }

                if p <= target_p0 {
                    let val_us = t_val.elapsed().as_micros() as u64;
                    return Ok((
                        best_result,
                        Some(AdaptiveValidationOutcome::PassedAfterBackoff {
                            iterations: i,
                            final_scale: scale,
                            measured_metric: p,
                        }),
                        Some(val_us),
                    ));
                }
            }

            let val_us = t_val.elapsed().as_micros() as u64;
            Ok((
                best_result,
                Some(AdaptiveValidationOutcome::FailedBudgetExceeded {
                    iterations: max_backoff,
                    best_scale,
                    best_metric,
                }),
                Some(val_us),
            ))
        }

        SharpenMode::Rgb => {
            let kernel = make_kernel(sigma)?;
            let blurred = crate::sharpen::gaussian_blur(base, &kernel);
            let src_px = base.pixels();
            let blur_px = blurred.pixels();
            let gain_data = gain_map.data();

            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let eff_strength = global_strength * scale;
                let out: Vec<f32> = src_px.chunks_exact(3)
                    .zip(blur_px.chunks_exact(3))
                    .zip(gain_data.iter())
                    .flat_map(|((s, b), &g)| {
                        let eff = eff_strength * g;
                        [
                            s[0] + eff * (s[0] - b[0]),
                            s[1] + eff * (s[1] - b[1]),
                            s[2] + eff * (s[2] - b[2]),
                        ]
                    })
                    .collect();
                LinearRgbImage::new(base.width(), base.height(), out).unwrap()
            };

            let result = apply_at_scale(1.0);
            let p = measure(&result);

            let t_val = Instant::now();

            if p <= target_p0 {
                let val_us = t_val.elapsed().as_micros() as u64;
                return Ok((
                    result,
                    Some(AdaptiveValidationOutcome::PassedDirect { measured_metric: p }),
                    Some(val_us),
                ));
            }

            let mut best_scale = 1.0_f32;
            let mut best_metric = p;
            let mut best_result = result;
            let mut scale = 1.0_f32;

            for i in 1..=max_backoff {
                scale *= backoff_factor;
                let result = apply_at_scale(scale);
                let p = measure(&result);

                if p < best_metric {
                    best_metric = p;
                    best_scale = scale;
                    best_result = result;
                }

                if p <= target_p0 {
                    let val_us = t_val.elapsed().as_micros() as u64;
                    return Ok((
                        best_result,
                        Some(AdaptiveValidationOutcome::PassedAfterBackoff {
                            iterations: i,
                            final_scale: scale,
                            measured_metric: p,
                        }),
                        Some(val_us),
                    ));
                }
            }

            let val_us = t_val.elapsed().as_micros() as u64;
            Ok((
                best_result,
                Some(AdaptiveValidationOutcome::FailedBudgetExceeded {
                    iterations: max_backoff,
                    best_scale,
                    best_metric,
                }),
                Some(val_us),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Convert sRGB-encoded LinearRgbImage to/from linear for pipeline callers
// who manage colour conversion outside the pipeline.
// ---------------------------------------------------------------------------

/// Convenience: convert an sRGB-encoded image (loaded from a file) to linear
/// RGB in place, ready for the pipeline.
pub fn to_linear_inplace(img: &mut LinearRgbImage) {
    color::image_srgb_to_linear(img);
}

/// Convenience: convert a linear RGB image back to sRGB in place, ready for
/// file encoding.
pub fn to_srgb_inplace(img: &mut LinearRgbImage) {
    color::image_linear_to_srgb(img);
}

// ---------------------------------------------------------------------------
// Two-pass adaptive probe placement (step 3)
// ---------------------------------------------------------------------------

/// Build `count` evenly-spaced values in `[lo, hi]`.
fn linspace(lo: f32, hi: f32, count: usize) -> Vec<f32> {
    if count == 0 {
        return vec![];
    }
    if count == 1 {
        return vec![lo];
    }
    (0..count)
        .map(|i| lo + (hi - lo) * (i as f32) / ((count - 1) as f32))
        .collect()
}

/// Given coarse probe results, find the dense window that brackets the P0 crossing.
///
/// Scans for the first upward crossing of `p0` in `metric_value` order.
/// Extends the crossing interval by `margin` on each side, clamped to
/// `[global_min, global_max]`.
///
/// Falls back to the upper or lower 30% of the range when no crossing is found.
fn find_dense_window(
    samples: &[ProbeSample],
    p0: f32,
    global_min: f32,
    global_max: f32,
    margin: f32,
) -> (f32, f32) {
    // Find first interval where metric_value crosses p0 upward.
    for w in samples.windows(2) {
        if w[0].metric_value <= p0 && w[1].metric_value > p0 {
            let interval = (w[1].strength - w[0].strength).max(1e-6);
            let pad = interval * margin;
            let lo = (w[0].strength - pad).max(global_min);
            let hi = (w[1].strength + pad).min(global_max);
            return (lo, hi);
        }
    }

    let span = ((global_max - global_min) * 0.3).max(1e-4);
    if samples.last().is_some_and(|s| s.metric_value <= p0) {
        // All probes under budget: concentrate at the upper end.
        let lo = (global_max - span).max(global_min);
        (lo, global_max)
    } else {
        // All probes over budget (or empty): concentrate at the lower end.
        let hi = (global_min + span).min(global_max);
        (global_min, hi)
    }
}

/// Two-pass probing: coarse scan over `[coarse_min, coarse_max]`, then dense
/// probing in a refined window around the estimated P0 crossing.
///
/// Returns all samples (coarse + dense, sorted by strength, near-duplicates
/// removed) plus a `ProbePassDiagnostics` record.
#[allow(clippy::too_many_arguments)]
fn run_two_pass_probing(
    coarse_count: usize,
    coarse_min: f32,
    coarse_max: f32,
    dense_count: usize,
    window_margin: f32,
    p0: f32,
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    weights: &MetricWeights,
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<(Vec<ProbeSample>, Option<ProbePassDiagnostics>), CoreError> {
    // Phase 1: coarse
    let coarse_strengths = linspace(coarse_min, coarse_max, coarse_count);
    let coarse_samples = probe_strengths(
        &coarse_strengths, base, base_luminance, sharpen_mode, metric_mode,
        artifact_metric, baseline_artifact_ratio, kernel, weights, metric_override,
    )?;

    // Locate dense window from coarse results
    let (dense_lo, dense_hi) =
        find_dense_window(&coarse_samples, p0, coarse_min, coarse_max, window_margin);

    // Phase 2: dense
    let dense_strengths = linspace(dense_lo, dense_hi, dense_count);
    let dense_samples = probe_strengths(
        &dense_strengths, base, base_luminance, sharpen_mode, metric_mode,
        artifact_metric, baseline_artifact_ratio, kernel, weights, metric_override,
    )?;

    // Merge, sort, remove near-duplicates (within 1e-5 of each other)
    let mut all: Vec<ProbeSample> = coarse_samples;
    all.extend(dense_samples);
    all.sort_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap_or(std::cmp::Ordering::Equal));
    all.dedup_by(|a, b| (a.strength - b.strength).abs() < 1e-5);

    let diag = ProbePassDiagnostics {
        coarse_count,
        coarse_min,
        coarse_max,
        dense_count,
        dense_min: dense_lo,
        dense_max: dense_hi,
    };
    Ok((all, Some(diag)))
}
