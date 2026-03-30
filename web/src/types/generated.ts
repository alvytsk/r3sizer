// Auto-generated from r3sizer-core Rust types. DO NOT EDIT.
//
// Regenerate with:
//   cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
//
// Any manual TypeScript types (e.g. ProcessResult) belong in wasm-types.ts,
// which re-exports everything from this file.

/* eslint-disable */
/* prettier-ignore */

export type SharpenMode = "rgb" | "lightness";

export type MetricMode = "absolute_total" | "relative_to_base";

export type ArtifactMetric = "channel_clipping_ratio" | "pixel_out_of_gamut_ratio";

export type FitStrategy = "Cubic" | "DirectSearch";

export type ClampPolicy = "Clamp" | "Normalize";

export type DiagnosticsLevel = "summary" | "full";

export type CrossingStatus = "found" | "not_found_in_range" | "not_attempted";

export type SelectionMode = "polynomial_root" | "best_sample_within_budget" | "least_bad_sample" | "budget_unreachable";

export type SelectionPolicy = "gamut_only" | "hybrid" | "composite_only";

export type FallbackReason = "fit_failed" | "fit_unstable" | "root_out_of_range" | "metric_non_monotonic" | "budget_too_strict_for_content" | "direct_search_configured" | "fit_poor_quality";

export type MetricComponent = "gamut_excursion" | "halo_ringing" | "edge_overshoot" | "texture_flattening";

export type RegionClass = "flat" | "textured" | "strong_edge" | "microtexture" | "risky_halo_zone";

export type ImageSize = { width: number, height: number, };

export type MetricWeights = { gamut_excursion: number, halo_ringing: number, edge_overshoot: number, texture_flattening: number, };

export type GainTable = { flat: number, textured: number, strong_edge: number, microtexture: number, risky_halo_zone: number, };

export type ClassificationParams = { gradient_low_threshold: number, gradient_high_threshold: number, variance_low_threshold: number, variance_high_threshold: number, variance_window: number, };

export type ProbeConfig = { "Range": { min: number, max: number, count: number, } } | { "Explicit": Array<number> } | { "TwoPass": { 
/**
 * Number of uniformly-spaced probes in the first (coarse) pass. Min: 3.
 */
coarse_count: number, 
/**
 * Lower bound of the coarse scan range (exclusive, must be > 0).
 */
coarse_min: number, 
/**
 * Upper bound of the coarse scan range (must be > `coarse_min`).
 */
coarse_max: number, 
/**
 * Number of probes in the second (dense) pass. Min: 2.
 */
dense_count: number, 
/**
 * How far to extend the crossing bracket on each side when building the
 * dense window, expressed as a fraction of the bracket width.
 * E.g. `0.5` extends by half the coarse interval on each side.
 */
window_margin: number, } };

export type SharpenStrategy = { "strategy": "uniform" } | { "strategy": "content_adaptive", classification: ClassificationParams, gain_table: GainTable, 
/**
 * Maximum backoff iterations if adaptive result exceeds budget. Default: 4.
 */
max_backoff_iterations: number, 
/**
 * Scale reduction per backoff iteration. Must be in (0.0, 1.0). Default: 0.8.
 */
backoff_scale_factor: number, };

