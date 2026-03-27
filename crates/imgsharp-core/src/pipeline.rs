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
    metrics::channel_clipping_ratio,
    resize::downscale,
    sharpen::{make_kernel, unsharp_mask_with_kernel, unsharp_mask_single_channel_with_kernel},
    solve::{find_sharpness, find_sharpness_direct},
    ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FitStatus,
    FitStrategy, ImageSize, LinearRgbImage, MetricMode, ProbeSample, ProcessOutput,
    Provenance, SelectionMode, SharpenMode, SharpenModel, StageProvenance, CoreError,
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
    let measure = |img: &LinearRgbImage| -> f32 {
        match params.artifact_metric {
            ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
            ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
        }
    };
    let baseline_artifact_ratio = measure(&base);

    // -------------------------------------------------------------------
    // 5. Probe sharpening strengths
    // -------------------------------------------------------------------
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

    let (solve_result, fit_status, fit_coefficients) = match params.fit_strategy {
        FitStrategy::DirectSearch => {
            let result = find_sharpness_direct(&probe_samples, params.target_artifact_ratio)?;
            (result, FitStatus::Skipped, None)
        }

        FitStrategy::Cubic => {
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
    // 8. Final sharpening
    // -------------------------------------------------------------------
    let selected_strength = solve_result.selected_strength;
    let mut final_image = sharpen_image(
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        params.sharpen_model,
        selected_strength,
        &kernel,
    )?;

    // -------------------------------------------------------------------
    // 9. Measure actual artifact ratio (pre-clamp)
    // -------------------------------------------------------------------
    let measured_artifact_ratio = measure(&final_image);
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    // -------------------------------------------------------------------
    // 10. Apply clamp policy
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
        crossing_status: solve_result.crossing_status,
        selected_strength,
        selection_mode,
        budget_reachable,
        measured_artifact_ratio,
        measured_metric_value,
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
    let measure = |img: &LinearRgbImage| -> f32 {
        match artifact_metric {
            ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
            ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
        }
    };
    let probe_one = |&s: &f32| -> Result<ProbeSample, CoreError> {
        let sharpened = sharpen_image(base, base_luminance, sharpen_mode, sharpen_model, s, kernel)?;
        let p_total = measure(&sharpened);
        let metric_value = compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
        Ok(ProbeSample { strength: s, artifact_ratio: p_total, metric_value })
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
