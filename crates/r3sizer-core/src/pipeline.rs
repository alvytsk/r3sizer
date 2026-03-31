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
    sharpen::{make_kernel, unsharp_mask_with_kernel, unsharp_mask_single_channel_with_kernel},
    solve::{find_sharpness_with_policy, find_sharpness_direct_with_policy},
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams,
    ClampPolicy, FallbackReason, FitStatus, FitStrategy, ImageSize, LinearRgbImage,
    MetricMode, ProbeConfig, ProbePassDiagnostics, ProbeSample, ProcessOutput,
    RegionCoverage, RobustnessFlags, SelectionMode, SharpenMode, SharpenStrategy,
    StageTiming, CoreError,
};

/// Pipeline-internal result of a sharpening step.
struct SharpenResult {
    image: LinearRgbImage,
}

// ---------------------------------------------------------------------------
// Base params fingerprint — tracks which params affect PreparedBase
// ---------------------------------------------------------------------------

/// Subset of [`AutoSharpParams`] that determines the [`PreparedBase`] result.
///
/// Two params with equal keys produce identical base preparation output.
/// Fields not listed here (e.g. `probe_strengths`, `metric_weights`,
/// `sharpen_mode`) only affect the probing/fitting phase, not the base.
#[derive(Debug, Clone, PartialEq)]
struct BaseParamsKey {
    target: ImageSize,
    target_artifact_ratio: f32,
    enable_contrast_leveling: bool,
    sharpen_strategy: SharpenStrategy,
    input_color_space: Option<crate::InputColorSpace>,
    resize_strategy: Option<crate::ResizeStrategy>,
    evaluation_color_space: Option<crate::EvaluationColorSpace>,
    artifact_metric: ArtifactMetric,
    evaluator_config: Option<crate::EvaluatorConfig>,
}

impl BaseParamsKey {
    fn from_params(params: &AutoSharpParams) -> Self {
        Self {
            target: ImageSize { width: params.target_width, height: params.target_height },
            target_artifact_ratio: params.target_artifact_ratio,
            enable_contrast_leveling: params.enable_contrast_leveling,
            sharpen_strategy: params.sharpen_strategy.clone(),
            input_color_space: params.input_color_space,
            resize_strategy: params.resize_strategy.clone(),
            evaluation_color_space: params.evaluation_color_space,
            artifact_metric: params.artifact_metric,
            evaluator_config: params.evaluator_config,
        }
    }
}

// ---------------------------------------------------------------------------
// Two-phase pipeline: prepare_base + process_from_prepared
// ---------------------------------------------------------------------------

/// Cached intermediate state from the pre-probing pipeline stages.
///
/// Contains everything computed before the probing loop: the downscaled base
/// image, classification, baseline metric, evaluator, and luminance.  This
/// struct is produced by [`prepare_base`] and consumed by
/// [`process_from_prepared`], allowing the expensive resize + classify +
/// evaluator work (~1.5 s on a 24 MP image) to run at image-load time.
pub struct PreparedBase {
    pub(crate) base: LinearRgbImage,
    pub(crate) input_size: ImageSize,
    /// Target dimensions this base was prepared for.
    pub target: ImageSize,
    /// Fingerprint of the params that produced this base.
    base_key: BaseParamsKey,
    pub(crate) base_luminance: Option<Vec<f32>>,
    pub(crate) gain_map: Option<crate::GainMap>,
    pub(crate) region_map: Option<crate::RegionMap>,
    pub(crate) region_coverage: Option<RegionCoverage>,
    pub(crate) baseline_artifact_ratio: f32,
    pub(crate) effective_p0: f32,
    pub(crate) base_resize_quality: crate::BaseResizeQuality,
    pub(crate) evaluator_cap: Option<f32>,
    // Timing
    pub(crate) resize_us: u64,
    pub(crate) base_quality_us: u64,
    pub(crate) contrast_us: u64,
    pub(crate) classification_us: Option<u64>,
    pub(crate) baseline_us: u64,
    pub(crate) evaluator_us: Option<u64>,
    pub(crate) ingress_us: Option<u64>,
    // Diagnostics
    pub(crate) input_ingress_diag: Option<crate::types::InputIngressDiagnostics>,
    pub(crate) resize_strategy_diag: Option<crate::ResizeStrategyDiagnostics>,
    pub(crate) used_staged_shrink: bool,
}