export type AutoSharpParams = { target_width: number, target_height: number, 
/**
 * How to select sharpening probe strengths.
 */
probe_strengths: ProbeConfig, 
/**
 * Target artifact ratio P0 (fraction of channel values outside [0,1]).
 * Default: 0.001 (= 0.1%).
 */
target_artifact_ratio: number, 
/**
 * Enable the contrast-leveling post-process stage.
 */
enable_contrast_leveling: boolean, 
/**
 * Unsharp-mask Gaussian sigma. Default: 1.0.
 */
sharpen_sigma: number, fit_strategy: FitStrategy, output_clamp: ClampPolicy, 
/**
 * Whether to sharpen RGB directly or through lightness channel.
 */
sharpen_mode: SharpenMode, 
/**
 * How the artifact metric is computed for strength selection.
 */
metric_mode: MetricMode, 
/**
 * Which artifact metric function to use. Default: `ChannelClippingRatio`.
 */
artifact_metric: ArtifactMetric, 
/**
 * Weights for the composite diagnostic metric. Default: [1.0, 0.3, 0.3, 0.1].
 */
metric_weights: MetricWeights, 
/**
 * How the solver ranks fallback candidates. Default: `GamutOnly`.
 */
selection_policy: SelectionPolicy, 
/**
 * Verbosity level for serialized diagnostics.
 */
diagnostics_level: DiagnosticsLevel, 
/**
 * Strength distribution strategy. Default: `Uniform`.
 */
sharpen_strategy: SharpenStrategy, 
/**
 * Input color space declaration. Default: `None` (= Srgb).
 */
input_color_space?: InputColorSpace | null, 
/**
 * Resize kernel strategy. Default: `None` (= Lanczos3).
 */
resize_strategy?: ResizeStrategy | null, 
/**
 * Extended sharpening mode with chroma guard. Default: `None`.
 */
experimental_sharpen_mode?: ExperimentalSharpenMode | null, 
/**
 * Color space for artifact evaluation. Default: `None` (= Rgb).
 */
evaluation_color_space?: EvaluationColorSpace | null, 
/**
 * Quality evaluator configuration. Default: `None` (disabled).
 */
evaluator_config?: EvaluatorConfig | null, };

export type CubicPolynomial = { a: number, b: number, c: number, d: number, };

export type FitQuality = { 
/**
 * Sum of squared residuals between fitted polynomial and data points.
 */
residual_sum_of_squares: number, 
/**
 * Coefficient of determination (1.0 = perfect fit, 0.0 = no better than mean).
 */
r_squared: number, 
/**
 * Largest absolute residual among all data points.
 */
max_residual: number, 
/**
 * Smallest pivot encountered during Gaussian elimination (condition proxy).
 */
min_pivot: number, };

export type RobustnessFlags = { 
/**
 * P(s) is non-decreasing across all probes.
 */
monotonic: boolean, 
/**
 * At most one monotonicity violation.
 */
quasi_monotonic: boolean, 
/**
 * R^2 exceeds the quality threshold (0.85).
 */
r_squared_ok: boolean, 
/**
 * Fit matrix is well-conditioned (min_pivot > threshold).
 */
well_conditioned: boolean, 
/**
 * Leave-one-out root is stable (max relative change < threshold).
 */
loo_stable: boolean, 
/**
 * Largest relative change in s* across leave-one-out refits.
 */
max_loo_root_change: number, };

export type FitStatus = { "status": "success" } | { "status": "failed", reason: string, } | { "status": "skipped" };

export type MetricBreakdown = { 
/**
 * Individual component scores.
 */
components: { [key in MetricComponent]?: number }, 
/**
 * Which metric drove solver selection (GamutExcursion in v0.2).
 */
selected_metric: MetricComponent, 
/**
 * The value of the selected metric.
 */
selection_score: number, 
/**
 * Weighted composite score (diagnostic only in v0.2 — not used for selection).
 */
composite_score: number, 
/**
 * Legacy alias for `selection_score`. Kept for backward compatibility.
 */
aggregate: number, };

export type ProbeSample = { 
/**
 * Sharpening strength `s`.
 */
strength: number, 
/**
 * P_total(s): fraction of channel components outside [0, 1] after sharpening.
 */
artifact_ratio: number, 
/**
 * The metric value used for fitting and selection, depending on `MetricMode`:
 * - `AbsoluteTotal`: same as `artifact_ratio`
 * - `RelativeToBase`: `max(0, artifact_ratio - baseline)`
 */
metric_value: number, 
/**
 * Per-component breakdown of the artifact metric (v0.2 scaffold).
 */
breakdown?: MetricBreakdown | null, };

