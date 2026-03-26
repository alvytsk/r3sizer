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
use crate::{
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::fit_cubic,
    metrics::artifact_ratio,
    resize::downscale,
    sharpen::{unsharp_mask, unsharp_mask_single_channel},
    solve::{find_sharpness, find_sharpness_direct},
    AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FitStatus,
    FitStrategy, ImageSize, LinearRgbImage, MetricMode, ProbeSample, ProcessOutput,
    SelectionMode, SharpenMode, CoreError,
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

    let input_size = input.size();
    let target = ImageSize { width: params.target_width, height: params.target_height };

    // -------------------------------------------------------------------
    // 2. Downscale in linear space
    // -------------------------------------------------------------------
    let downscaled = downscale(input, target)?;

    // -------------------------------------------------------------------
    // 3. Optional contrast leveling
    // -------------------------------------------------------------------
    let mut base = downscaled;
    let cl_params = ContrastLevelingParams { enabled: params.enable_contrast_leveling };
    apply_contrast_leveling(&mut base, &cl_params)?;

    // -------------------------------------------------------------------
    // 4. Measure baseline artifact ratio (before any sharpening)
    // -------------------------------------------------------------------
    let baseline_artifact_ratio = artifact_ratio(&base);

    // -------------------------------------------------------------------
    // 5. Probe sharpening strengths
    // -------------------------------------------------------------------
    let strengths = params.probe_strengths.resolve()?;
    let sigma = params.sharpen_sigma;
    let w = base.width() as usize;
    let h = base.height() as usize;

    // Extract luminance once if using lightness mode (avoids recomputation per probe).
    let base_luminance = if matches!(params.sharpen_mode, SharpenMode::Lightness) {
        Some(color::extract_luminance(&base))
    } else {
        None
    };

    let mut probe_samples: Vec<ProbeSample> = Vec::with_capacity(strengths.len());
    for &s in &strengths {
        let sharpened = sharpen_image(
            &base,
            base_luminance.as_deref(),
            params.sharpen_mode,
            s,
            sigma,
            w,
            h,
        )?;
        let p_total = artifact_ratio(&sharpened);
        let metric_value = compute_metric_value(
            p_total,
            baseline_artifact_ratio,
            params.metric_mode,
        );
        probe_samples.push(ProbeSample {
            strength: s,
            artifact_ratio: p_total,
            metric_value,
        });
    }

    // -------------------------------------------------------------------
    // 6 + 7. Fit + solve (or direct search)
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

    let (solve_result, fit_status, fit_coefficients) = match params.fit_strategy {
        FitStrategy::DirectSearch => {
            let result = find_sharpness_direct(&probe_samples, params.target_artifact_ratio)?;
            (result, FitStatus::Skipped, None)
        }

        FitStrategy::ForcedLinear | FitStrategy::Cubic => {
            match fit_cubic(&fit_data) {
                Ok(poly) => {
                    let result =
                        find_sharpness(&poly, p0, s_min, s_max, &probe_samples)?;
                    (result, FitStatus::Success, Some(poly))
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
                    )
                }
            }
        }
    };

    // -------------------------------------------------------------------
    // 8. Determine budget reachability
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
    // 9. Final sharpening
    // -------------------------------------------------------------------
    let selected_strength = solve_result.selected_strength;
    let mut final_image = sharpen_image(
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        selected_strength,
        sigma,
        w,
        h,
    )?;

    // -------------------------------------------------------------------
    // 10. Measure actual artifact ratio (pre-clamp)
    // -------------------------------------------------------------------
    let measured_artifact_ratio = artifact_ratio(&final_image);
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    // -------------------------------------------------------------------
    // 11. Apply clamp policy
    // -------------------------------------------------------------------
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
                    *v /= max_val;
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // 12. Return
    // -------------------------------------------------------------------
    let diagnostics = AutoSharpDiagnostics {
        input_size,
        output_size: target,
        sharpen_mode: params.sharpen_mode,
        metric_mode: params.metric_mode,
        target_artifact_ratio: params.target_artifact_ratio,
        baseline_artifact_ratio,
        probe_samples,
        fit_status,
        fit_coefficients,
        crossing_status: solve_result.crossing_status,
        selected_strength,
        selection_mode,
        budget_reachable,
        measured_artifact_ratio,
        measured_metric_value,
    };

    Ok(ProcessOutput { image: final_image, diagnostics })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply sharpening to the base image using the configured mode.
fn sharpen_image(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    amount: f32,
    sigma: f32,
    w: usize,
    h: usize,
) -> Result<LinearRgbImage, CoreError> {
    match mode {
        SharpenMode::Rgb => unsharp_mask(base, amount, sigma),
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let sharpened_l = unsharp_mask_single_channel(lum, w, h, amount, sigma)?;
            Ok(color::reconstruct_rgb_from_lightness(base, &sharpened_l))
        }
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