impl PreparedBase {
    /// Base image pixel data (linear RGB, f32 interleaved).
    pub fn base_pixels(&self) -> &[f32] { self.base.pixels() }
    /// Base image width.
    pub fn base_width(&self) -> u32 { self.base.width() }
    /// Base image height.
    pub fn base_height(&self) -> u32 { self.base.height() }
    /// Pre-computed luminance (always Some after prepare_base).
    pub fn luminance(&self) -> Option<&[f32]> { self.base_luminance.as_deref() }
    /// Baseline artifact ratio (before sharpening).
    pub fn baseline_artifact_ratio(&self) -> f32 { self.baseline_artifact_ratio }
    /// Effective P0 target (after envelope scaling).
    pub fn effective_p0(&self) -> f32 { self.effective_p0 }
    /// Whether this prepared base is valid for the given params.
    ///
    /// Returns `false` if the params differ in any field that affects base
    /// preparation (target dimensions, strategy, contrast, classification,
    /// color space, artifact metric, evaluator, or target artifact ratio).
    pub fn matches_params(&self, params: &AutoSharpParams) -> bool {
        self.base_key == BaseParamsKey::from_params(params)
    }
}

/// Pre-compute all pipeline stages that don't depend on sharpen/probe params.
///
/// Call this at image-load time with the current params.  The returned
/// [`PreparedBase`] can be reused across multiple [`process_from_prepared`]
/// calls as long as the target dimensions and strategy haven't changed.
pub fn prepare_base(
    input: &LinearRgbImage,
    params: &AutoSharpParams,
    on_stage: &dyn Fn(&str),
) -> Result<PreparedBase, CoreError> {
    on_stage("validating");
    params.validate()?;

    let input_size = input.size();
    let target = ImageSize { width: params.target_width, height: params.target_height };

    // Input color-space ingress
    let (input, input_ingress_diag, ingress_us) = {
        if let Some(cs) = params.input_color_space {
            let t0 = Instant::now();
            let (prepared, diag) = crate::color_space::prepare_input(input, cs)?;
            let us = t0.elapsed().as_micros() as u64;
            (std::borrow::Cow::Owned(prepared), Some(diag), Some(us))
        } else {
            (std::borrow::Cow::Borrowed(input), None, None)
        }
    };

    // Resize
    on_stage("resizing");
    let t0 = Instant::now();
    let (downscaled, resize_strategy_diag, used_staged_shrink) = {
        if let Some(ref strategy) = params.resize_strategy {
            let (img, diag) = crate::resize_strategy::downscale_with_strategy(&input, target, strategy)?;
            (img, Some(diag), false)
        } else {
            let (img, staged) = crate::resize::downscale_with_info(&input, target)?;
            (img, None, staged)
        }
    };
    let resize_us = t0.elapsed().as_micros() as u64;

    // Base resize quality
    let t0 = Instant::now();
    let base_resize_quality = crate::base_quality::score_base_resize(&input, &downscaled);
    let effective_p0 = params.target_artifact_ratio * base_resize_quality.envelope_scale;
    let base_quality_us = t0.elapsed().as_micros() as u64;

    // Contrast leveling
    let t0 = Instant::now();
    let mut base = downscaled;
    let cl_params = ContrastLevelingParams { enabled: params.enable_contrast_leveling };
    apply_contrast_leveling(&mut base, &cl_params)?;
    let contrast_us = t0.elapsed().as_micros() as u64;

    // Classification
    on_stage("classifying");
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

    // Baseline measurement
    on_stage("baseline");
    let t0 = Instant::now();
    let baseline_artifact_ratio = {
        if let Some(cs) = params.evaluation_color_space {
            crate::chroma_guard::evaluate_in_color_space(&base, cs)
        } else {
            match params.artifact_metric {
                ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(&base),
                ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(&base),
            }
        }
    };
    let baseline_us = t0.elapsed().as_micros() as u64;

    // Luminance extraction (always extract — cheap, needed for both probing and metrics)
    let base_luminance = Some(color::extract_luminance(&base));

    // Evaluator
    on_stage("evaluating");
    let t0 = Instant::now();
    let evaluator_cap = match &params.evaluator_config {
        Some(crate::types::EvaluatorConfig::Heuristic) => {
            let eval = crate::evaluator::HeuristicEvaluator;
            crate::evaluator::QualityEvaluator::suggest_strength(&eval, &base, 0.8)
        }
        None => None,
    };
    let evaluator_us = if params.evaluator_config.is_some() {
        Some(t0.elapsed().as_micros() as u64)
    } else {
        None
    };

    let base_key = BaseParamsKey::from_params(params);

    Ok(PreparedBase {
        base,
        input_size,
        target,
        base_key,
        base_luminance,
        gain_map,
        region_map,
        region_coverage,
        baseline_artifact_ratio,
        effective_p0,
        base_resize_quality,
        evaluator_cap,
        resize_us,
        base_quality_us,
        contrast_us,
        classification_us,
        baseline_us,
        evaluator_us,
        ingress_us,
        input_ingress_diag,
        resize_strategy_diag,
        used_staged_shrink,
    })
}