export type StageTiming = { resize_us: number, contrast_us: number, baseline_us: number, 
/**
 * Wall-clock time for the entire probe loop (parallel or sequential).
 */
probing_us: number, fit_us: number, robustness_us: number, final_sharpen_us: number, clamp_us: number, total_us: number, 
/**
 * Region classification time (None when Uniform).
 */
classification_us?: number | null, 
/**
 * Adaptive validation + backoff time (None when Uniform).
 */
adaptive_validation_us?: number | null, 
/**
 * Input color-space ingress time (None when not configured).
 */
ingress_us?: number | null, 
/**
 * Evaluator execution time (None when not configured).
 */
evaluator_us?: number | null, 
/**
 * Base resize quality scoring time (step 4).
 */
base_quality_us?: number | null, };

export type RegionCoverage = { total_pixels: number, flat: number, textured: number, strong_edge: number, microtexture: number, risky_halo_zone: number, flat_fraction: number, textured_fraction: number, strong_edge_fraction: number, microtexture_fraction: number, risky_halo_zone_fraction: number, };

export type AdaptiveValidationOutcome = { "outcome": "passed_direct", measured_metric: number, } | { "outcome": "passed_after_backoff", iterations: number, final_scale: number, measured_metric: number, } | { "outcome": "failed_budget_exceeded", iterations: number, best_scale: number, best_metric: number, };

export type ProbePassDiagnostics = { 
/**
 * Number of probes fired in the coarse pass.
 */
coarse_count: number, 
/**
 * Coarse pass range lower bound (= `ProbeConfig::TwoPass::coarse_min`).
 */
coarse_min: number, 
/**
 * Coarse pass range upper bound (= `ProbeConfig::TwoPass::coarse_max`).
 */
coarse_max: number, 
/**
 * Number of probes fired in the dense pass.
 */
dense_count: number, 
/**
 * Dense window lower bound selected after coarse bracket search.
 */
dense_min: number, 
/**
 * Dense window upper bound selected after coarse bracket search.
 */
dense_max: number, };

export type BaseResizeQuality = { 
/**
 * Fraction of source Sobel edge energy preserved in the resized image.
 * Scale-independent per-pixel energy ratio; higher is better.
 * Diagnostic only in v1 — does not affect solver budget.
 */
edge_retention: number, 
/**
 * Fraction of source local texture variance preserved in the resized image.
 * Computed via 5×5 window mean variance ratio; higher is better.
 * Diagnostic only in v1 — does not affect solver budget.
 */
texture_retention: number, 
/**
 * Fraction of near-edge pixels showing sign-flip oscillation (ringing proxy).
 * Higher is worse.  Active in v1: drives `envelope_scale`.
 */
ringing_score: number, 
/**
 * Budget multiplier applied to `target_artifact_ratio` before probing:
 * `effective_p0 = target_artifact_ratio × envelope_scale`.
 * Derived as `clamp(1.0 − 2.0 × ringing_score, 0.65, 1.0)`.
 */
envelope_scale: number, };

