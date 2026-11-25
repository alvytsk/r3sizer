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
use serde::{Deserialize, Serialize};
use web_time::Instant;

use crate::{
    classifier::gain_map_from_region_map,
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::{check_monotonicity, fit_cubic, fit_cubic_with_quality},
    metrics::channel_clipping_ratio,
    sharpen::make_kernel,
    solve::{find_sharpness_direct_with_policy, find_sharpness_with_policy},
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy,
    CoreError, DiagnosticsLevel, EvaluationColorSpace, EvaluatorCapDiagnostics, FallbackReason,
    FitStatus, FitStrategy, ImageSize, LinearRgbImage, MetricMode, MetricWeights, ProbeConfig,
    ProbePassDiagnostics, ProbeSample, ProcessOutput, RegionCoverage, RobustnessFlags,
    SelectionMode, SelectionPolicy, SharpenMode, SharpenStrategy, StageTiming,
};

// ---------------------------------------------------------------------------
// Probe settings — everything a probe measurement reads from the params
// ---------------------------------------------------------------------------

/// The parameters a probe measurement reads, and nothing else.
///
/// Both probe paths build their context from this value, so in-process and
/// worker probes measure the same thing for the same inputs.  A probe worker
/// input holds no params, which makes that structural: a setting that is not
/// here cannot reach a measurement.
///
/// Floats are stored as [`f32::to_bits`], so two settings compare equal
/// exactly when they would produce the same numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeSettings {
    sharpen_sigma_bits: u32,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    metric_weights_bits: [u32; 4],
    selection_policy: SelectionPolicy,
    evaluation_color_space: Option<EvaluationColorSpace>,
}

impl ProbeSettings {
    /// The settings `params` configures a probe measurement with.
    pub(crate) fn from_params(params: &AutoSharpParams) -> Self {
        let w = params.metric_weights;
        Self {
            sharpen_sigma_bits: params.sharpen_sigma.to_bits(),
            sharpen_mode: params.sharpen_mode,
            metric_mode: params.metric_mode,
            artifact_metric: params.artifact_metric,
            metric_weights_bits: [
                w.gamut_excursion.to_bits(),
                w.halo_ringing.to_bits(),
                w.edge_overshoot.to_bits(),
                w.texture_flattening.to_bits(),
            ],
            selection_policy: params.selection_policy,
            evaluation_color_space: params.evaluation_color_space,
        }
    }

    /// Whether a probe computes a per-probe [`MetricBreakdown`](crate::MetricBreakdown).
    fn needs_probe_breakdown(&self) -> bool {
        self.selection_policy.needs_probe_breakdown()
    }

    fn metric_weights(&self) -> MetricWeights {
        let [gamut_excursion, halo_ringing, edge_overshoot, texture_flattening] =
            self.metric_weights_bits;
        MetricWeights {
            gamut_excursion: f32::from_bits(gamut_excursion),
            halo_ringing: f32::from_bits(halo_ringing),
            edge_overshoot: f32::from_bits(edge_overshoot),
            texture_flattening: f32::from_bits(texture_flattening),
        }
    }

    /// Length of the detail signal these settings probe with, in floats, for
    /// a base of `pixels` pixels: one per pixel in Lightness mode, three in
    /// Rgb mode.
    fn detail_len(&self, pixels: usize) -> usize {
        match self.sharpen_mode {
            SharpenMode::Lightness => pixels,
            SharpenMode::Rgb => pixels * 3,
        }
    }
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
            target: ImageSize {
                width: params.target_width,
                height: params.target_height,
            },
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
    pub fn base_pixels(&self) -> &[f32] {
        self.base.pixels()
    }
    /// Base image width.
    pub fn base_width(&self) -> u32 {
        self.base.width()
    }
    /// Base image height.
    pub fn base_height(&self) -> u32 {
        self.base.height()
    }
    /// Pre-computed luminance (always Some after prepare_base).
    pub fn luminance(&self) -> Option<&[f32]> {
        self.base_luminance.as_deref()
    }
    /// Baseline artifact ratio (before sharpening).
    pub fn baseline_artifact_ratio(&self) -> f32 {
        self.baseline_artifact_ratio
    }
    /// Effective P0 target (after envelope scaling).
    pub fn effective_p0(&self) -> f32 {
        self.effective_p0
    }
    /// Whether this prepared base is valid for the given params.
    ///
    /// Returns `false` if the params differ in any field that affects base
    /// preparation (target dimensions, strategy, contrast, classification,
    /// color space, artifact metric, evaluator, or target artifact ratio).
    pub fn matches_params(&self, params: &AutoSharpParams) -> bool {
        self.base_key == BaseParamsKey::from_params(params)
    }

    /// Pre-compute the detail signal `D = input − blur(input)` for this base.
    ///
    /// The returned buffer has the same layout as the base pixels, but
    /// channel-count depends on `params.sharpen_mode`:
    /// - **Lightness**: `W × H` floats (single channel)
    /// - **RGB**: `W × H × 3` floats (interleaved)
    ///
    /// Pass this detail buffer to probe workers so they skip the Gaussian blur
    /// entirely and call [`run_probes_from_detail`] instead of
    /// [`run_probes_standalone`].  The detail signal is independent of sharpening
    /// strength, so it only needs to be computed once per probe phase.
    pub fn compute_detail(&self, params: &AutoSharpParams) -> Result<Vec<f32>, CoreError> {
        compute_probe_detail(
            self.base.pixels(),
            self.base.width(),
            self.base.height(),
            self.base_luminance.as_deref().unwrap_or(&[]),
            params,
        )
    }