/// Run the pipeline from a pre-computed base (probing → fit → sharpen → output).
///
/// Use [`prepare_base`] to produce the `PreparedBase`, then call this with the
/// same or updated sharpen/probe params.  The expensive resize + classify +
/// evaluator work is skipped entirely.
pub fn process_from_prepared(
    prepared: &PreparedBase,
    params: &AutoSharpParams,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    let probe_result = run_probes_for_prepared(prepared, params, on_stage)?;
    finish_pipeline(prepared, params, probe_result, on_stage)
}

/// Like [`process_from_prepared`] but with externally-collected probe samples.
///
/// Use this when probes were computed in parallel across multiple workers.
/// The `probing_us` field should reflect the wall-clock time of the parallel
/// probing phase (not the sum of per-worker times).
///
/// If `pass_diagnostics` is `Some`, it is included in the output diagnostics
/// (useful for TwoPass parallel probing where JS resolved the dense window).
pub fn process_from_prepared_with_probes(
    prepared: &PreparedBase,
    params: &AutoSharpParams,
    probe_samples: Vec<ProbeSample>,
    probing_us: u64,
    pass_diagnostics: Option<ProbePassDiagnostics>,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    let probe_result = ProbeResult {
        samples: probe_samples,
        pass_diagnostics,
        probing_us,
    };
    finish_pipeline(prepared, params, probe_result, on_stage)
}

// ---------------------------------------------------------------------------
// Probe strength resolution — for parallel probing from JS
// ---------------------------------------------------------------------------

/// Resolve the initial probe strengths for the current config.
///
/// For [`ProbeConfig::TwoPass`], returns the coarse-pass strengths (first phase).
/// For [`ProbeConfig::Explicit`] or [`ProbeConfig::Range`], returns all strengths.
pub fn resolve_initial_strengths(params: &AutoSharpParams) -> Result<Vec<f32>, CoreError> {
    match &params.probe_strengths {
        ProbeConfig::TwoPass { coarse_count, coarse_min, coarse_max, .. } => {
            Ok(linspace(*coarse_min, *coarse_max, *coarse_count))
        }
        other => other.resolve(),
    }
}

/// Resolve the dense (second-pass) probe strengths from coarse results.
///
/// Only meaningful for [`ProbeConfig::TwoPass`].  Returns `Ok(None)` for other
/// configs (no second pass needed).
pub fn resolve_dense_strengths(
    coarse_samples: &[ProbeSample],
    params: &AutoSharpParams,
    effective_p0: f32,
) -> Result<Option<(Vec<f32>, ProbePassDiagnostics)>, CoreError> {
    match &params.probe_strengths {
        ProbeConfig::TwoPass {
            coarse_count, coarse_min, coarse_max, dense_count, window_margin,
        } => {
            let (dense_lo, dense_hi) = find_dense_window(
                coarse_samples, effective_p0, *coarse_min, *coarse_max, *window_margin,
            );
            let dense_strengths = linspace(dense_lo, dense_hi, *dense_count);
            let diag = ProbePassDiagnostics {
                coarse_count: *coarse_count,
                coarse_min: *coarse_min,
                coarse_max: *coarse_max,
                dense_count: *dense_count,
                dense_min: dense_lo,
                dense_max: dense_hi,
            };
            Ok(Some((dense_strengths, diag)))
        }
        _ => Ok(None),
    }
}

/// Run probes against raw image data (for use in dedicated probe workers).
///
/// Each probe worker receives the base image pixels + luminance from the main
/// worker and calls this function with its assigned subset of strengths.
pub fn run_probes_standalone(
    base_pixels: &[f32],
    width: u32,
    height: u32,
    base_luminance: &[f32],
    strengths: &[f32],
    params: &AutoSharpParams,
    baseline_artifact_ratio: f32,
) -> Result<Vec<ProbeSample>, CoreError> {
    let base = LinearRgbImage::new(width, height, base_pixels.to_vec())?;
    let kernel = make_kernel(params.sharpen_sigma)?;
    probe_strengths(
        strengths, &base, Some(base_luminance),
        params.sharpen_mode, params.metric_mode, params.artifact_metric,
        baseline_artifact_ratio, &kernel, None,
    )
}