export type AutoSharpDiagnostics = { input_size: ImageSize, output_size: ImageSize, sharpen_mode: SharpenMode, metric_mode: MetricMode, artifact_metric: ArtifactMetric, 
/**
 * Which selection policy was used for fallback ranking.
 */
selection_policy: SelectionPolicy, target_artifact_ratio: number, 
/**
 * Artifact ratio of the downscaled image before any sharpening is applied.
 */
baseline_artifact_ratio: number, probe_samples: Array<ProbeSample>, fit_status: FitStatus, fit_coefficients: CubicPolynomial | null, 
/**
 * Quality metrics for the polynomial fit (residuals, R², condition).
 */
fit_quality?: FitQuality | null, crossing_status: CrossingStatus, 
/**
 * Robustness flags computed from probe data and fit quality.
 */
robustness?: RobustnessFlags | null, 
/**
 * Sharpening strength that was applied to produce the final image.
 */
selected_strength: number, selection_mode: SelectionMode, 
/**
 * Why the polynomial root was not used (None when selection_mode == PolynomialRoot).
 */
fallback_reason?: FallbackReason | null, 
/**
 * Whether the target artifact budget is achievable given the baseline and probe range.
 */
budget_reachable: boolean, 
/**
 * P_total(s*) on the final sharpened image, before clamping.
 */
measured_artifact_ratio: number, 
/**
 * Metric value of the final output (relative or absolute depending on mode).
 */
measured_metric_value: number, 
/**
 * Per-component breakdown of the final artifact metric.
 */
metric_components?: MetricBreakdown | null, 
/**
 * Weights used for composite score computation.
 */
metric_weights: MetricWeights, 
/**
 * Per-class region coverage. None when `SharpenStrategy::Uniform`.
 */
region_coverage?: RegionCoverage | null, 
/**
 * Outcome of adaptive validation. None when `SharpenStrategy::Uniform`.
 */
adaptive_validation?: AdaptiveValidationOutcome | null, 
/**
 * Per-stage wall-clock timing.
 */
timing: StageTiming, 
/**
 * Input color-space ingress diagnostics.
 */
input_ingress?: InputIngressDiagnostics | null, 
/**
 * Resize strategy diagnostics.
 */
resize_strategy_diagnostics?: ResizeStrategyDiagnostics | null, 
/**
 * Chroma guard diagnostics.
 */
chroma_guard?: ChromaGuardDiagnostics | null, 
/**
 * Quality evaluator result (advisory).
 */
evaluator_result?: QualityEvaluation | null, 
/**
 * Actionable recommendations derived from pipeline diagnostics.
 */
recommendations?: Array<Recommendation>, 
/**
 * Coarse/dense pass diagnostics. Present only when `ProbeConfig::TwoPass` was used.
 */
probe_pass_diagnostics?: ProbePassDiagnostics | null, 
/**
 * Quality assessment of the base resized image before sharpening.
 */
base_resize_quality?: BaseResizeQuality | null, 
/**
 * Effective target artifact ratio after applying the ringing-aware envelope.
 * `effective = target_artifact_ratio × base_resize_quality.envelope_scale`.
 * Equals `target_artifact_ratio` when `base_resize_quality` is `None`.
 */
effective_target_artifact_ratio: number, };

export type InputColorSpace = "srgb" | "linear_rgb" | "raw_linear";

export type ResizeKernel = "lanczos3" | "mitchell_netravali" | "catmull_rom" | "gaussian";

export type KernelTable = { flat: ResizeKernel, textured: ResizeKernel, strong_edge: ResizeKernel, microtexture: ResizeKernel, risky_halo_zone: ResizeKernel, };

export type ResizeStrategy = { "strategy": "uniform", kernel: ResizeKernel, } | { "strategy": "content_adaptive", classification: ClassificationParams, kernel_table: KernelTable, };

export type ResizeStrategyDiagnostics = { 
/**
 * Which distinct kernels were actually used.
 */
kernels_used: Array<ResizeKernel>, 
/**
 * Per-kernel pixel count in the output image.
 */
per_kernel_pixel_count: { [key in string]: number }, };

export type ChromaRegionFactors = { flat: number, textured: number, strong_edge: number, microtexture: number, risky_halo_zone: number, };

export type SaturationGuardParams = { 
/**
 * Minimum scale factor applied to fully-saturated pixels. Default: 0.6.
 */
min_scale: number, 
/**
 * Gamma exponent controlling the saturation→scale curve. Default: 1.5.
 */
gamma: number, };

export type ExperimentalSharpenMode = { "luma_plus_chroma_guard": { 
/**
 * Maximum allowed chroma shift as a fraction of original chroma magnitude.
 * Values above this trigger soft clamping. Default: 0.10 (10%).
 */
max_chroma_shift: number, 
/**
 * Per-region-class multipliers for `max_chroma_shift`.
 * Only effective when the pipeline also produces a region map
 * (i.e. `SharpenStrategy::ContentAdaptive`).
 */
chroma_region_factors?: ChromaRegionFactors | null, 
/**
 * Saturation-dependent threshold tightening.
 * Active regardless of region map availability.
 */
saturation_guard?: SaturationGuardParams | null, } };

