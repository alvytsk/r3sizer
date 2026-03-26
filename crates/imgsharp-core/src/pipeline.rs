/// Pipeline orchestrator.
///
/// Coordinates all processing stages in the correct order:
///
/// 1. Validate parameters.
/// 2. Downscale to target size (linear space).
/// 3. Optional contrast leveling.
/// 4. Probe multiple sharpening strengths, measure P(s).
/// 5. Fit cubic polynomial to probe samples.
/// 6. Solve P_hat(s*) = P0 (with fallback on failure).
/// 7. Apply final sharpening with s*.
/// 8. Measure actual artifact ratio on the final image.
/// 9. Apply clamp/normalize policy.
/// 10. Return result image + full diagnostics.
use crate::{
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::fit_cubic,
    metrics::artifact_ratio,
    resize::downscale,
    sharpen::unsharp_mask,
    solve::find_sharpness,
    AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FitStrategy, ImageSize,
    LinearRgbImage, ProbeSample, ProcessOutput, CoreError,
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
    // 4. Probe sharpening strengths
    // -------------------------------------------------------------------
    let strengths = params.probe_strengths.resolve()?;
    let sigma = params.sharpen_sigma;

    let mut probe_samples: Vec<ProbeSample> = Vec::with_capacity(strengths.len());
    for &s in &strengths {
        let sharpened = unsharp_mask(&base, s, sigma)?;
        let p = artifact_ratio(&sharpened);
        probe_samples.push(ProbeSample { strength: s, artifact_ratio: p });
    }

    // -------------------------------------------------------------------
    // 5 + 6. Fit + solve (or direct search)
    // -------------------------------------------------------------------
    let s_min = strengths.first().copied().unwrap_or(0.5) as f64;
    let s_max = strengths.last().copied().unwrap_or(4.0) as f64;
    let p0 = params.target_artifact_ratio as f64;

    let (selected_strength, fallback_used, fallback_reason, fit_coefficients) =
        match params.fit_strategy {
            FitStrategy::DirectSearch => {
                // Skip fitting entirely.
                let (s, fb, reason) = find_sharpness(
                    // Dummy polynomial — will immediately fall back to samples.
                    &crate::CubicPolynomial { a: 0.0, b: 0.0, c: 0.0, d: f64::MAX },
                    p0,
                    s_min,
                    s_max,
                    &probe_samples,
                )?;
                (s, fb, reason, None)
            }

            FitStrategy::ForcedLinear | FitStrategy::Cubic => {
                let fit_result = fit_cubic(&probe_samples);
                match fit_result {
                    Ok(poly) => {
                        let (s, fb, reason) =
                            find_sharpness(&poly, p0, s_min, s_max, &probe_samples)?;
                        (s, fb, reason, Some(poly))
                    }
                    Err(fit_err) => {
                        // Fitting failed — fall back to direct sample search and
                        // record the reason.
                        let fallback_reason = Some(format!(
                            "cubic fit failed ({}); using direct sample search",
                            fit_err
                        ));
                        let (s, _, _) = find_sharpness(
                            &crate::CubicPolynomial { a: 0.0, b: 0.0, c: 0.0, d: f64::MAX },
                            p0,
                            s_min,
                            s_max,
                            &probe_samples,
                        )?;
                        (s, true, fallback_reason, None)
                    }
                }
            }
        };

    // -------------------------------------------------------------------
    // 7. Final sharpening
    // -------------------------------------------------------------------
    let mut final_image = unsharp_mask(&base, selected_strength, sigma)?;

    // -------------------------------------------------------------------
    // 8. Measure actual artifact ratio (pre-clamp)
    // -------------------------------------------------------------------
    let measured_artifact_ratio = artifact_ratio(&final_image);

    // -------------------------------------------------------------------
    // 9. Apply clamp policy
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
    // 10. Return
    // -------------------------------------------------------------------
    let diagnostics = AutoSharpDiagnostics {
        selected_strength,
        target_artifact_ratio: params.target_artifact_ratio,
        measured_artifact_ratio,
        probe_samples,
        fit_coefficients,
        fallback_used,
        fallback_reason,
        input_size,
        output_size: ImageSize { width: params.target_width, height: params.target_height },
    };

    Ok(ProcessOutput { image: final_image, diagnostics })
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