/// Compute the precomputed detail signal from a prepared base.
///
/// Returns `D = input - blur(input)` for the current sharpen mode:
/// - **Lightness**: single-channel detail (W×H floats)
/// - **RGB**: three-channel detail (W×H×3 floats)
///
/// The returned detail buffer can be sent to probe workers so they skip the
/// Gaussian blur entirely, eliminating the dominant per-worker cost.
pub fn compute_probe_detail(
    base_pixels: &[f32],
    width: u32,
    height: u32,
    base_luminance: &[f32],
    params: &AutoSharpParams,
) -> Result<Vec<f32>, CoreError> {
    use crate::sharpen;
    let kernel = make_kernel(params.sharpen_sigma)?;
    let w = width as usize;
    let h = height as usize;

    match params.sharpen_mode {
        SharpenMode::Lightness => {
            Ok(sharpen::compute_detail_single_channel(base_luminance, w, h, &kernel))
        }
        SharpenMode::Rgb => {
            let base = LinearRgbImage::new(width, height, base_pixels.to_vec())?;
            Ok(sharpen::compute_detail_rgb(&base, &kernel))
        }
    }
}

/// Like [`run_probes_standalone`] but uses a precomputed detail signal,
/// skipping the Gaussian blur (the dominant cost per worker).
///
/// Call [`compute_probe_detail`] once in the main worker, distribute the
/// result to probe workers, then call this function in each worker.
#[allow(clippy::too_many_arguments)]
pub fn run_probes_from_detail(
    base_pixels: &[f32],
    width: u32,
    height: u32,
    base_luminance: &[f32],
    detail: &[f32],
    strengths: &[f32],
    params: &AutoSharpParams,
    baseline_artifact_ratio: f32,
) -> Result<Vec<ProbeSample>, CoreError> {
    let base = LinearRgbImage::new(width, height, base_pixels.to_vec())?;
    probe_strengths_from_detail(
        strengths, &base, Some(base_luminance), detail,
        params.sharpen_mode, params.metric_mode, params.artifact_metric,
        baseline_artifact_ratio,
    )
}

/// Probe loop using precomputed detail — no Gaussian blur, just apply + metric.
#[allow(clippy::too_many_arguments)]
fn probe_strengths_from_detail(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    detail: &[f32],
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
) -> Result<Vec<ProbeSample>, CoreError> {
    use crate::sharpen;

    let measure = |img: &LinearRgbImage| -> f32 {
        crate::metrics::compute_selection_metric(img, artifact_metric)
    };

    let mut results = Vec::with_capacity(strengths.len());
    match sharpen_mode {
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance required for Lightness mode");
            let n = (base.width() as usize) * (base.height() as usize);
            let mut scratch = vec![0.0f32; n];

            for &s in strengths {
                sharpen::apply_detail_single_channel_into(lum, detail, s, &mut scratch);
                let image = color::reconstruct_rgb_from_lightness_with_luma(
                    base, &scratch, Some(lum),
                );
                let p_total = measure(&image);
                let metric_value =
                    compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                results.push(ProbeSample {
                    strength: s, artifact_ratio: p_total, metric_value, breakdown: None,
                });
            }
        }
        SharpenMode::Rgb => {
            for &s in strengths {
                let image = sharpen::apply_detail_rgb(base, detail, s);
                let p_total = measure(&image);
                let metric_value =
                    compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                results.push(ProbeSample {
                    strength: s, artifact_ratio: p_total, metric_value, breakdown: None,
                });
            }
        }
    }
    Ok(results)
}

/// Internal result of the probing phase.
struct ProbeResult {
    samples: Vec<ProbeSample>,
    pass_diagnostics: Option<ProbePassDiagnostics>,
    probing_us: u64,
}