export type EvaluationColorSpace = "rgb" | "luma_only" | "lab_approx";

export type ChromaRegionClampStats = { pixel_count: number, clamped_count: number, clamped_fraction: number, mean_shift: number, max_shift: number, };

export type ChromaPerRegionDiagnostics = { flat: ChromaRegionClampStats, textured: ChromaRegionClampStats, strong_edge: ChromaRegionClampStats, microtexture: ChromaRegionClampStats, risky_halo_zone: ChromaRegionClampStats, };

export type ChromaGuardDiagnostics = { 
/**
 * Fraction of pixels where chroma soft-clamping was applied.
 */
pixels_clamped_fraction: number, 
/**
 * Mean chroma shift magnitude across all pixels.
 */
mean_chroma_shift: number, 
/**
 * Maximum chroma shift magnitude.
 */
max_chroma_shift: number, 
/**
 * Minimum effective threshold across all pixels.
 */
effective_threshold_min?: number | null, 
/**
 * Mean effective threshold across all pixels.
 */
effective_threshold_mean?: number | null, 
/**
 * Maximum effective threshold across all pixels.
 */
effective_threshold_max?: number | null, 
/**
 * Per-region-class clamp statistics.
 * Present only when a region map was available (ContentAdaptive strategy).
 */
per_region?: ChromaPerRegionDiagnostics | null, };

export type EvaluatorConfig = "heuristic";

export type ImageFeatures = { 
/**
 * Fraction of pixels classified as edges (Sobel magnitude > threshold).
 */
edge_density: number, 
/**
 * Mean Sobel gradient magnitude across all pixels.
 */
mean_gradient_magnitude: number, 
/**
 * Variance of gradient magnitudes.
 */
gradient_variance: number, 
/**
 * Mean local variance (5×5 window) across all pixels.
 */
mean_local_variance: number, 
/**
 * Variance of local variances (texture heterogeneity).
 */
local_variance_variance: number, 
/**
 * Variance of the Laplacian response (frequency content proxy).
 */
laplacian_variance: number, 
/**
 * Shannon entropy of the 64-bin luminance histogram.
 */
luminance_histogram_entropy: number, };

export type QualityEvaluation = { 
/**
 * Predicted overall quality score in [0, 1] (higher = better).
 */
predicted_quality_score: number, 
/**
 * Optional suggested sharpening strength (advisory, not enforced).
 */
suggested_strength?: number | null, 
/**
 * Confidence in the prediction, in [0, 1].
 */
confidence: number, 
/**
 * Raw feature vector used for the prediction.
 */
features: ImageFeatures, };

export type InputIngressDiagnostics = { 
/**
 * Which color space was declared by the caller.
 */
declared_color_space: InputColorSpace, 
/**
 * (min, max) of raw channel values. Present for `RawLinear`.
 */
raw_value_min?: number | null, raw_value_max?: number | null, 
/**
 * Scale factor applied to bring values into [0, 1]. Present for `RawLinear`.
 */
normalization_scale?: number | null, 
/**
 * Fraction of values > 1.0. Present for `LinearRgb` validation.
 */
out_of_range_fraction?: number | null, };

export type RecommendationKind = "switch_to_content_adaptive" | "lower_strong_edge_gain" | "raise_artifact_budget" | "switch_to_lightness" | "widen_probe_range" | "lower_sigma" | "switch_to_hybrid";

export type Severity = "info" | "suggestion" | "warning";

export type ParamPatch = { sharpen_strategy?: SharpenStrategy | null, target_artifact_ratio?: number | null, sharpen_mode?: SharpenMode | null, probe_strengths?: ProbeConfig | null, sharpen_sigma?: number | null, selection_policy?: SelectionPolicy | null, };