    /// Run sharpening probes for the given strengths against this base.
    ///
    /// Convenience wrapper around [`run_probes_standalone`] that reads the
    /// required fields directly from `self`, avoiding the need to pass raw
    /// pixel slices from call sites.
    ///
    /// For higher performance in parallel workers, prefer using
    /// [`compute_detail`][Self::compute_detail] once and then calling
    /// [`run_probes_from_detail`] per worker.
    pub fn run_probes(
        &self,
        strengths: &[f32],
        params: &AutoSharpParams,
    ) -> Result<Vec<ProbeSample>, CoreError> {
        run_probes_standalone(
            self.base.pixels(),
            self.base.width(),
            self.base.height(),
            self.base_luminance.as_deref().unwrap_or(&[]),
            strengths,
            params,
            self.baseline_artifact_ratio,
        )
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
    let target = ImageSize {
        width: params.target_width,
        height: params.target_height,
    };

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
            let (img, diag) =
                crate::resize_strategy::downscale_with_strategy(&input, target, strategy)?;
            (img, Some(diag), false)
        } else {
            let (img, staged) = crate::resize::downscale_with_info(&input, target)?;
            (img, None, staged)
        }
    };
    let resize_us = t0.elapsed().as_micros() as u64;

    // Base resize quality
    // Source-side edge/texture retention is diagnostic-only in v1 (not used by
    // the solver or displayed in the web UI).  Always skip to avoid expensive
    // O(W_src × H_src) Sobel + local-variance on the full source image.
    // Ringing score (resized-image only, active) always runs.
    let t0 = Instant::now();
    let base_resize_quality = crate::base_quality::score_base_resize(&input, &downscaled, false);
    let effective_p0 = params.target_artifact_ratio * base_resize_quality.envelope_scale;
    let base_quality_us = t0.elapsed().as_micros() as u64;

    // Contrast leveling
    let t0 = Instant::now();
    let mut base = downscaled;
    let cl_params = ContrastLevelingParams {
        enabled: params.enable_contrast_leveling,
    };
    apply_contrast_leveling(&mut base, &cl_params)?;
    let contrast_us = t0.elapsed().as_micros() as u64;

    // Luminance extraction — moved before classification so the classifier
    // can reuse it instead of extracting its own copy.
    let base_luminance = Some(color::extract_luminance(&base));

    // Classification — reuses pre-extracted luminance via classify_with_luminance.
    on_stage("classifying");
    let t0 = Instant::now();
    let (gain_map, region_map, region_coverage, classification_us) = match &params.sharpen_strategy
    {
        SharpenStrategy::ContentAdaptive {
            classification,
            gain_table,
            ..
        } => {
            let luma = base_luminance.as_ref().unwrap();
            let rmap = crate::classifier::classify_with_luminance(
                luma,
                base.width(),
                base.height(),
                classification,
            );
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
                ArtifactMetric::PixelOutOfGamutRatio => {
                    crate::metrics::pixel_out_of_gamut_ratio(&base)
                }
            }
        }
    };
    let baseline_us = t0.elapsed().as_micros() as u64;

    // Evaluator — use precomputed luminance to avoid redundant extraction + full
    // feature computation.  `suggest_strength_from_luma` only computes Sobel
    // edge density (the single feature the strength heuristic uses).
    on_stage("evaluating");
    let t0 = Instant::now();
    let evaluator_cap = match &params.evaluator_config {
        Some(crate::types::EvaluatorConfig::Heuristic) => {
            let eval = crate::evaluator::HeuristicEvaluator;
            let luma = base_luminance.as_ref().unwrap();
            crate::evaluator::QualityEvaluator::suggest_strength_from_luma(
                &eval,
                luma,
                base.width() as usize,
                base.height() as usize,
            )
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
/// `probe_samples` may be supplied in any order; they are sorted internally by
/// ascending strength before fitting and solving.
///
/// If `pass_diagnostics` is `Some`, it is included in the output diagnostics
/// (useful for TwoPass parallel probing where JS resolved the dense window).
///
/// `detail` is the [`compute_probe_detail`] output for these same params, if
/// the caller still has it; the final sharpen then skips its own blur.  A
/// buffer of the wrong length for the sharpen mode is ignored and recomputed.
#[allow(clippy::too_many_arguments)]
pub fn process_from_prepared_with_probes(
    prepared: &PreparedBase,
    params: &AutoSharpParams,
    probe_samples: Vec<ProbeSample>,
    probing_us: u64,
    pass_diagnostics: Option<ProbePassDiagnostics>,
    detail: Option<Vec<f32>>,
    on_stage: &dyn Fn(&str),
) -> Result<ProcessOutput, CoreError> {
    let probe_result = ProbeResult {
        samples: probe_samples,
        pass_diagnostics,
        probing_us,
        detail,
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
        ProbeConfig::TwoPass {
            coarse_count,
            coarse_min,
            coarse_max,
            ..
        } => Ok(linspace(*coarse_min, *coarse_max, *coarse_count)),
        other => other.resolve(),
    }
}

/// Resolve the dense (second-pass) probe strengths from coarse results.
///
/// Only meaningful for [`ProbeConfig::TwoPass`].  Returns `Ok(None)` for other
/// configs (no second pass needed).
///
/// `coarse_samples` may be supplied in any order (e.g. collected out of order
/// by the parallel probe pool); they are sorted internally by ascending
/// strength before the crossing window is located.
pub fn resolve_dense_strengths(
    coarse_samples: &[ProbeSample],
    params: &AutoSharpParams,
    effective_p0: f32,
) -> Result<Option<(Vec<f32>, ProbePassDiagnostics)>, CoreError> {
    match &params.probe_strengths {
        ProbeConfig::TwoPass {
            coarse_count,
            coarse_min,
            coarse_max,
            dense_count,
            window_margin,
        } => {
            // Samples may arrive in any order (parallel probe pool); the
            // window search assumes ascending strength.
            let mut sorted = coarse_samples.to_vec();
            sorted.sort_by(|a, b| {
                a.strength
                    .partial_cmp(&b.strength)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let (dense_lo, dense_hi) = find_dense_window(
                &sorted,
                effective_p0,
                *coarse_min,
                *coarse_max,
                *window_margin,
            );
            let dense_strengths = linspace(dense_lo, dense_hi, *dense_count);
            let diag = ProbePassDiagnostics {
                coarse_count: *coarse_count,
                coarse_min: *coarse_min,
                coarse_max: *coarse_max,
                dense_count: *dense_count,
                dense_min: dense_lo,
                dense_max: dense_hi,
                coarse_probes_used: None,
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
    let detail = compute_detail(&base, Some(base_luminance), params.sharpen_mode, &kernel);
    probe_strengths(
        strengths,
        &base,
        Some(base_luminance),
        &detail,
        baseline_artifact_ratio,
        &ProbeSettings::from_params(params),
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
        SharpenMode::Lightness => Ok(sharpen::compute_detail_single_channel(
            base_luminance,
            w,
            h,
            &kernel,
        )),
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
    let w = width as usize;
    let h = height as usize;
    let ctx = ProbeContext {
        base: &base,
        base_luminance: Some(base_luminance),
        detail,
        baseline_artifact_ratio,
        settings: ProbeSettings::from_params(params),
    };
    let mut scratch = ProbeScratch {
        luma: match params.sharpen_mode {
            SharpenMode::Lightness => vec![0.0f32; w * h],
            SharpenMode::Rgb => Vec::new(),
        },
        rgb: LinearRgbImage::zeros(width, height)?,
    };
    let results = strengths
        .iter()
        .map(|&s| probe_one_reuse(&ctx, s, &mut scratch))
        .collect();
    Ok(results)
}

/// Internal result of the probing phase.
struct ProbeResult {
    samples: Vec<ProbeSample>,
    pass_diagnostics: Option<ProbePassDiagnostics>,
    probing_us: u64,
    /// Detail signal the probes used, reused by the final sharpen.
    detail: Option<Vec<f32>>,
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
    let settings = ProbeSettings::from_params(params);

    on_stage("probing");
    let t0 = Instant::now();
    let detail = compute_detail(base, base_luminance, params.sharpen_mode, &kernel);
    let (samples, pass_diagnostics) = match &params.probe_strengths {
        ProbeConfig::TwoPass {
            coarse_count,
            coarse_min,
            coarse_max,
            dense_count,
            window_margin,
        } => run_two_pass_probing(
            *coarse_count,
            *coarse_min,
            *coarse_max,
            *dense_count,
            *window_margin,
            effective_p0,
            base,
            base_luminance,
            baseline_artifact_ratio,
            &detail,
            &settings,
        )?,
        _ => {
            let strengths = params.probe_strengths.resolve()?;
            let samples = probe_strengths(
                &strengths,
                base,
                base_luminance,
                &detail,
                baseline_artifact_ratio,
                &settings,
            )?;
            (samples, None)
        }
    };
    let probing_us = t0.elapsed().as_micros() as u64;

    Ok(ProbeResult {
        samples,
        pass_diagnostics,
        probing_us,
        detail: Some(detail),
    })
}

/// Apply the configured clamp/normalize policy to the image.
fn apply_clamp_policy(image: &mut LinearRgbImage, policy: ClampPolicy) {
    match policy {
        ClampPolicy::Clamp => {
            for v in image.pixels_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        ClampPolicy::Normalize => {
            let max_val = image
                .pixels()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            // Divide by max(global maximum, 1.0): in-gamut images (max <= 1.0)
            // pass through unchanged; only values above 1.0 are compressed.
            // Negatives are floored to 0.
            let denom = max_val.max(1.0);
            for v in image.pixels_mut() {
                *v = (*v / denom).max(0.0);
            }
        }
    }
}

/// Apply the evaluator's advisory strength cap and record it when it binds.
///
/// The evaluator is an advisory *selection* stage: it may lower the final
/// strength below the solver's value.  Returns the (possibly capped) strength
/// and a diagnostic record that is `Some` only when the cap actually bound.
fn apply_evaluator_cap(
    solved_strength: f32,
    cap: Option<f32>,
) -> (f32, Option<EvaluatorCapDiagnostics>) {
    match cap {
        Some(c) if solved_strength > c => (
            c,
            Some(EvaluatorCapDiagnostics {
                cap: c,
                strength_before_cap: solved_strength,
            }),
        ),
        _ => (solved_strength, None),
    }
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
        move |img: &LinearRgbImage| -> f32 { crate::chroma_guard::evaluate_in_color_space(img, cs) }
    });
    let metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)> = eval_cs_fn
        .as_ref()
        .map(|f| f as &(dyn Fn(&LinearRgbImage) -> f32 + Sync));

    let ProbeResult {
        samples: mut probe_samples,
        pass_diagnostics: probe_pass_diagnostics,
        probing_us,
        detail,
    } = probe_result;

    // Samples may arrive unsorted (externally-collected parallel probes).
    // Downstream fit/solve/monotonicity/window logic assumes ascending
    // strength, so sort defensively (idempotent for already-sorted input).
    probe_samples.sort_by(|a, b| {
        a.strength
            .partial_cmp(&b.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // --- Fit + Solve ---
    let s_min = if matches!(params.metric_mode, MetricMode::RelativeToBase) {
        0.0_f64
    } else {
        probe_samples
            .first()
            .map(|s| s.strength as f64)
            .unwrap_or(0.05)
    };
    let s_max = probe_samples
        .last()
        .map(|s| s.strength as f64)
        .unwrap_or(3.0);
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
                &probe_samples,
                effective_p0,
                params.selection_policy,
            )?;
            (result, FitStatus::Skipped, None, None)
        }
        FitStrategy::Cubic => match fit_cubic_with_quality(&fit_data) {
            Ok((poly, quality)) => {
                let result = find_sharpness_with_policy(
                    &poly,
                    p0,
                    s_min,
                    s_max,
                    &probe_samples,
                    params.selection_policy,
                )?;
                if quality.r_squared < 0.85
                    && matches!(result.selection_mode, SelectionMode::PolynomialRoot)
                {
                    let direct = find_sharpness_direct_with_policy(
                        &probe_samples,
                        effective_p0,
                        params.selection_policy,
                    )?;
                    (direct, FitStatus::Success, Some(poly), Some(quality))
                } else {
                    (result, FitStatus::Success, Some(poly), Some(quality))
                }
            }
            Err(fit_err) => {
                let result = find_sharpness_direct_with_policy(
                    &probe_samples,
                    effective_p0,
                    params.selection_policy,
                )?;
                (
                    result,
                    FitStatus::Failed {
                        reason: fit_err.to_string(),
                    },
                    None,
                    None,
                )
            }
        },
    };
    let fit_us = t0.elapsed().as_micros() as u64;

    // --- LOO stability ---
    // Only a polynomial root is comparable to the refits' roots; a selected
    // sample strength (direct search, R² redirect, root out of range) is not.
    on_stage("robustness");
    let t0 = Instant::now();
    let loo = if matches!(solve_result.selection_mode, SelectionMode::PolynomialRoot) {
        let primary_s = solve_result.selected_strength as f64;
        loo_stability(&fit_data, p0, s_min, s_max, primary_s)
    } else {
        LooStability::NOT_RUN
    };
    let robustness_us = t0.elapsed().as_micros() as u64;

    let r_squared_ok = fit_quality.is_none_or(|q| q.r_squared > 0.85);
    let well_conditioned = fit_quality.is_none_or(|q| q.min_pivot > 1e-8);
    let robustness = Some(RobustnessFlags {
        monotonic,
        quasi_monotonic,
        r_squared_ok,
        well_conditioned,
        loo_stable: loo.stable,
        max_loo_root_change: loo.max_root_change,
        loo_coverage: loo.coverage,
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
        &selection_mode,
        &fit_status,
        budget_reachable_baseline,
        monotonic,
        r_squared_ok,
        params.fit_strategy,
        &solve_result.crossing_status,
    );

    // Evaluator cap — advisory selection stage; recorded when it binds.
    let (selected_strength, evaluator_cap_diag) =
        apply_evaluator_cap(solve_result.selected_strength, prepared.evaluator_cap);

    // --- Final sharpening ---
    on_stage("sharpening");
    let t0 = Instant::now();
    let px = base.width() as usize * base.height() as usize;
    let detail_len = match params.sharpen_mode {
        SharpenMode::Lightness => px,
        SharpenMode::Rgb => px * 3,
    };
    let detail = match detail {
        Some(d) if d.len() == detail_len => d,
        _ => compute_detail(base, base_luminance, params.sharpen_mode, &kernel),
    };
    let _chroma_guard_diag;
    let (mut final_image, adaptive_validation, adaptive_validation_us) =
        match (&params.sharpen_strategy, &prepared.gain_map) {
            (SharpenStrategy::Uniform, _) | (_, None) => {
                let image = sharpen_image(
                    base,
                    base_luminance,
                    params.sharpen_mode,
                    selected_strength,
                    &detail,
                );
                (image, None, None)
            }
            (
                SharpenStrategy::ContentAdaptive {
                    max_backoff_iterations,
                    backoff_scale_factor,
                    ..
                },
                Some(gm),
            ) => {
                let effective_max_backoff = if budget_reachable {
                    *max_backoff_iterations
                } else {
                    0
                };
                adaptive_sharpen_with_validation(
                    base,
                    base_luminance,
                    params.sharpen_mode,
                    selected_strength,
                    gm,
                    &detail,
                    effective_p0,
                    params.artifact_metric,
                    params.metric_mode,
                    baseline_artifact_ratio,
                    effective_max_backoff,
                    *backoff_scale_factor,
                    params.evaluation_color_space,
                )?
            }
        };

    // Chroma guard
    {
        if let Some(crate::types::ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift,
            chroma_region_factors,
            saturation_guard,
        }) = &params.experimental_sharpen_mode
        {
            let (guarded, cg_diag) = crate::chroma_guard::apply_chroma_guard(
                base,
                &final_image,
                *max_chroma_shift,
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
    //
    // The inspection path (Full diagnostics) computes the complete metric
    // breakdown (edge profiling, halo, overshoot, texture) and runs the
    // post-sharpen evaluator.  The output path (Summary) only needs the
    // selection metric (gamut check) for `measured_artifact_ratio`.
    let full_diagnostics = matches!(params.diagnostics_level, DiagnosticsLevel::Full);

    let (measured_artifact_ratio, final_breakdown) = if full_diagnostics {
        // Full: compute all four metric components.
        let fallback_luma;
        let base_luma = match prepared.base_luminance.as_deref() {
            Some(l) => l,
            None => {
                fallback_luma = color::extract_luminance(base);
                &fallback_luma
            }
        };
        let final_luma = color::extract_luminance(&final_image);
        let breakdown = crate::metrics::compute_metric_breakdown(
            &final_image,
            base,
            base_luma,
            &final_luma,
            params.artifact_metric,
            &params.metric_weights,
        );
        let ratio = match metric_override {
            Some(f) => f(&final_image),
            None => breakdown.selection_score,
        };
        (ratio, Some(breakdown))
    } else {
        // Summary: selection metric only — skip edge profiling, halo,
        // overshoot, texture, and the luminance extractions they require.
        let ratio = match metric_override {
            Some(f) => f(&final_image),
            None => crate::metrics::compute_selection_metric(&final_image, params.artifact_metric),
        };
        (ratio, None)
    };
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    // Evaluator (full diagnostics only — purely diagnostic, does not
    // alter the selection path or the output image).
    // Uses precomputed base luminance to avoid redundant extraction.
    let (_evaluator_result, _evaluator_process_us) = {
        if full_diagnostics {
            if let Some(ref eval_config) = params.evaluator_config {
                let t0 = Instant::now();
                let result = match eval_config {
                    crate::types::EvaluatorConfig::Heuristic => {
                        let eval = crate::evaluator::HeuristicEvaluator;
                        let luma = prepared
                            .base_luminance
                            .as_deref()
                            .expect("base_luminance is always set in prepare_base");
                        crate::evaluator::QualityEvaluator::evaluate_with_luma(
                            &eval,
                            base,
                            luma,
                            &final_image,
                            selected_strength,
                        )
                    }
                };
                let us = t0.elapsed().as_micros() as u64;
                (Some(result), Some(us))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    };

    // --- Clamp ---
    on_stage("finalizing");
    let t0 = Instant::now();
    apply_clamp_policy(&mut final_image, params.output_clamp);
    let clamp_us = t0.elapsed().as_micros() as u64;
    let total_us = pipeline_start.elapsed().as_micros() as u64;

    // --- Assemble diagnostics ---
    // Total timing includes pre-computed stages from PreparedBase.
    let full_total_us = total_us
        + probing_us
        + prepared.resize_us
        + prepared.base_quality_us
        + prepared.contrast_us
        + prepared.classification_us.unwrap_or(0)
        + prepared.baseline_us
        + prepared.evaluator_us.unwrap_or(0)
        + prepared.ingress_us.unwrap_or(0);

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
        metric_components: final_breakdown,
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
        resize_strategy_diagnostics: prepared.resize_strategy_diag.clone(), // contains Vec
        chroma_guard: _chroma_guard_diag,
        evaluator_result: _evaluator_result,
        evaluator_cap: evaluator_cap_diag,
        recommendations: Vec::new(),
        probe_pass_diagnostics,
        base_resize_quality: Some(prepared.base_resize_quality),
        effective_target_artifact_ratio: effective_p0,
        used_staged_shrink: prepared.used_staged_shrink,
        ingest: None,
    };

    diagnostics.recommendations =
        crate::recommendations::generate_recommendations(&diagnostics, params);

    Ok(ProcessOutput {
        image: final_image,
        diagnostics,
    })
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

/// Detail signal `D = input - blur(input)` for the sharpen mode: W×H floats
/// for Lightness, W×H×3 for RGB.  Independent of strength, so one blur serves
/// both probing and the final sharpen.
fn compute_detail(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    kernel: &[f32],
) -> Vec<f32> {
    match mode {
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            crate::sharpen::compute_detail_single_channel(
                lum,
                base.width() as usize,
                base.height() as usize,
                kernel,
            )
        }
        SharpenMode::Rgb => crate::sharpen::compute_detail_rgb(base, kernel),
    }
}

/// Apply uniform sharpening from a precomputed detail signal — the same
/// `input + amount × D` the probes measured.
fn sharpen_image(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    amount: f32,
    detail: &[f32],
) -> LinearRgbImage {
    match mode {
        SharpenMode::Rgb => crate::sharpen::apply_detail_rgb(base, detail, amount),
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let sharpened_l = crate::sharpen::apply_detail_single_channel(lum, detail, amount);
            color::reconstruct_rgb_from_lightness_with_luma(base, &sharpened_l, Some(lum))
        }
    }
}

/// Run all probe strengths and collect `ProbeSample`s.
///
/// Each probe applies `out = input + s × D` from the precomputed detail
/// signal (see [`compute_detail`]) — a trivial multiply-add — instead of
/// recomputing the separable blur for every strength.
fn probe_strengths(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    detail: &[f32],
    baseline_artifact_ratio: f32,
    settings: &ProbeSettings,
) -> Result<Vec<ProbeSample>, CoreError> {
    let w = base.width() as usize;
    let h = base.height() as usize;

    let ctx = ProbeContext {
        base,
        base_luminance,
        detail,
        baseline_artifact_ratio,
        settings: *settings,
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        let bw = base.width();
        let bh = base.height();
        let mode = settings.sharpen_mode;
        let results: Vec<ProbeSample> = strengths
            .par_iter()
            .map_init(
                move || ProbeScratch {
                    luma: match mode {
                        SharpenMode::Lightness => vec![0.0f32; w * h],
                        SharpenMode::Rgb => Vec::new(),
                    },
                    rgb: LinearRgbImage::zeros(bw, bh).unwrap(),
                },
                |scratch, &s| probe_one_reuse(&ctx, s, scratch),
            )
            .collect();
        Ok(results)
    }

    #[cfg(not(feature = "parallel"))]
    {
        let mut scratch = ProbeScratch {
            luma: match settings.sharpen_mode {
                SharpenMode::Lightness => vec![0.0f32; w * h],
                SharpenMode::Rgb => Vec::new(),
            },
            rgb: LinearRgbImage::zeros(base.width(), base.height())?,
        };
        let results = strengths
            .iter()
            .map(|&s| probe_one_reuse(&ctx, s, &mut scratch))
            .collect();
        Ok(results)
    }
}

/// Compute the metric value used for fitting, based on the configured mode.
#[inline]
fn compute_metric_value(p_total: f32, baseline: f32, mode: MetricMode) -> f32 {
    match mode {
        MetricMode::AbsoluteTotal => p_total,
        MetricMode::RelativeToBase => (p_total - baseline).max(0.0),
    }
}

// ---------------------------------------------------------------------------
// Leave-one-out stability check
// ---------------------------------------------------------------------------

/// Outcome of the leave-one-out root stability check.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LooStability {
    /// Every refit produced an in-range root and none moved beyond the
    /// stability threshold.
    stable: bool,
    /// Largest relative change among the refits that produced an in-range root.
    max_root_change: f64,
    /// Fraction of refits that produced an in-range root.
    coverage: f64,
}

impl LooStability {
    /// Reported when the check does not run: nothing counts against the fit.
    const NOT_RUN: Self = Self {
        stable: true,
        max_root_change: 0.0,
        coverage: 1.0,
    };
}

/// Refit the cubic N times (dropping one data point each time), re-solve for
/// the root, and report how far the root moves relative to `primary_s`, the
/// in-range root of the full fit.
///
/// A refit that cannot be fitted, or whose root leaves `[s_min, s_max]`, does
/// not show where the root moves — which is not evidence that it stays put.
/// Such refits lower [`LooStability::coverage`] and fail the check instead of
/// being skipped as maximally stable.
///
/// Returns [`LooStability::NOT_RUN`] for fewer than 5 points (a refit needs 4).
fn loo_stability(
    fit_data: &[(f64, f64)],
    p0: f64,
    s_min: f64,
    s_max: f64,
    primary_s: f64,
) -> LooStability {
    const LOO_THRESHOLD: f64 = 0.25; // 25% relative change

    let n = fit_data.len();
    if n < 5 {
        // Not enough data for meaningful LOO (need >=4 after dropping one).
        return LooStability::NOT_RUN;
    }

    let mut in_range = 0usize;
    let mut max_change = 0.0f64;
    for skip in 0..n {
        let subset: Vec<(f64, f64)> = fit_data
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != skip)
            .map(|(_, &v)| v)
            .collect();

        // A refit that fails here leaves `in_range` untouched on purpose.
        if let Ok(poly) = fit_cubic(&subset) {
            // Find the largest root in range for this refit.
            if let Ok(result) = find_sharpness_with_policy(
                &poly,
                p0,
                s_min,
                s_max,
                &[], // empty samples — force polynomial-only path or no fallback
                crate::SelectionPolicy::GamutOnly, // LOO only checks polynomial root stability
            ) {
                if matches!(result.selection_mode, SelectionMode::PolynomialRoot) {
                    in_range += 1;
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

    LooStability {
        stable: in_range == n && max_change < LOO_THRESHOLD,
        max_root_change: max_change,
        coverage: in_range as f64 / n as f64,
    }
}

// ---------------------------------------------------------------------------
// Fallback reason determination
// ---------------------------------------------------------------------------

/// Determine why the pipeline used a fallback instead of the polynomial root.
///
/// Returns `None` when `selection_mode == PolynomialRoot` (no fallback).
///
/// `FallbackReason::FitUnstable` is never returned: the leave-one-out check
/// runs only for a polynomial root, which takes the early return above.
fn determine_fallback_reason(
    selection_mode: &SelectionMode,
    fit_status: &FitStatus,
    budget_reachable_baseline: bool,
    monotonic: bool,
    r_squared_ok: bool,
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
    if matches!(crossing_status, crate::CrossingStatus::NotFoundInRange) {
        return Some(FallbackReason::RootOutOfRange);
    }

    // Catch-all for edge cases (shouldn't normally happen).
    Some(FallbackReason::RootOutOfRange)
}

// ---------------------------------------------------------------------------
// Adaptive sharpen + validate + backoff (Stage 9 + 9.5)
// ---------------------------------------------------------------------------

/// Run the adaptive sharpening backoff loop.
///
/// Calls `apply_at_scale(1.0)` first. If the metric exceeds `target_p0`,
/// iteratively reduces the scale by `backoff_factor` until budget is met
/// or `max_backoff` iterations are exhausted.
fn adaptive_backoff_loop(
    apply_at_scale: &dyn Fn(f32) -> LinearRgbImage,
    measure: &dyn Fn(&LinearRgbImage) -> f32,
    target_p0: f32,
    max_backoff: u8,
    backoff_factor: f32,
) -> (
    LinearRgbImage,
    Option<AdaptiveValidationOutcome>,
    Option<u64>,
) {
    let result = apply_at_scale(1.0);
    let p = measure(&result);

    let t_val = Instant::now();

    if p <= target_p0 {
        let val_us = t_val.elapsed().as_micros() as u64;
        return (
            result,
            Some(AdaptiveValidationOutcome::PassedDirect { measured_metric: p }),
            Some(val_us),
        );
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
            return (
                best_result,
                Some(AdaptiveValidationOutcome::PassedAfterBackoff {
                    iterations: i,
                    final_scale: scale,
                    measured_metric: p,
                }),
                Some(val_us),
            );
        }
    }

    let val_us = t_val.elapsed().as_micros() as u64;
    (
        best_result,
        Some(AdaptiveValidationOutcome::FailedBudgetExceeded {
            iterations: max_backoff,
            best_scale,
            best_metric,
        }),
        Some(val_us),
    )
}

/// Adaptive sharpen with validation and backoff loop.
///
/// Applies adaptive sharpening from the precomputed `detail` signal. If the
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
    detail: &[f32],
    target_p0: f32,
    artifact_metric: ArtifactMetric,
    metric_mode: MetricMode,
    baseline_artifact_ratio: f32,
    max_backoff: u8,
    backoff_factor: f32,
    evaluation_color_space: Option<crate::types::EvaluationColorSpace>,
) -> Result<
    (
        LinearRgbImage,
        Option<AdaptiveValidationOutcome>,
        Option<u64>,
    ),
    CoreError,
> {
    let measure = |img: &LinearRgbImage| -> f32 {
        let raw = if let Some(cs) = evaluation_color_space {
            crate::chroma_guard::evaluate_in_color_space(img, cs)
        } else {
            match artifact_metric {
                ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
                ArtifactMetric::PixelOutOfGamutRatio => {
                    crate::metrics::pixel_out_of_gamut_ratio(img)
                }
            }
        };
        compute_metric_value(raw, baseline_artifact_ratio, metric_mode)
    };

    match sharpen_mode {
        SharpenMode::Lightness => {
            let luma = base_luminance.expect("luminance required for lightness mode");
            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let sharpened_l = crate::sharpen::apply_adaptive_lightness_from_detail(
                    luma,
                    detail,
                    global_strength * scale,
                    gain_map,
                );
                crate::color::reconstruct_rgb_from_lightness_with_luma(
                    base,
                    &sharpened_l,
                    Some(luma),
                )
            };

            Ok(adaptive_backoff_loop(
                &apply_at_scale,
                &measure,
                target_p0,
                max_backoff,
                backoff_factor,
            ))
        }

        SharpenMode::Rgb => {
            let src_px = base.pixels();
            let gain_data = gain_map.data();

            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let eff_strength = global_strength * scale;
                let out: Vec<f32> = src_px
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .zip(detail.as_chunks::<3>().0.iter())
                    .zip(gain_data.iter())
                    .flat_map(|((s, d), &g)| {
                        let eff = eff_strength * g;
                        [s[0] + eff * d[0], s[1] + eff * d[1], s[2] + eff * d[2]]
                    })
                    .collect();
                LinearRgbImage::new(base.width(), base.height(), out).unwrap()
            };

            Ok(adaptive_backoff_loop(
                &apply_at_scale,
                &measure,
                target_p0,
                max_backoff,
                backoff_factor,
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

/// Minimum coarse probes before early-stop is eligible.
///
/// We need enough points to locate a bracket.  3 probes gives at least
/// 2 intervals to check, plus a data point on each side of the bracket.
///
/// **Monotonicity assumption:** the early-stop check looks for an upward P0
/// crossing (`prev <= P0 && curr > P0`), which requires the metric to be
/// roughly monotonically increasing with strength.  This holds for the
/// unsharp-mask artifact ratio on well-behaved images.  On images with
/// non-monotonic metric response, early stopping may miss a later bracket,
/// but the dense pass still runs and the fit + solve recover gracefully.
const MIN_COARSE_BEFORE_EARLY_STOP: usize = 3;

/// Two-pass probing: coarse scan over `[coarse_min, coarse_max]`, then dense
/// probing in a refined window around the estimated P0 crossing.
///
/// **Early stopping:** after the first [`MIN_COARSE_BEFORE_EARLY_STOP`] probes,
/// the coarse scan exits as soon as a P0 crossing bracket is found, saving
/// the remaining probes at the tail of the range.
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
    baseline_artifact_ratio: f32,
    detail: &[f32],
    settings: &ProbeSettings,
) -> Result<(Vec<ProbeSample>, Option<ProbePassDiagnostics>), CoreError> {
    let coarse_strengths = linspace(coarse_min, coarse_max, coarse_count);

    // Scratch buffers and context reused across all coarse + dense probes.
    let w = base.width() as usize;
    let h = base.height() as usize;
    let ctx = ProbeContext {
        base,
        base_luminance,
        detail,
        baseline_artifact_ratio,
        settings: *settings,
    };
    let mut scratch = ProbeScratch {
        luma: match settings.sharpen_mode {
            SharpenMode::Lightness => vec![0.0f32; w * h],
            SharpenMode::Rgb => Vec::new(),
        },
        rgb: LinearRgbImage::zeros(base.width(), base.height())?,
    };

    // Phase 1: coarse with early stopping
    let mut coarse_samples = Vec::with_capacity(coarse_count);
    for &s in &coarse_strengths {
        coarse_samples.push(probe_one_reuse(&ctx, s, &mut scratch));

        // Early-stop: once we have enough probes and a P0 bracket, skip the rest.
        if coarse_samples.len() >= MIN_COARSE_BEFORE_EARLY_STOP {
            let n = coarse_samples.len();
            let prev = coarse_samples[n - 2].metric_value;
            let curr = coarse_samples[n - 1].metric_value;
            if prev <= p0 && curr > p0 {
                break;
            }
        }
    }
    let coarse_probes_used = coarse_samples.len();

    // Locate dense window from coarse results
    let (dense_lo, dense_hi) =
        find_dense_window(&coarse_samples, p0, coarse_min, coarse_max, window_margin);

    // Phase 2: dense — reuse the precomputed detail and scratch buffers
    let dense_strengths = linspace(dense_lo, dense_hi, dense_count);
    let mut dense_samples = Vec::with_capacity(dense_count);
    for &s in &dense_strengths {
        dense_samples.push(probe_one_reuse(&ctx, s, &mut scratch));
    }

    // Merge, sort, remove near-duplicates (within 1e-5 of each other)
    let mut all: Vec<ProbeSample> = coarse_samples;
    all.extend(dense_samples);
    all.sort_by(|a, b| {
        a.strength
            .partial_cmp(&b.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all.dedup_by(|a, b| (a.strength - b.strength).abs() < 1e-5);

    let early_stopped = coarse_probes_used < coarse_count;
    let diag = ProbePassDiagnostics {
        coarse_count,
        coarse_min,
        coarse_max,
        dense_count,
        dense_min: dense_lo,
        dense_max: dense_hi,
        coarse_probes_used: if early_stopped {
            Some(coarse_probes_used)
        } else {
            None
        },
    };
    Ok((all, Some(diag)))
}

/// Immutable context shared across all probes in a single probing phase.
struct ProbeContext<'a> {
    base: &'a LinearRgbImage,
    base_luminance: Option<&'a [f32]>,
    detail: &'a [f32],
    /// Artifact ratio of the unsharpened base, which `RelativeToBase` subtracts.
    baseline_artifact_ratio: f32,
    /// Everything else a measurement reads.
    settings: ProbeSettings,
}

/// Mutable scratch buffers reused across probes to avoid per-probe allocation.
///
/// - `luma`: intermediate sharpened luminance buffer; only written in
///   Lightness mode (must have length >= W×H). Empty in RGB mode.
/// - `rgb`: output image buffer. Must have the same dimensions as `base`.
struct ProbeScratch {
    luma: Vec<f32>,
    rgb: LinearRgbImage,
}

/// Evaluate a single probe from precomputed detail, reusing scratch buffers.
fn probe_one_reuse(
    ctx: &ProbeContext<'_>,
    strength: f32,
    scratch: &mut ProbeScratch,
) -> ProbeSample {
    use crate::sharpen;

    match ctx.settings.sharpen_mode {
        SharpenMode::Lightness => {
            let lum = ctx
                .base_luminance
                .expect("base_luminance required for Lightness mode");
            sharpen::apply_detail_single_channel_into(lum, ctx.detail, strength, &mut scratch.luma);
            color::reconstruct_rgb_from_lightness_into(
                ctx.base,
                &scratch.luma,
                lum,
                &mut scratch.rgb,
            );
        }
        SharpenMode::Rgb => {
            sharpen::apply_detail_rgb_into(ctx.base, ctx.detail, strength, &mut scratch.rgb);
        }
    }

    // The same measurement for in-process and worker probes: an evaluation
    // color space replaces the default metric when one is configured.
    let p_total = match ctx.settings.evaluation_color_space {
        Some(cs) => crate::chroma_guard::evaluate_in_color_space(&scratch.rgb, cs),
        None => {
            crate::metrics::compute_selection_metric(&scratch.rgb, ctx.settings.artifact_metric)
        }
    };
    let metric_value = compute_metric_value(
        p_total,
        ctx.baseline_artifact_ratio,
        ctx.settings.metric_mode,
    );

    let breakdown = if ctx.settings.needs_probe_breakdown() {
        // luma_sharpened: reuse scratch.luma in Lightness mode; extract in RGB mode.
        let sharp_luma_owned;
        let sharp_luma: &[f32] = match ctx.settings.sharpen_mode {
            SharpenMode::Lightness => &scratch.luma,
            SharpenMode::Rgb => {
                sharp_luma_owned = color::extract_luminance(&scratch.rgb);
                &sharp_luma_owned
            }
        };
        // luma_original: prefer precomputed base luminance, else extract.
        let base_luma_owned;
        let base_luma: &[f32] = match ctx.base_luminance {
            Some(l) => l,
            None => {
                base_luma_owned = color::extract_luminance(ctx.base);
                &base_luma_owned
            }
        };
        Some(crate::metrics::compute_metric_breakdown(
            &scratch.rgb,
            ctx.base,
            base_luma,
            sharp_luma,
            ctx.settings.artifact_metric,
            &ctx.settings.metric_weights(),
        ))
    } else {
        None
    };

    ProbeSample {
        strength,
        artifact_ratio: p_total,
        metric_value,
        breakdown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClampPolicy;

    #[test]
    fn evaluator_cap_records_when_it_binds() {
        use crate::EvaluatorCapDiagnostics;

        // Cap below solver strength → capped + recorded.
        let (s, diag) = apply_evaluator_cap(1.0, Some(0.25));
        assert!((s - 0.25).abs() < 1e-6);
        let d: EvaluatorCapDiagnostics = diag.expect("cap should be recorded");
        assert!((d.cap - 0.25).abs() < 1e-6);
        assert!((d.strength_before_cap - 1.0).abs() < 1e-6);

        // Cap at/above solver strength → no change, no record.
        let (s, diag) = apply_evaluator_cap(0.2, Some(0.25));
        assert!((s - 0.2).abs() < 1e-6);
        assert!(diag.is_none());

        // No cap → no change, no record.
        let (s, diag) = apply_evaluator_cap(1.0, None);
        assert!((s - 1.0).abs() < 1e-6);
        assert!(diag.is_none());
    }

    #[test]
    fn normalize_leaves_in_gamut_image_unchanged() {
        // Max is 0.9 (< 1.0): the image is already in gamut, so it must pass
        // through unchanged rather than being brightened.
        let mut img = LinearRgbImage::new(1, 2, vec![0.1, 0.5, 0.9, 0.2, 0.4, 0.6]).unwrap();
        let before = img.pixels().to_vec();
        apply_clamp_policy(&mut img, ClampPolicy::Normalize);
        for (a, b) in before.iter().zip(img.pixels()) {
            assert!((a - b).abs() < 1e-6, "in-gamut value changed: {a} -> {b}");
        }
    }

    #[test]
    fn normalize_compresses_out_of_range_and_floors_negatives() {
        // Max is 2.0 (> 1.0): everything scales by 1/2.0; the negative floors to 0.
        let mut img = LinearRgbImage::new(1, 2, vec![2.0, 1.0, 0.0, -0.5, 0.5, 0.5]).unwrap();
        apply_clamp_policy(&mut img, ClampPolicy::Normalize);
        let p = img.pixels();
        assert!((p[0] - 1.0).abs() < 1e-6); // 2.0 / 2.0
        assert!((p[1] - 0.5).abs() < 1e-6); // 1.0 / 2.0
        assert!((p[3] - 0.0).abs() < 1e-6); // -0.5 floored to 0
    }

    fn gradient(w: u32, h: u32) -> LinearRgbImage {
        let mut data = vec![0.0f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = x as f32 / w as f32;
                data[idx + 1] = y as f32 / h as f32;
                data[idx + 2] = 0.5;
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn total_time_includes_probing() {
        // 256x256 source so probing over ~11 probes is reliably > 1µs.
        let img = gradient(256, 256);
        let params = AutoSharpParams::photo(64, 64);
        let out = crate::process_auto_sharp_downscale(&img, &params).unwrap();
        let t = &out.diagnostics.timing;
        assert!(t.probing_us > 0, "probing_us should be measured");
        assert!(
            t.total_us >= t.probing_us,
            "total_us ({}) must include probing_us ({})",
            t.total_us,
            t.probing_us
        );
    }

    fn sample(strength: f32, metric_value: f32) -> ProbeSample {
        ProbeSample {
            strength,
            artifact_ratio: metric_value,
            metric_value,
            breakdown: None,
        }
    }

    #[test]
    fn resolve_dense_strengths_tolerates_unsorted_input() {
        // TwoPass params so resolve_dense_strengths returns Some.
        let params = AutoSharpParams::photo(16, 16);

        // Crossing of p0=0.005 lies between strengths 0.5 (0.002) and 0.8 (0.010).
        let sorted = vec![
            sample(0.2, 0.001),
            sample(0.5, 0.002),
            sample(0.8, 0.010),
            sample(1.0, 0.020),
        ];
        let mut unsorted = sorted.clone();
        unsorted.swap(0, 3);
        unsorted.swap(1, 2);

        let a = resolve_dense_strengths(&sorted, &params, 0.005).unwrap();
        let b = resolve_dense_strengths(&unsorted, &params, 0.005).unwrap();
        // Same dense window regardless of input order.
        assert_eq!(a.unwrap().1.dense_min, b.unwrap().1.dense_min);
    }

    // -----------------------------------------------------------------------
    // AutoSharpParams field roles
    // -----------------------------------------------------------------------

    /// Fields that change the [`PreparedBase`]; [`BaseParamsKey`] must cover
    /// exactly these.  `target` is one key field holding the first two.
    const BASE_AFFECTING: [&str; 10] = [
        "target_width",
        "target_height",
        "target_artifact_ratio",
        "enable_contrast_leveling",
        "sharpen_strategy",
        "input_color_space",
        "resize_strategy",
        "evaluation_color_space",
        "artifact_metric",
        "evaluator_config",
    ];

    /// Fields only the probe measurement reads.
    const PROBE_ONLY: [&str; 5] = [
        "sharpen_sigma",
        "sharpen_mode",
        "metric_mode",
        "metric_weights",
        "selection_policy",
    ];

    /// Fields read by neither the base nor the probes.
    const NEITHER: [&str; 6] = [
        "probe_strengths",
        "fit_strategy",
        "output_clamp",
        "diagnostics_level",
        "experimental_sharpen_mode",
        "pipeline_mode",
    ];

    /// Fields whose value a probe measurement reads.  Two of them also change
    /// the base, so they are listed in [`BASE_AFFECTING`] as well.
    const PROBE_SETTINGS_FIELDS: [&str; 7] = [
        "sharpen_sigma",
        "sharpen_mode",
        "metric_mode",
        "artifact_metric",
        "metric_weights",
        "selection_policy",
        "evaluation_color_space",
    ];

    /// Compile-time tripwire for the three role lists.  The destructure has
    /// no `..`, so a new field fails to compile until it is listed; a binding
    /// left out of every array is an unused-variable error under `-D warnings`.
    #[test]
    fn every_param_field_has_a_role() {
        let AutoSharpParams {
            target_width,
            target_height,
            probe_strengths,
            target_artifact_ratio,
            enable_contrast_leveling,
            sharpen_sigma,
            fit_strategy,
            output_clamp,
            sharpen_mode,
            metric_mode,
            artifact_metric,
            metric_weights,
            selection_policy,
            diagnostics_level,
            sharpen_strategy,
            input_color_space,
            resize_strategy,
            experimental_sharpen_mode,
            evaluation_color_space,
            evaluator_config,
            pipeline_mode,
        } = AutoSharpParams::default();

        // One `&dyn Debug` per binding, grouped and ordered as the constants
        // list their names.
        let base_affecting: [&dyn std::fmt::Debug; BASE_AFFECTING.len()] = [
            &target_width,
            &target_height,
            &target_artifact_ratio,
            &enable_contrast_leveling,
            &sharpen_strategy,
            &input_color_space,
            &resize_strategy,
            &evaluation_color_space,
            &artifact_metric,
            &evaluator_config,
        ];
        let probe_only: [&dyn std::fmt::Debug; PROBE_ONLY.len()] = [
            &sharpen_sigma,
            &sharpen_mode,
            &metric_mode,
            &metric_weights,
            &selection_policy,
        ];
        let neither: [&dyn std::fmt::Debug; NEITHER.len()] = [
            &probe_strengths,
            &fit_strategy,
            &output_clamp,
            &diagnostics_level,
            &experimental_sharpen_mode,
            &pipeline_mode,
        ];
        assert_eq!(base_affecting.len() + probe_only.len() + neither.len(), 21);
    }

    /// Change one named field to another valid value.
    fn toggle_param(name: &str, params: &mut AutoSharpParams) {
        use crate::{
            ArtifactMetric, ClassificationParams, DiagnosticsLevel, EvaluationColorSpace,
            EvaluatorConfig, ExperimentalSharpenMode, FitStrategy, GainTable, InputColorSpace,
            MetricMode, PipelineMode, ProbeConfig, ResizeKernel, ResizeStrategy, SelectionPolicy,
            SharpenMode, SharpenStrategy,
        };
        match name {
            "target_width" => params.target_width += 7,
            "target_height" => params.target_height += 7,
            "target_artifact_ratio" => {
                params.target_artifact_ratio = if params.target_artifact_ratio == 0.5 {
                    0.25
                } else {
                    0.5
                };
            }
            "enable_contrast_leveling" => {
                params.enable_contrast_leveling = !params.enable_contrast_leveling;
            }
            "sharpen_strategy" => {
                params.sharpen_strategy = match params.sharpen_strategy {
                    SharpenStrategy::Uniform => SharpenStrategy::ContentAdaptive {
                        classification: ClassificationParams::default(),
                        gain_table: GainTable::v03_default(),
                        max_backoff_iterations: 4,
                        backoff_scale_factor: 0.8,
                    },
                    SharpenStrategy::ContentAdaptive { .. } => SharpenStrategy::Uniform,
                };
            }
            "input_color_space" => {
                params.input_color_space = match params.input_color_space {
                    None => Some(InputColorSpace::LinearRgb),
                    Some(_) => None,
                };
            }
            "resize_strategy" => {
                params.resize_strategy = match params.resize_strategy {
                    None => Some(ResizeStrategy::Uniform {
                        kernel: ResizeKernel::CatmullRom,
                    }),
                    Some(_) => None,
                };
            }
            "evaluation_color_space" => {
                params.evaluation_color_space = match params.evaluation_color_space {
                    None => Some(EvaluationColorSpace::LumaOnly),
                    Some(_) => None,
                };
            }
            "artifact_metric" => {
                params.artifact_metric = match params.artifact_metric {
                    ArtifactMetric::ChannelClippingRatio => ArtifactMetric::PixelOutOfGamutRatio,
                    ArtifactMetric::PixelOutOfGamutRatio => ArtifactMetric::ChannelClippingRatio,
                };
            }
            "evaluator_config" => {
                params.evaluator_config = match params.evaluator_config {
                    None => Some(EvaluatorConfig::Heuristic),
                    Some(_) => None,
                };
            }
            "sharpen_sigma" => params.sharpen_sigma = 1.5,
            "sharpen_mode" => {
                params.sharpen_mode = match params.sharpen_mode {
                    SharpenMode::Lightness => SharpenMode::Rgb,
                    SharpenMode::Rgb => SharpenMode::Lightness,
                };
            }
            "metric_mode" => {
                params.metric_mode = match params.metric_mode {
                    MetricMode::RelativeToBase => MetricMode::AbsoluteTotal,
                    MetricMode::AbsoluteTotal => MetricMode::RelativeToBase,
                };
            }
            "metric_weights" => params.metric_weights.gamut_excursion = 2.0,
            "selection_policy" => {
                params.selection_policy = match params.selection_policy {
                    SelectionPolicy::GamutOnly => SelectionPolicy::Hybrid,
                    _ => SelectionPolicy::GamutOnly,
                };
            }
            "probe_strengths" => {
                params.probe_strengths = ProbeConfig::Explicit(vec![0.25, 0.5, 1.0, 2.0]);
            }
            "fit_strategy" => {
                params.fit_strategy = match params.fit_strategy {
                    FitStrategy::Cubic => FitStrategy::DirectSearch,
                    FitStrategy::DirectSearch => FitStrategy::Cubic,
                };
            }
            "output_clamp" => {
                params.output_clamp = match params.output_clamp {
                    ClampPolicy::Clamp => ClampPolicy::Normalize,
                    ClampPolicy::Normalize => ClampPolicy::Clamp,
                };
            }
            "diagnostics_level" => {
                params.diagnostics_level = match params.diagnostics_level {
                    DiagnosticsLevel::Summary => DiagnosticsLevel::Full,
                    DiagnosticsLevel::Full => DiagnosticsLevel::Summary,
                };
            }
            "experimental_sharpen_mode" => {
                params.experimental_sharpen_mode = if params.experimental_sharpen_mode.is_none() {
                    Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
                        max_chroma_shift: 0.25,
                        chroma_region_factors: None,
                        saturation_guard: None,
                    })
                } else {
                    None
                };
            }
            "pipeline_mode" => {
                params.pipeline_mode = match params.pipeline_mode {
                    None => Some(PipelineMode::Fast),
                    Some(_) => None,
                };
            }
            other => panic!("`{other}` is not in any role list"),
        }
    }

    fn prepared_photo() -> PreparedBase {
        prepare_base(&gradient(48, 36), &AutoSharpParams::photo(16, 12), &|_| {}).unwrap()
    }

    #[test]
    fn base_params_key_rejects_a_change_in_any_base_affecting_field() {
        let prepared = prepared_photo();
        for name in BASE_AFFECTING {
            let mut changed = AutoSharpParams::photo(16, 12);
            toggle_param(name, &mut changed);
            assert!(
                !prepared.matches_params(&changed),
                "`{name}` changes the prepared base but BaseParamsKey ignores it"
            );
        }
    }

    #[test]
    fn base_params_key_ignores_every_field_that_leaves_the_base_unchanged() {
        let prepared = prepared_photo();
        assert!(prepared.matches_params(&AutoSharpParams::photo(16, 12)));
        for name in PROBE_ONLY.into_iter().chain(NEITHER) {
            let mut changed = AutoSharpParams::photo(16, 12);
            toggle_param(name, &mut changed);
            assert!(
                prepared.matches_params(&changed),
                "`{name}` leaves the prepared base unchanged but BaseParamsKey rejects it"
            );
        }
    }

    #[test]
    fn probe_settings_change_exactly_when_a_probe_setting_field_changes() {
        let base = AutoSharpParams::photo(16, 12);
        let settings = ProbeSettings::from_params(&base);
        let all: Vec<&str> = BASE_AFFECTING
            .iter()
            .chain(PROBE_ONLY.iter())
            .chain(NEITHER.iter())
            .copied()
            .collect();
        assert_eq!(all.len(), 21);
        for name in all {
            let mut changed = base.clone();
            toggle_param(name, &mut changed);
            assert_eq!(
                ProbeSettings::from_params(&changed) != settings,
                PROBE_SETTINGS_FIELDS.contains(&name),
                "`{name}`"
            );
        }
    }

    #[test]
    fn probes_carry_breakdown_only_under_composite_policy() {
        use crate::SelectionPolicy;

        let base = gradient(24, 24);
        let luma = color::extract_luminance(&base);
        let strengths = [0.5f32, 1.0, 2.0, 3.0];

        // GamutOnly (default): fast path, no per-probe breakdown.
        let mut params = AutoSharpParams::photo(24, 24);
        params.selection_policy = SelectionPolicy::GamutOnly;
        let gamut = crate::run_probes_standalone(
            base.pixels(),
            base.width(),
            base.height(),
            &luma,
            &strengths,
            &params,
            0.0,
        )
        .unwrap();
        assert!(gamut.iter().all(|s| s.breakdown.is_none()));

        // Hybrid: per-probe breakdown must be populated.
        params.selection_policy = SelectionPolicy::Hybrid;
        let hybrid = crate::run_probes_standalone(
            base.pixels(),
            base.width(),
            base.height(),
            &luma,
            &strengths,
            &params,
            0.0,
        )
        .unwrap();
        assert!(hybrid.iter().all(|s| s.breakdown.is_some()));
    }

    // -----------------------------------------------------------------------
    // LOO stability
    // -----------------------------------------------------------------------

    /// Six probes on `y = 0.001·s²` — a quadratic, so every leave-one-out
    /// refit reproduces it exactly.
    fn quadratic_points() -> Vec<(f64, f64)> {
        [0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
            .iter()
            .map(|&s| (s, 0.001 * s * s))
            .collect()
    }

    /// The primary root the way the pipeline derives it: the largest in-range
    /// root of the full fit.
    fn primary_root(data: &[(f64, f64)], p0: f64, s_min: f64, s_max: f64) -> f64 {
        use crate::SelectionPolicy;

        let poly = fit_cubic(data).unwrap();
        let result =
            find_sharpness_with_policy(&poly, p0, s_min, s_max, &[], SelectionPolicy::GamutOnly)
                .unwrap();
        assert_eq!(
            result.selection_mode,
            SelectionMode::PolynomialRoot,
            "test data must have an in-range root"
        );
        result.selected_strength as f64
    }

    #[test]
    fn loo_stable_fit_reports_small_change() {
        // A quartic perturbation keeps the data off any exact cubic, so the
        // refits disagree slightly: every root stays within 0.3% of the
        // primary (independent least-squares check).
        let data: Vec<(f64, f64)> = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
            .iter()
            .map(|&s: &f64| (s, 0.001 * s * s + 0.0001 * s.powi(4)))
            .collect();
        let (s_min, s_max, p0) = (0.0, 3.0, 0.004);
        let primary_s = primary_root(&data, p0, s_min, s_max);

        let loo = loo_stability(&data, p0, s_min, s_max, primary_s);

        assert!(loo.stable);
        assert!(
            (loo.max_root_change - 0.0025).abs() < 5e-4,
            "max_root_change = {}",
            loo.max_root_change
        );
    }

    #[test]
    fn loo_influential_point_is_unstable() {
        // One probe at 3x its trend value drags the full-fit root from √2 to
        // ≈2.05.  The refits land between ≈1.09 (dropping the s = 0.5 probe)
        // and √2 (dropping the influential one) — up to 47% away.
        let mut data = quadratic_points();
        data[5].1 = 0.03;
        let (s_min, s_max, p0) = (0.0, 3.0, 0.002);
        let primary_s = primary_root(&data, p0, s_min, s_max);

        let loo = loo_stability(&data, p0, s_min, s_max, primary_s);

        assert!(!loo.stable);
        assert!(
            loo.max_root_change > 0.25,
            "max_root_change = {}",
            loo.max_root_change
        );
    }

    #[test]
    fn loo_near_zero_primary_uses_absolute_change() {
        // Refits of exact quadratic data all root at √(0.004 / 0.001) = 2.
        // With a primary of 0 the relative form would divide by zero; the
        // absolute form reports the 2.0 shift instead.
        let data = quadratic_points();

        let loo = loo_stability(&data, 0.004, 0.0, 3.0, 0.0);

        assert!(!loo.stable);
        assert!(
            (loo.max_root_change - 2.0).abs() < 1e-6,
            "max_root_change = {}",
            loo.max_root_change
        );
    }

    #[test]
    fn loo_with_fewer_than_five_points_is_not_run() {
        let data = quadratic_points()[..4].to_vec();

        let loo = loo_stability(&data, 0.004, 0.0, 1.5, 2.0);

        assert_eq!(
            loo,
            LooStability {
                stable: true,
                max_root_change: 0.0,
                coverage: 1.0,
            }
        );
    }

    #[test]
    fn loo_refit_root_leaving_range_is_unstable() {
        // Five probes lie on y = 0.001·s² and P0 = 0.010, so the refit that
        // drops the raised top probe is that exact quadratic and roots at
        // √10 ≈ 3.162 — outside the probed range [0, 3]. Every other refit
        // stays within 4% of the primary root, so the escaped root is the
        // only evidence of instability and must not be skipped.
        let mut data = quadratic_points();
        data[5].1 = 0.02;
        let (s_min, s_max, p0) = (0.0, 3.0, 0.010);
        let primary_s = primary_root(&data, p0, s_min, s_max);

        let loo = loo_stability(&data, p0, s_min, s_max, primary_s);

        assert!(
            !loo.stable,
            "a refit root that leaves the range must count against stability (max_root_change = {})",
            loo.max_root_change
        );
        assert!(
            loo.max_root_change < 0.25,
            "max_root_change = {}",
            loo.max_root_change
        );
    }

    #[test]
    fn loo_unfittable_refit_is_unstable() {
        // Every refit leaves four points on at most two distinct strengths —
        // a singular fit, so no refit says where the root moves.  That is not
        // evidence that the root stays put.
        let data = vec![
            (1.0, 0.001),
            (2.0, 0.004),
            (2.0, 0.004),
            (2.0, 0.004),
            (2.0, 0.004),
        ];

        let loo = loo_stability(&data, 0.004, 0.0, 3.0, 2.0);

        assert!(
            !loo.stable,
            "an unfittable refit must count against stability (max_root_change = {})",
            loo.max_root_change
        );
    }

    #[test]
    fn loo_reports_coverage_of_in_range_roots() {
        // In the escaping-root data exactly one of six refits loses its root.
        let mut data = quadratic_points();
        data[5].1 = 0.02;
        let (s_min, s_max, p0) = (0.0, 3.0, 0.010);
        let primary_s = primary_root(&data, p0, s_min, s_max);

        let loo = loo_stability(&data, p0, s_min, s_max, primary_s);

        assert!(
            (loo.coverage - 5.0 / 6.0).abs() < 1e-9,
            "coverage = {}",
            loo.coverage
        );

        // Full coverage when every refit roots in range.
        let data = quadratic_points();
        let primary_s = primary_root(&data, 0.002, 0.0, 3.0);
        let loo = loo_stability(&data, 0.002, 0.0, 3.0, primary_s);
        assert_eq!(loo.coverage, 1.0);
    }
}