/// Run the probing phase against a PreparedBase.
fn run_probes_for_prepared(
    prepared: &PreparedBase,
    params: &AutoSharpParams,
    on_stage: &dyn Fn(&str),
) -> Result<ProbeResult, CoreError> {
    let base = &prepared.base;
    let base_luminance = prepared.base_luminance.as_deref();
    let baseline_artifact_ratio = prepared.baseline_artifact_ratio;
    let effective_p0 = prepared.effective_p0;

    let kernel = make_kernel(params.sharpen_sigma)?;

    let eval_cs_fn = params.evaluation_color_space.map(|cs| {
        move |img: &LinearRgbImage| -> f32 {
            crate::chroma_guard::evaluate_in_color_space(img, cs)
        }
    });
    let metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)> =
        eval_cs_fn.as_ref().map(|f| f as &(dyn Fn(&LinearRgbImage) -> f32 + Sync));

    on_stage("probing");
    let t0 = Instant::now();
    let (samples, pass_diagnostics) = match &params.probe_strengths {
        ProbeConfig::TwoPass {
            coarse_count, coarse_min, coarse_max, dense_count, window_margin,
        } => {
            run_two_pass_probing(
                *coarse_count, *coarse_min, *coarse_max,
                *dense_count, *window_margin,
                effective_p0,
                base, base_luminance,
                params.sharpen_mode, params.metric_mode, params.artifact_metric,
                baseline_artifact_ratio, &kernel, metric_override,
            )?
        }
        _ => {
            let strengths = params.probe_strengths.resolve()?;
            let samples = probe_strengths(
                &strengths, base, base_luminance,
                params.sharpen_mode, params.metric_mode, params.artifact_metric,
                baseline_artifact_ratio, &kernel, metric_override,
            )?;
            (samples, None)
        }
    };
    let probing_us = t0.elapsed().as_micros() as u64;

    Ok(ProbeResult { samples, pass_diagnostics, probing_us })
}

