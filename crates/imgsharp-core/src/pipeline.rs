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
use std::time::Instant;

use crate::{
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::{check_monotonicity, fit_cubic, fit_cubic_with_quality},
    metrics::channel_clipping_ratio,
    resize::downscale,
    sharpen::{make_kernel, unsharp_mask_with_kernel, unsharp_mask_single_channel_with_kernel},
    solve::{find_sharpness, find_sharpness_direct},
    ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FallbackReason,
    FitStatus, FitStrategy, ImageSize, LinearRgbImage, MetricMode, ProbeSample,
    ProcessOutput, Provenance, RobustnessFlags, SelectionMode, SharpenMode, SharpenModel,
    StageTiming, StageProvenance, CoreError,
};

/// Run the full automatic-sharpness downscale pipeline.
///
/// `input` should already be in linear RGB space (use `imgsharp-io` to load
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
    // -------------------------------------------------------------------
    // 1. Validate
    // -------------------------------------------------------------------
    params.validate()?;

    let pipeline_start = Instant::now();
    let input_size = input.size();
    let target = ImageSize { width: params.target_width, height: params.target_height };

    // -------------------------------------------------------------------
    // 2. Downscale in linear space
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let downscaled = downscale(input, target)?;
    let resize_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 3. Optional contrast leveling
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let mut base = downscaled;
    let cl_params = ContrastLevelingParams { enabled: params.enable_contrast_leveling };
    apply_contrast_leveling(&mut base, &cl_params)?;
    let contrast_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 4. Measure baseline artifact ratio (before any sharpening)
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let measure = |img: &LinearRgbImage| -> f32 {
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
    let t0 = Instant::now();
    let strengths = params.probe_strengths.resolve()?;

    // Build the Gaussian kernel once — sigma is constant across all probes.
    let kernel = make_kernel(params.sharpen_sigma)?;

    // Extract luminance once if using lightness mode (avoids recomputation per probe).
    let base_luminance = if matches!(params.sharpen_mode, SharpenMode::Lightness) {
        Some(color::extract_luminance(&base))
    } else {
        None
    };

    let probe_samples = probe_strengths(
        &strengths,
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        params.sharpen_model,
        params.metric_mode,
        params.artifact_metric,
        baseline_artifact_ratio,
        &kernel,
    )?;
    let probing_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 6–7. Fit + solve (or direct search)
    // -------------------------------------------------------------------
    let s_min = strengths.first().copied().unwrap_or(0.05) as f64;
    let s_max = strengths.last().copied().unwrap_or(3.0) as f64;
    let p0 = params.target_artifact_ratio as f64;

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
            let result = find_sharpness_direct(&probe_samples, params.target_artifact_ratio)?;
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
                        params.target_artifact_ratio,
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
        MetricMode::AbsoluteTotal => baseline_artifact_ratio <= params.target_artifact_ratio,
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
    // 8. Final sharpening
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let selected_strength = solve_result.selected_strength;
    let mut final_image = sharpen_image(
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        params.sharpen_model,
        selected_strength,
        &kernel,
    )?;
    let final_sharpen_us = t0.elapsed().as_micros() as u64;

    // -------------------------------------------------------------------
    // 9. Measure actual artifact ratio (pre-clamp)
    // -------------------------------------------------------------------
    let final_breakdown = crate::metrics::compute_metric_breakdown(&final_image, params.artifact_metric);
    let measured_artifact_ratio = final_breakdown.aggregate;
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    // -------------------------------------------------------------------
    // 10. Apply clamp policy
    // -------------------------------------------------------------------
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
    let diagnostics = AutoSharpDiagnostics {
        input_size,
        output_size: target,
        sharpen_mode: params.sharpen_mode,
        sharpen_model: params.sharpen_model,
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
        },
        provenance: StageProvenance {
            color_conversion: Provenance::PaperConfirmed,
            resize: Provenance::EngineeringChoice,
            contrast_leveling: if params.enable_contrast_leveling {
                Provenance::Placeholder
            } else {
                Provenance::PaperConfirmed
            },
            sharpen_operator: match params.sharpen_model {
                SharpenModel::PracticalUsm => Provenance::EngineeringChoice,
                SharpenModel::PaperLightnessApprox => Provenance::PaperSupported,
            },
            lightness_reconstruction: match params.sharpen_mode {
                SharpenMode::Lightness => Provenance::PaperSupported,
                SharpenMode::Rgb => Provenance::PaperConfirmed,
            },
            artifact_metric: Provenance::EngineeringProxy,
            polynomial_fit: Provenance::PaperConfirmed,
        },
    };

    Ok(ProcessOutput { image: final_image, diagnostics })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply sharpening to the base image using the configured mode, model, and a pre-built kernel.
fn sharpen_image(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    model: SharpenModel,
    amount: f32,
    kernel: &[f32],
) -> Result<LinearRgbImage, CoreError> {
    match mode {
        SharpenMode::Rgb => Ok(unsharp_mask_with_kernel(base, amount, kernel)),
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;
            let sharpened_l = match model {
                SharpenModel::PracticalUsm => {
                    unsharp_mask_single_channel_with_kernel(lum, w, h, amount, kernel)
                }
                SharpenModel::PaperLightnessApprox => {
                    crate::paper_sharpen::paper_sharpen_lightness(lum, w, h, amount, kernel)
                }
            };
            Ok(color::reconstruct_rgb_from_lightness(base, &sharpened_l))
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
    sharpen_model: SharpenModel,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
) -> Result<Vec<ProbeSample>, CoreError> {
    let probe_one = |&s: &f32| -> Result<ProbeSample, CoreError> {
        let sharpened = sharpen_image(base, base_luminance, sharpen_mode, sharpen_model, s, kernel)?;
        let breakdown = crate::metrics::compute_metric_breakdown(&sharpened, artifact_metric);
        let p_total = breakdown.aggregate;
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