export type Recommendation = { kind: RecommendationKind, severity: Severity, 
/**
 * Confidence in \[0, 1\].  Display-only — does not affect patch content.
 */
confidence: number, 
/**
 * Human-readable explanation of why this recommendation was generated.
 */
reason: string, 
/**
 * Self-contained param patch.  Apply via `updateParams(patch)`.
 */
patch: ParamPatch, };

// ── Default constants (generated from Rust Default impls) ──

export const DEFAULT_METRIC_WEIGHTS: MetricWeights = {
  "gamut_excursion": 1.0,
  "halo_ringing": 0.3,
  "edge_overshoot": 0.3,
  "texture_flattening": 0.1
};

export const DEFAULT_GAIN_TABLE: GainTable = {
  "flat": 0.75,
  "textured": 0.95,
  "strong_edge": 1.0,
  "microtexture": 1.1,
  "risky_halo_zone": 0.7
};

export const DEFAULT_CLASSIFICATION_PARAMS: ClassificationParams = {
  "gradient_low_threshold": 0.05,
  "gradient_high_threshold": 0.4,
  "variance_low_threshold": 0.001,
  "variance_high_threshold": 0.01,
  "variance_window": 5
};

export const DEFAULT_SHARPEN_STRATEGY: SharpenStrategy = {
  "strategy": "uniform"
};

export const DEFAULT_PARAMS: AutoSharpParams = {
  "target_width": 800,
  "target_height": 600,
  "probe_strengths": {
    "TwoPass": {
      "coarse_count": 7,
      "coarse_min": 0.003,
      "coarse_max": 1.0,
      "dense_count": 4,
      "window_margin": 0.5
    }
  },
  "target_artifact_ratio": 0.003,
  "enable_contrast_leveling": false,
  "sharpen_sigma": 1.0,
  "fit_strategy": "Cubic",
  "output_clamp": "Clamp",
  "sharpen_mode": "lightness",
  "metric_mode": "relative_to_base",
  "artifact_metric": "channel_clipping_ratio",
  "metric_weights": {
    "gamut_excursion": 1.0,
    "halo_ringing": 0.3,
    "edge_overshoot": 0.3,
    "texture_flattening": 0.1
  },
  "selection_policy": "gamut_only",
  "diagnostics_level": "summary",
  "sharpen_strategy": {
    "strategy": "content_adaptive",
    "classification": {
      "gradient_low_threshold": 0.05,
      "gradient_high_threshold": 0.4,
      "variance_low_threshold": 0.001,
      "variance_high_threshold": 0.01,
      "variance_window": 5
    },
    "gain_table": {
      "flat": 0.75,
      "textured": 0.95,
      "strong_edge": 1.0,
      "microtexture": 1.1,
      "risky_halo_zone": 0.7
    },
    "max_backoff_iterations": 4,
    "backoff_scale_factor": 0.8
  },
  "experimental_sharpen_mode": {
    "luma_plus_chroma_guard": {
      "max_chroma_shift": 0.25,
      "chroma_region_factors": {
        "flat": 1.0,
        "textured": 0.9,
        "strong_edge": 0.65,
        "microtexture": 0.8,
        "risky_halo_zone": 0.45
      },
      "saturation_guard": {
        "min_scale": 0.6,
        "gamma": 1.5
      }
    }
  },
  "evaluator_config": "heuristic"
};

export const DEFAULT_KERNEL_TABLE: KernelTable = {
  "flat": "gaussian",
  "textured": "lanczos3",
  "strong_edge": "lanczos3",
  "microtexture": "catmull_rom",
  "risky_halo_zone": "mitchell_netravali"
};

export const DEFAULT_CHROMA_REGION_FACTORS: ChromaRegionFactors = {
  "flat": 1.0,
  "textured": 0.9,
  "strong_edge": 0.65,
  "microtexture": 0.8,
  "risky_halo_zone": 0.45
};

export const DEFAULT_SATURATION_GUARD_PARAMS: SaturationGuardParams = {
  "min_scale": 0.6,
  "gamma": 1.5
};