/// Post-probing pipeline: fit → solve → sharpen → metrics → diagnostics.
fn finish_pipeline(
    prepared: &PreparedBase,
    params: &AutoSharpParams,
    probe_result: ProbeResult,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    let pipeline_start = Instant::now();

    let base = &prepared.base;
    let base_luminance = prepared.base_luminance.as_deref();
    let baseline_artifact_ratio = prepared.baseline_artifact_ratio;
    let effective_p0 = prepared.effective_p0;

    let kernel = make_kernel(params.sharpen_sigma)?;

    let eval_cs_fn = params.evaluation_color_space.map(|cs| {
        move |img: &LinearRgbImage| -> f32 {
            crate::chroma_guard::evaluate_in_color_space(img, cs)
        }
    });
    let metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)> =
        eval_cs_fn.as_ref().map(|f| f as &(dyn Fn(&LinearRgbImage) -> f32 + Sync));

    let ProbeResult { samples: probe_samples, pass_diagnostics: probe_pass_diagnostics, probing_us } = probe_result;

    // --- Fit + Solve ---
    let s_min = if matches!(params.metric_mode, MetricMode::RelativeToBase) {
        0.0_f64
    } else {
        probe_samples.first().map(|s| s.strength as f64).unwrap_or(0.05)
    };
    let s_max = probe_samples.last().map(|s| s.strength as f64).unwrap_or(3.0);
    let p0 = effective_p0 as f64;

    on_stage("fitting");
    let mut fit_data: Vec<(f64, f64)> = probe_samples
        .iter()
        .map(|ps| (ps.strength as f64, ps.metric_value as f64))
        .collect();
    if matches!(params.metric_mode, MetricMode::RelativeToBase) {
        let first_s = probe_samples.first().map(|p| p.strength).unwrap_or(1.0);
        if first_s > 1e-6 {
            fit_data.insert(0, (0.0, 0.0));
        }
    }

    let (monotonic, quasi_monotonic) = check_monotonicity(&probe_samples);

    let t0 = Instant::now();
    let (solve_result, fit_status, fit_coefficients, fit_quality) = match params.fit_strategy {
        FitStrategy::DirectSearch => {
            let result = find_sharpness_direct_with_policy(
                &probe_samples, effective_p0, params.selection_policy,
            )?;
            (result, FitStatus::Skipped, None, None)
        }
        FitStrategy::Cubic => {
            match fit_cubic_with_quality(&fit_data) {
                Ok((poly, quality)) => {
                    let result = find_sharpness_with_policy(
                        &poly, p0, s_min, s_max, &probe_samples, params.selection_policy,
                    )?;
                    if quality.r_squared < 0.85
                        && matches!(result.selection_mode, SelectionMode::PolynomialRoot)
                    {
                        let direct = find_sharpness_direct_with_policy(
                            &probe_samples, effective_p0, params.selection_policy,
                        )?;
                        (direct, FitStatus::Success, Some(poly), Some(quality))
                    } else {
                        (result, FitStatus::Success, Some(poly), Some(quality))
                    }
                }
                Err(fit_err) => {
                    let result = find_sharpness_direct_with_policy(
                        &probe_samples, effective_p0, params.selection_policy,
                    )?;
                    (result, FitStatus::Failed { reason: fit_err.to_string() }, None, None)
                }
            }
        }
    };
    let fit_us = t0.elapsed().as_micros() as u64;

    // --- LOO stability ---
    on_stage("robustness");
    let t0 = Instant::now();
    let (loo_stable, max_loo_root_change) = if fit_coefficients.is_some() {
        let primary_s = solve_result.selected_strength as f64;
        loo_stability(&fit_data, p0, s_min, s_max, primary_s)
    } else {
        (true, 0.0)
    };
    let robustness_us = t0.elapsed().as_micros() as u64;

    let r_squared_ok = fit_quality.is_none_or(|q| q.r_squared > 0.85);
    let well_conditioned = fit_quality.is_none_or(|q| q.min_pivot > 1e-8);
    let robustness = Some(RobustnessFlags {
        monotonic, quasi_monotonic, r_squared_ok, well_conditioned, loo_stable, max_loo_root_change,
    });

    let budget_reachable_baseline = match params.metric_mode {
        MetricMode::AbsoluteTotal => baseline_artifact_ratio <= effective_p0,
        MetricMode::RelativeToBase => true,
    };
    let budget_reachable = budget_reachable_baseline
        && !matches!(solve_result.selection_mode, SelectionMode::LeastBadSample);
    let selection_mode = if !budget_reachable_baseline {
        SelectionMode::BudgetUnreachable
    } else {
        solve_result.selection_mode
    };

    let fallback_reason = determine_fallback_reason(
        &selection_mode, &fit_status, budget_reachable_baseline,
        monotonic, r_squared_ok, loo_stable, params.fit_strategy,
        &solve_result.crossing_status,
    );

    // Evaluator cap
    let selected_strength = match prepared.evaluator_cap {
        Some(cap) if solve_result.selected_strength > cap => cap,
        _ => solve_result.selected_strength,
    };

    // --- Final sharpening ---
    on_stage("sharpening");
    let t0 = Instant::now();
    let _chroma_guard_diag;
    let (mut final_image, adaptive_validation, adaptive_validation_us) =
        match (&params.sharpen_strategy, &prepared.gain_map) {
            (SharpenStrategy::Uniform, _) | (_, None) => {
                let result = sharpen_image(
                    base, base_luminance, params.sharpen_mode,
                    selected_strength, &kernel,
                )?;
                (result.image, None, None)
            }
            (
                SharpenStrategy::ContentAdaptive {
                    max_backoff_iterations, backoff_scale_factor, ..
                },
                Some(gm),
            ) => {
                let effective_max_backoff = if budget_reachable { *max_backoff_iterations } else { 0 };
                adaptive_sharpen_with_validation(
                    base, base_luminance, params.sharpen_mode,
                    selected_strength, gm, params.sharpen_sigma, effective_p0,
                    params.artifact_metric, params.metric_mode,
                    baseline_artifact_ratio, effective_max_backoff,
                    *backoff_scale_factor, params.evaluation_color_space,
                )?
            }
        };

    // Chroma guard
    {
        if let Some(crate::types::ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift, chroma_region_factors, saturation_guard,
        }) = &params.experimental_sharpen_mode {
            let (guarded, cg_diag) = crate::chroma_guard::apply_chroma_guard(
                base, &final_image, *max_chroma_shift,
                prepared.region_map.as_ref(),
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

    // --- Metrics on final image ---
    let fallback_luma;
    let base_luma = match prepared.base_luminance.as_deref() {
        Some(l) => l,
        None => { fallback_luma = color::extract_luminance(base); &fallback_luma }
    };
    let final_luma = color::extract_luminance(&final_image);
    let final_breakdown = crate::metrics::compute_metric_breakdown(
        &final_image, base, base_luma, &final_luma,
        params.artifact_metric, &params.metric_weights,
    );
    let measured_artifact_ratio = match metric_override {
        Some(f) => f(&final_image),
        None => final_breakdown.selection_score,
    };
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio, baseline_artifact_ratio, params.metric_mode,
    );

    // Evaluator (full)
    let (_evaluator_result, _evaluator_process_us) = {
        if let Some(ref eval_config) = params.evaluator_config {
            let t0 = Instant::now();
            let result = match eval_config {
                crate::types::EvaluatorConfig::Heuristic => {
                    let eval = crate::evaluator::HeuristicEvaluator;
                    crate::evaluator::QualityEvaluator::evaluate(&eval, base, &final_image, selected_strength)
                }
            };
            let us = t0.elapsed().as_micros() as u64;
            (Some(result), Some(us))
        } else {
            (None, None)
        }
    };

    // --- Clamp ---
    on_stage("finalizing");
    let t0 = Instant::now();
    match params.output_clamp {
        ClampPolicy::Clamp => {
            for v in final_image.pixels_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        ClampPolicy::Normalize => {
            let max_val = final_image.pixels().iter().copied().fold(f32::NEG_INFINITY, f32::max);
            if max_val > 0.0 {
                for v in final_image.pixels_mut() {
                    *v = (*v / max_val).max(0.0);
                }
            } else {
                for v in final_image.pixels_mut() {
                    *v = 0.0;
                }
            }
        }
    }
    let clamp_us = t0.elapsed().as_micros() as u64;
    let total_us = pipeline_start.elapsed().as_micros() as u64;

    // --- Assemble diagnostics ---
    // Total timing includes pre-computed stages from PreparedBase.
    let full_total_us = total_us
        + prepared.resize_us + prepared.base_quality_us + prepared.contrast_us
        + prepared.classification_us.unwrap_or(0) + prepared.baseline_us
        + prepared.evaluator_us.unwrap_or(0) + prepared.ingress_us.unwrap_or(0);

    let mut diagnostics = AutoSharpDiagnostics {
        input_size: prepared.input_size,
        output_size: prepared.target,
        sharpen_mode: params.sharpen_mode,
        metric_mode: params.metric_mode,
        artifact_metric: params.artifact_metric,
        selection_policy: params.selection_policy,
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
        region_coverage: prepared.region_coverage,
        adaptive_validation,
        timing: StageTiming {
            resize_us: prepared.resize_us,
            contrast_us: prepared.contrast_us,
            baseline_us: prepared.baseline_us,
            probing_us,
            fit_us,
            robustness_us,
            final_sharpen_us,
            clamp_us,
            total_us: full_total_us,
            classification_us: prepared.classification_us,
            adaptive_validation_us,
            ingress_us: prepared.ingress_us,
            evaluator_us: prepared.evaluator_us,
            base_quality_us: Some(prepared.base_quality_us),
        },
        input_ingress: prepared.input_ingress_diag,
        resize_strategy_diagnostics: prepared.resize_strategy_diag.clone(),  // contains Vec
        chroma_guard: _chroma_guard_diag,
        evaluator_result: _evaluator_result,
        recommendations: Vec::new(),
        probe_pass_diagnostics,
        base_resize_quality: Some(prepared.base_resize_quality),
        effective_target_artifact_ratio: effective_p0,
        used_staged_shrink: prepared.used_staged_shrink,
    };

    diagnostics.recommendations =
        crate::recommendations::generate_recommendations(&diagnostics, params);

    Ok(ProcessOutput { image: final_image, diagnostics })
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
///
/// Internally delegates to [`prepare_base`] + [`process_from_prepared`] so
/// that there is a single implementation of the pipeline logic.
pub fn process_auto_sharp_downscale_with_progress(
    input: &LinearRgbImage,
    params: &AutoSharpParams,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    let prepared = prepare_base(input, params, on_stage)?;
    process_from_prepared(&prepared, params, on_stage)
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
    let image = match mode {
        SharpenMode::Rgb => unsharp_mask_with_kernel(base, amount, kernel),
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;
            let sharpened_l = unsharp_mask_single_channel_with_kernel(lum, w, h, amount, kernel);
            color::reconstruct_rgb_from_lightness_with_luma(base, &sharpened_l, Some(lum))
        }
    };
    Ok(SharpenResult { image })
}

/// Run all probe strengths and collect `ProbeSample`s.
///
/// **Optimisation:** the Gaussian blur (the dominant cost) is computed once to
/// derive a detail signal `D = input - blur(input)`.  Each probe then applies
/// `out = input + s × D` — a trivial multiply-add — instead of recomputing the
/// full separable blur for every strength.  This collapses probe cost from
/// `N × (blur + apply)` to `1 × blur + N × apply`.
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
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<Vec<ProbeSample>, CoreError> {
    use crate::sharpen;

    let measure = |img: &LinearRgbImage| -> f32 {
        match metric_override {
            Some(f) => f(img),
            None => crate::metrics::compute_selection_metric(img, artifact_metric),
        }
    };

    match sharpen_mode {
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;

            // One blur — the expensive part.
            let detail = sharpen::compute_detail_single_channel(lum, w, h, kernel);

            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                strengths
                    .par_iter()
                    .map(|&s| {
                        let sharpened_l = sharpen::apply_detail_single_channel(lum, &detail, s);
                        let image = color::reconstruct_rgb_from_lightness_with_luma(
                            base, &sharpened_l, Some(lum),
                        );
                        let p_total = measure(&image);
                        let metric_value =
                            compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                        Ok(ProbeSample {
                            strength: s,
                            artifact_ratio: p_total,
                            metric_value,
                            breakdown: None,
                        })
                    })
                    .collect()
            }

            #[cfg(not(feature = "parallel"))]
            {
                let n = w * h;
                let mut scratch = vec![0.0f32; n];
                let mut results = Vec::with_capacity(strengths.len());
                for &s in strengths {
                    sharpen::apply_detail_single_channel_into(lum, &detail, s, &mut scratch);
                    let image = color::reconstruct_rgb_from_lightness_with_luma(
                        base, &scratch, Some(lum),
                    );
                    let p_total = measure(&image);
                    let metric_value =
                        compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                    results.push(ProbeSample {
                        strength: s,
                        artifact_ratio: p_total,
                        metric_value,
                        breakdown: None,
                    });
                }
                Ok(results)
            }
        }

        SharpenMode::Rgb => {
            // One blur — the expensive part.
            let detail = sharpen::compute_detail_rgb(base, kernel);

            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                strengths
                    .par_iter()
                    .map(|&s| {
                        let image = sharpen::apply_detail_rgb(base, &detail, s);
                        let p_total = measure(&image);
                        let metric_value =
                            compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                        Ok(ProbeSample {
                            strength: s,
                            artifact_ratio: p_total,
                            metric_value,
                            breakdown: None,
                        })
                    })
                    .collect()
            }

            #[cfg(not(feature = "parallel"))]
            {
                let mut results = Vec::with_capacity(strengths.len());
                for &s in strengths {
                    let image = sharpen::apply_detail_rgb(base, &detail, s);
                    let p_total = measure(&image);
                    let metric_value =
                        compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
                    results.push(ProbeSample {
                        strength: s,
                        artifact_ratio: p_total,
                        metric_value,
                        breakdown: None,
                    });
                }
                Ok(results)
            }
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
            if let Ok(result) = find_sharpness_with_policy(
                &poly, p0, s_min, s_max,
                &[], // empty samples — force polynomial-only path or no fallback
                crate::SelectionPolicy::GamutOnly, // LOO only checks polynomial root stability
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
    r_squared_ok: bool,
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
    if !r_squared_ok {
        return Some(FallbackReason::FitPoorQuality);
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
    evaluation_color_space: Option<crate::types::EvaluationColorSpace>,
) -> Result<(LinearRgbImage, Option<AdaptiveValidationOutcome>, Option<u64>), CoreError> {
    let w = base.width() as usize;
    let h = base.height() as usize;

    let measure = |img: &LinearRgbImage| -> f32 {
        let raw = if let Some(cs) = evaluation_color_space {
            crate::chroma_guard::evaluate_in_color_space(img, cs)
        } else {
            match artifact_metric {
                ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
                ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
            }
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
                crate::color::reconstruct_rgb_from_lightness_with_luma(
                    base, &sharpened_l, Some(luma),
                )
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
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<(Vec<ProbeSample>, Option<ProbePassDiagnostics>), CoreError> {
    // Phase 1: coarse
    let coarse_strengths = linspace(coarse_min, coarse_max, coarse_count);
    let coarse_samples = probe_strengths(
        &coarse_strengths, base, base_luminance, sharpen_mode, metric_mode,
        artifact_metric, baseline_artifact_ratio, kernel, metric_override,
    )?;

    // Locate dense window from coarse results
    let (dense_lo, dense_hi) =
        find_dense_window(&coarse_samples, p0, coarse_min, coarse_max, window_margin);

    // Phase 2: dense
    let dense_strengths = linspace(dense_lo, dense_hi, dense_count);
    let dense_samples = probe_strengths(
        &dense_strengths, base, base_luminance, sharpen_mode, metric_mode,
        artifact_metric, baseline_artifact_ratio, kernel, metric_override,
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
