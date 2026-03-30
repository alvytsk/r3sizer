use serde::{Deserialize, Serialize};

#[cfg(feature = "typegen")]
use ts_rs::TS;

use crate::CoreError;

// ---------------------------------------------------------------------------
// Image representation
// ---------------------------------------------------------------------------

/// Owned linear-RGB image buffer.
///
/// Pixel layout: interleaved `[R, G, B, R, G, B, …]` in row-major order.
/// Values are nominally in `[0.0, 1.0]` but intermediate processing stages
/// intentionally allow values outside that range (e.g. after sharpening).
/// Clamping to the valid range happens only at the final output stage.
#[derive(Debug, Clone)]
pub struct LinearRgbImage {
    width: u32,
    height: u32,
    /// Length == width * height * 3.
    data: Vec<f32>,
}

impl LinearRgbImage {
    /// Create a new image. Returns an error if `data.len() != width * height * 3`
    /// or if either dimension is zero.
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::EmptyImage);
        }
        let expected = (width as usize) * (height as usize) * 3;
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Create an all-zero (black) image of the given size.
    pub fn zeros(width: u32, height: u32) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::EmptyImage);
        }
        let len = (width as usize) * (height as usize) * 3;
        Ok(Self { width, height, data: vec![0.0f32; len] })
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Read-only flat slice of all pixel components.
    pub fn pixels(&self) -> &[f32] { &self.data }

    /// Mutable flat slice of all pixel components.
    pub fn pixels_mut(&mut self) -> &mut [f32] { &mut self.data }

    /// Total number of f32 components (width * height * 3).
    pub fn total_components(&self) -> usize { self.data.len() }

    /// Read-only view of scan-line `y` (0-indexed).
    pub fn row(&self, y: u32) -> &[f32] {
        let start = (y as usize) * (self.width as usize) * 3;
        let end = start + (self.width as usize) * 3;
        &self.data[start..end]
    }

    /// Mutable view of scan-line `y` (0-indexed).
    pub fn row_mut(&mut self, y: u32) -> &mut [f32] {
        let stride = (self.width as usize) * 3;
        let start = (y as usize) * stride;
        let end = start + stride;
        &mut self.data[start..end]
    }

    pub fn size(&self) -> ImageSize {
        ImageSize { width: self.width, height: self.height }
    }

    /// Consume the image and return the underlying flat buffer.
    pub fn into_data(self) -> Vec<f32> { self.data }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Sharpening and metric configuration
// ---------------------------------------------------------------------------

/// How sharpening is applied to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum SharpenMode {
    /// Apply unsharp mask directly to all RGB channels.
    Rgb,
    /// Apply unsharp mask to CIE Y lightness, reconstruct RGB via multiplicative
    /// ratio `k = L'/L`.
    ///
    /// Engineering approximation -- the reconstruction formula is a strong inference
    /// from the paper, not a confirmed exact formula.
    Lightness,
}

/// How the artifact metric is computed for sharpness selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum MetricMode {
    /// P_total(s): absolute fraction of channel values outside [0,1].
    /// Includes artifacts from both the resize stage and the sharpen stage.
    AbsoluteTotal,
    /// max(0, P_total(s) - P_base): additional artifacts attributable to sharpening.
    ///
    /// Engineering approximation -- assumes resize and sharpen artifacts are approximately
    /// additive and independent, which is not guaranteed.
    RelativeToBase,
}

/// Which artifact metric function to use for measuring out-of-range values.
///
/// Orthogonal to [`MetricMode`] (which selects absolute vs relative comparison).
/// `ArtifactMetric` selects *what* is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum ArtifactMetric {
    /// Per-channel: fraction of f32 channel values outside [0,1]. Denominator = W*H*3.
    ChannelClippingRatio,
    /// Per-pixel: fraction of pixels where *any* channel is outside [0,1]. Denominator = W*H.
    PixelOutOfGamutRatio,
}

// ---------------------------------------------------------------------------
// Solver / diagnostics status enums
// ---------------------------------------------------------------------------

/// Status of the polynomial fit attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum FitStatus {
    /// Cubic polynomial was fitted successfully.
    Success,
    /// Fitting failed for a numerical or data reason.
    Failed { reason: String },
    /// Fitting was skipped (e.g. DirectSearch strategy).
    Skipped,
}

/// Whether the polynomial crossing P_hat(s*) = P0 was found in the probe interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum CrossingStatus {
    /// A root was found inside [s_min, s_max].
    Found,
    /// No crossing exists inside the probed interval.
    NotFoundInRange,
    /// Polynomial fit was not attempted or failed; crossing search was skipped.
    NotAttempted,
}

/// How the final sharpening strength was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Optimal s* from the cubic polynomial root.
    PolynomialRoot,
    /// Polynomial root not available; selected largest probe sample within artifact budget.
    BestSampleWithinBudget,
    /// All probe samples exceed budget; selected the sample with the smallest metric value.
    LeastBadSample,
    /// Budget is structurally unreachable (e.g. baseline already exceeds target in absolute mode).
    BudgetUnreachable,
}

// ---------------------------------------------------------------------------
// Processing parameters
// ---------------------------------------------------------------------------

/// Controls which sharpening strengths are probed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub enum ProbeConfig {
    /// `count` values linearly spaced over `[min, max]`.
    Range { min: f32, max: f32, count: usize },
    /// Caller-supplied explicit list (must have >= 4 distinct, positive values).
    Explicit(Vec<f32>),
    /// Two-pass adaptive strategy: coarse scan over the full range followed by
    /// dense probing in a narrow window around the estimated P(s) = P0 crossing.
    ///
    /// The coarse pass brackets the crossing; the dense pass refines it.
    /// All collected samples are merged and fed into the existing cubic fit path.
    TwoPass {
        /// Number of uniformly-spaced probes in the first (coarse) pass. Min: 3.
        coarse_count: usize,
        /// Lower bound of the coarse scan range (exclusive, must be > 0).
        coarse_min: f32,
        /// Upper bound of the coarse scan range (must be > `coarse_min`).
        coarse_max: f32,
        /// Number of probes in the second (dense) pass. Min: 2.
        dense_count: usize,
        /// How far to extend the crossing bracket on each side when building the
        /// dense window, expressed as a fraction of the bracket width.
        /// E.g. `0.5` extends by half the coarse interval on each side.
        window_margin: f32,
    },
}

impl ProbeConfig {
    /// Resolve static configs (`Range`, `Explicit`) to a sorted `Vec<f32>`.
    ///
    /// Returns `Err` for `TwoPass` — that variant requires measurement between
    /// passes and is handled directly in the pipeline.
    pub fn resolve(&self) -> Result<Vec<f32>, CoreError> {
        let mut values = match self {
            ProbeConfig::Range { min, max, count } => {
                if *count < 4 {
                    return Err(CoreError::InvalidParams(
                        "probe range must have at least 4 samples".into(),
                    ));
                }
                if min >= max {
                    return Err(CoreError::InvalidParams(
                        "probe range min must be less than max".into(),
                    ));
                }
                if *min <= 0.0 {
                    return Err(CoreError::InvalidParams(
                        "probe range min must be positive".into(),
                    ));
                }
                let n = *count;
                (0..n)
                    .map(|i| min + (max - min) * (i as f32) / ((n - 1) as f32))
                    .collect::<Vec<f32>>()
            }
            ProbeConfig::Explicit(v) => {
                if v.len() < 4 {
                    return Err(CoreError::InvalidParams(
                        "explicit probe list must have at least 4 values".into(),
                    ));
                }
                v.clone()
            }
            ProbeConfig::TwoPass { .. } => {
                return Err(CoreError::InvalidParams(
                    "TwoPass probe config is handled by the pipeline; use AutoSharpParams directly".into(),
                ));
            }
        };
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(values)
    }
}

/// Diagnostics from the two-pass adaptive probe placement strategy.
///
/// Present only when `ProbeConfig::TwoPass` was used; `None` for static configs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ProbePassDiagnostics {
    /// Number of probes fired in the coarse pass.
    pub coarse_count: usize,
    /// Coarse pass range lower bound (= `ProbeConfig::TwoPass::coarse_min`).
    pub coarse_min: f32,
    /// Coarse pass range upper bound (= `ProbeConfig::TwoPass::coarse_max`).
    pub coarse_max: f32,
    /// Number of probes fired in the dense pass.
    pub dense_count: usize,
    /// Dense window lower bound selected after coarse bracket search.
    pub dense_min: f32,
    /// Dense window upper bound selected after coarse bracket search.
    pub dense_max: f32,
}

/// Quality assessment of the resized image before any sharpening is applied.
///
/// Computed in the pipeline immediately after downscaling, using both the source
/// and resized images.  In v1 only `ringing_score` is active (drives
/// `envelope_scale`); `edge_retention` and `texture_retention` are diagnostic.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct BaseResizeQuality {
    /// Fraction of source Sobel edge energy preserved in the resized image.
    /// Scale-independent per-pixel energy ratio; higher is better.
    /// Diagnostic only in v1 — does not affect solver budget.
    pub edge_retention: f32,
    /// Fraction of source local texture variance preserved in the resized image.
    /// Computed via 5×5 window mean variance ratio; higher is better.
    /// Diagnostic only in v1 — does not affect solver budget.
    pub texture_retention: f32,
    /// Fraction of near-edge pixels showing sign-flip oscillation (ringing proxy).
    /// Higher is worse.  Active in v1: drives `envelope_scale`.
    pub ringing_score: f32,
    /// Budget multiplier applied to `target_artifact_ratio` before probing:
    /// `effective_p0 = target_artifact_ratio × envelope_scale`.
    /// Derived as `clamp(1.0 − 2.0 × ringing_score, 0.65, 1.0)`.
    pub envelope_scale: f32,
}

/// Polynomial fit strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub enum FitStrategy {
    /// Least-squares cubic fit; fall back to direct sampled search if numerically unstable.
    Cubic,
    /// Skip fitting; pick best strength directly from probe samples.
    DirectSearch,
}

/// How to handle out-of-range values at the final output stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub enum ClampPolicy {
    /// Hard clamp: values < 0.0 -> 0.0, values > 1.0 -> 1.0.
    Clamp,
    /// Rescale entire image by its global maximum.
    Normalize,
}

/// All parameters controlling the auto-sharpness downscale pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct AutoSharpParams {
    pub target_width: u32,
    pub target_height: u32,
    /// How to select sharpening probe strengths.
    pub probe_strengths: ProbeConfig,
    /// Target artifact ratio P0 (fraction of channel values outside [0,1]).
    /// Default: 0.001 (= 0.1%).
    pub target_artifact_ratio: f32,
    /// Enable the contrast-leveling post-process stage.
    pub enable_contrast_leveling: bool,
    /// Unsharp-mask Gaussian sigma. Default: 1.0.
    pub sharpen_sigma: f32,
    pub fit_strategy: FitStrategy,
    pub output_clamp: ClampPolicy,
    /// Whether to sharpen RGB directly or through lightness channel.
    pub sharpen_mode: SharpenMode,
    /// How the artifact metric is computed for strength selection.
    pub metric_mode: MetricMode,
    /// Which artifact metric function to use. Default: `ChannelClippingRatio`.
    pub artifact_metric: ArtifactMetric,
    /// Weights for the composite diagnostic metric. Default: [1.0, 0.3, 0.3, 0.1].
    pub metric_weights: MetricWeights,
    /// Verbosity level for serialized diagnostics.
    pub diagnostics_level: DiagnosticsLevel,
    /// Strength distribution strategy. Default: `Uniform`.
    #[serde(default)]
    pub sharpen_strategy: SharpenStrategy,

    // --- Experimental (v0.4) ---

    /// Input color space declaration. Default: `None` (= Srgb).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_color_space: Option<InputColorSpace>,

    /// Resize kernel strategy. Default: `None` (= Lanczos3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize_strategy: Option<ResizeStrategy>,

    /// Extended sharpening mode with chroma guard. Default: `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_sharpen_mode: Option<ExperimentalSharpenMode>,

    /// Color space for artifact evaluation. Default: `None` (= Rgb).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_color_space: Option<EvaluationColorSpace>,

    /// Quality evaluator configuration. Default: `None` (disabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_config: Option<EvaluatorConfig>,
}

impl Default for AutoSharpParams {
    /// Default is the **Photo** preset: P0=0.003, two-pass probing up to 1.0,
    /// content-adaptive sharpening with chroma guard and heuristic evaluator.
    fn default() -> Self {
        Self {
            target_width: 800,
            target_height: 600,
            probe_strengths: ProbeConfig::TwoPass {
                coarse_count: 7,
                coarse_min: 0.003,
                coarse_max: 1.00,
                dense_count: 4,
                window_margin: 0.5,
            },
            target_artifact_ratio: 0.003,
            enable_contrast_leveling: false,
            sharpen_sigma: 1.0,
            fit_strategy: FitStrategy::Cubic,
            output_clamp: ClampPolicy::Clamp,
            sharpen_mode: SharpenMode::Lightness,
            metric_mode: MetricMode::RelativeToBase,
            artifact_metric: ArtifactMetric::ChannelClippingRatio,
            metric_weights: MetricWeights::default(),
            diagnostics_level: DiagnosticsLevel::default(),
            sharpen_strategy: SharpenStrategy::ContentAdaptive {
                classification: ClassificationParams::default(),
                gain_table: GainTable::v03_default(),
                max_backoff_iterations: 4,
                backoff_scale_factor: 0.8,
            },
            input_color_space: None,
            resize_strategy: None,
            experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
                max_chroma_shift: 0.25,
                chroma_region_factors: Some(ChromaRegionFactors::default()),
                saturation_guard: Some(SaturationGuardParams::default()),
            }),
            evaluation_color_space: None,
            evaluator_config: Some(EvaluatorConfig::Heuristic),
        }
    }
}

impl AutoSharpParams {
    /// **Photo** preset: P0=0.003, two-pass probing [0.003, 1.00].
    /// Intended for natural photographic content.
    pub fn photo(target_width: u32, target_height: u32) -> Self {
        Self {
            target_width,
            target_height,
            ..Self::default()
        }
    }

    /// **Precision** preset: P0=0.001, two-pass probing [0.003, 0.50].
    /// Intended for text, UI, architecture, and hard-edge content.
    pub fn precision(target_width: u32, target_height: u32) -> Self {
        Self {
            target_width,
            target_height,
            target_artifact_ratio: 0.001,
            probe_strengths: ProbeConfig::TwoPass {
                coarse_count: 7,
                coarse_min: 0.003,
                coarse_max: 0.50,
                dense_count: 4,
                window_margin: 0.5,
            },
            ..Self::default()
        }
    }

    /// Validate that parameters are internally consistent. Called at pipeline entry.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.target_width == 0 || self.target_height == 0 {
            return Err(CoreError::InvalidParams("target dimensions must be non-zero".into()));
        }
        if self.target_artifact_ratio < 0.0 || self.target_artifact_ratio > 1.0 {
            return Err(CoreError::InvalidParams(
                "target_artifact_ratio must be in [0, 1]".into(),
            ));
        }
        if self.sharpen_sigma <= 0.0 {
            return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
        }
        if let SharpenStrategy::ContentAdaptive { backoff_scale_factor, .. } = &self.sharpen_strategy {
            if *backoff_scale_factor <= 0.0 || *backoff_scale_factor >= 1.0 {
                return Err(CoreError::InvalidParams(
                    "backoff_scale_factor must be in (0.0, 1.0)".into(),
                ));
            }
        }
        match &self.probe_strengths {
            ProbeConfig::TwoPass { coarse_count, coarse_min, coarse_max, dense_count, window_margin } => {
                if *coarse_count < 3 {
                    return Err(CoreError::InvalidParams("TwoPass coarse_count must be >= 3".into()));
                }
                if *dense_count < 2 {
                    return Err(CoreError::InvalidParams("TwoPass dense_count must be >= 2".into()));
                }
                if *coarse_min <= 0.0 {
                    return Err(CoreError::InvalidParams("TwoPass coarse_min must be positive".into()));
                }
                if coarse_min >= coarse_max {
                    return Err(CoreError::InvalidParams("TwoPass coarse_min must be less than coarse_max".into()));
                }
                if *window_margin < 0.0 {
                    return Err(CoreError::InvalidParams("TwoPass window_margin must be >= 0".into()));
                }
            }
            _ => { self.probe_strengths.resolve()?; }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Fit quality and robustness
// ---------------------------------------------------------------------------

/// Quality metrics for the polynomial fit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct FitQuality {
    /// Sum of squared residuals between fitted polynomial and data points.
    pub residual_sum_of_squares: f64,
    /// Coefficient of determination (1.0 = perfect fit, 0.0 = no better than mean).
    pub r_squared: f64,
    /// Largest absolute residual among all data points.
    pub max_residual: f64,
    /// Smallest pivot encountered during Gaussian elimination (condition proxy).
    pub min_pivot: f64,
}

/// Robustness assessment of the probe data and fit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct RobustnessFlags {
    /// P(s) is non-decreasing across all probes.
    pub monotonic: bool,
    /// At most one monotonicity violation.
    pub quasi_monotonic: bool,
    /// R^2 exceeds the quality threshold (0.85).
    pub r_squared_ok: bool,
    /// Fit matrix is well-conditioned (min_pivot > threshold).
    pub well_conditioned: bool,
    /// Leave-one-out root is stable (max relative change < threshold).
    pub loo_stable: bool,
    /// Largest relative change in s* across leave-one-out refits.
    pub max_loo_root_change: f64,
}

/// Why the pipeline fell back from polynomial root to sample-based selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// Cubic fit failed numerically (singular matrix, insufficient data).
    FitFailed,
    /// Fit succeeded but leave-one-out check showed instability.
    FitUnstable,
    /// Polynomial root exists but falls outside the probed interval.
    RootOutOfRange,
    /// Probe metric values are non-monotonic — fit may be unreliable.
    MetricNonMonotonic,
    /// Target artifact budget is structurally unreachable (baseline already exceeds P0).
    BudgetTooStrictForContent,
    /// User configured DirectSearch strategy — fit was not attempted.
    DirectSearchConfigured,
    /// Fit succeeded numerically but R² is below the quality threshold (0.85).
    /// The polynomial root may be a false crossing from a poorly-modeled P(s) curve.
    FitPoorQuality,
}

/// Per-stage wall-clock timing in microseconds.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct StageTiming {
    pub resize_us: u64,
    pub contrast_us: u64,
    pub baseline_us: u64,
    /// Wall-clock time for the entire probe loop (parallel or sequential).
    pub probing_us: u64,
    pub fit_us: u64,
    pub robustness_us: u64,
    pub final_sharpen_us: u64,
    pub clamp_us: u64,
    pub total_us: u64,
    /// Region classification time (None when Uniform).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification_us: Option<u64>,
    /// Adaptive validation + backoff time (None when Uniform).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_validation_us: Option<u64>,
    /// Input color-space ingress time (None when not configured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress_us: Option<u64>,
    /// Evaluator execution time (None when not configured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_us: Option<u64>,
    /// Base resize quality scoring time (step 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_quality_us: Option<u64>,
}

// ---------------------------------------------------------------------------
// Composite metric types (v0.2 scaffold)
// ---------------------------------------------------------------------------

/// Individual components of the composite artifact metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum MetricComponent {
    /// Fraction of channel values outside [0, 1] (existing metric_v0).
    GamutExcursion,
    /// Sign-alternating oscillations near strong edges (v0.2).
    HaloRinging,
    /// Sharpening exceeding local edge-strength proxy (v0.2).
    EdgeOvershoot,
    /// Changes in fine-scale local variance in textured regions (v0.2).
    TextureFlattening,
}

/// Per-component breakdown of the composite artifact metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct MetricBreakdown {
    /// Individual component scores.
    pub components: std::collections::BTreeMap<MetricComponent, f32>,

    /// Which metric drove solver selection (GamutExcursion in v0.2).
    pub selected_metric: MetricComponent,
    /// The value of the selected metric.
    pub selection_score: f32,

    /// Weighted composite score (diagnostic only in v0.2 — not used for selection).
    pub composite_score: f32,

    /// Legacy alias for `selection_score`. Kept for backward compatibility.
    #[deprecated(note = "use selection_score")]
    pub aggregate: f32,
}

/// Weights for the composite artifact metric.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct MetricWeights {
    pub gamut_excursion: f32,
    pub halo_ringing: f32,
    pub edge_overshoot: f32,
    pub texture_flattening: f32,
}

impl Default for MetricWeights {
    fn default() -> Self {
        Self {
            gamut_excursion: 1.0,
            halo_ringing: 0.3,
            edge_overshoot: 0.3,
            texture_flattening: 0.1,
        }
    }
}

/// Controls verbosity of serialized diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsLevel {
    /// Final measurement breakdown only (compact JSON).
    #[default]
    Summary,
    /// Per-probe breakdowns included (evaluation mode).
    Full,
}

// ---------------------------------------------------------------------------
// Content-adaptive sharpening types (v0.3)
// ---------------------------------------------------------------------------

/// Number of region classes.
pub const REGION_CLASS_COUNT: usize = 5;

/// Classification of a pixel's local content for adaptive sharpening.
///
/// Stable `as usize` ordering is part of the public contract:
/// Flat=0, Textured=1, StrongEdge=2, Microtexture=3, RiskyHaloZone=4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RegionClass {
    Flat = 0,
    Textured = 1,
    StrongEdge = 2,
    Microtexture = 3,
    RiskyHaloZone = 4,
}

/// Per-pixel region classification map with embedded dimensions.
///
/// Dimensions are part of the type to prevent accidental reuse with
/// a wrong-sized image.
#[derive(Debug, Clone)]
pub struct RegionMap {
    pub width: u32,
    pub height: u32,
    data: Vec<RegionClass>,
}

impl RegionMap {
    /// Create a new region map. Returns error if `data.len() != width * height`.
    pub fn new(width: u32, height: u32, data: Vec<RegionClass>) -> Result<Self, CoreError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Read the class at pixel (x, y).
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> RegionClass {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Read-only access to the underlying data slice.
    pub fn data(&self) -> &[RegionClass] {
        &self.data
    }
}

/// Per-pixel gain multiplier map with embedded dimensions.
#[derive(Debug, Clone)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    data: Vec<f32>,
}

impl GainMap {
    /// Create a new gain map. Returns error if `data.len() != width * height`.
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> Result<Self, CoreError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Read the gain at pixel (x, y).
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Read-only access to the underlying data slice.
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}

/// Per-class gain multipliers for adaptive sharpening.
///
/// **Hard validation bound:** all values must be in `[0.25, 4.0]`.
/// This prevents absurd configuration but does not imply values near the
/// bounds are supported or tested.
///
/// **Recommended operating range:** `[0.5, 1.5]`.
///
/// **Design criterion:** misclassification should degrade gently, not dramatically.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct GainTable {
    pub flat: f32,
    pub textured: f32,
    pub strong_edge: f32,
    pub microtexture: f32,
    pub risky_halo_zone: f32,
}

impl GainTable {
    /// Minimum allowed gain value per region class.
    pub const MIN_GAIN_VALUE: f32 = 0.25;
    const MIN_GAIN: f32 = Self::MIN_GAIN_VALUE;
    const MAX_GAIN: f32 = 4.0;

    /// Construct with validation: all values must be in `[0.25, 4.0]`.
    pub fn new(
        flat: f32,
        textured: f32,
        strong_edge: f32,
        microtexture: f32,
        risky_halo_zone: f32,
    ) -> Result<Self, CoreError> {
        let vals = [flat, textured, strong_edge, microtexture, risky_halo_zone];
        for &v in &vals {
            if !(Self::MIN_GAIN..=Self::MAX_GAIN).contains(&v) {
                return Err(CoreError::InvalidParams(format!(
                    "gain value {v} outside allowed range [{}, {}]",
                    Self::MIN_GAIN,
                    Self::MAX_GAIN,
                )));
            }
        }
        Ok(Self { flat, textured, strong_edge, microtexture, risky_halo_zone })
    }

    /// Canonical v0.3 preset. Range `[0.70, 1.10]`.
    pub fn v03_default() -> Self {
        Self {
            flat: 0.75,
            textured: 0.95,
            strong_edge: 1.00,
            microtexture: 1.10,
            risky_halo_zone: 0.70,
        }
    }

    /// Look up the gain for a given region class.
    #[inline]
    pub fn gain_for(&self, class: RegionClass) -> f32 {
        match class {
            RegionClass::Flat => self.flat,
            RegionClass::Textured => self.textured,
            RegionClass::StrongEdge => self.strong_edge,
            RegionClass::Microtexture => self.microtexture,
            RegionClass::RiskyHaloZone => self.risky_halo_zone,
        }
    }
}

/// Thresholds for the four-pass pixel classifier.
///
/// All thresholds are tied to the specific operators in `classifier.rs`:
/// - Gradient thresholds: **unnormalized Sobel scale** (max ≈ 5.66 for luminance in [0,1]).
/// - Variance thresholds: **squared-luminance units** (max 0.25 for bounded data).
///
/// Changing the Sobel normalization or variance formula invalidates these defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ClassificationParams {
    pub gradient_low_threshold: f32,
    pub gradient_high_threshold: f32,
    pub variance_low_threshold: f32,
    pub variance_high_threshold: f32,
    pub variance_window: usize,
}

impl ClassificationParams {
    /// Construct with validation.
    pub fn new(
        gradient_low_threshold: f32,
        gradient_high_threshold: f32,
        variance_low_threshold: f32,
        variance_high_threshold: f32,
        variance_window: usize,
    ) -> Result<Self, CoreError> {
        if gradient_low_threshold > gradient_high_threshold {
            return Err(CoreError::InvalidParams(
                "gradient_low_threshold must be <= gradient_high_threshold".into(),
            ));
        }
        if variance_low_threshold > variance_high_threshold {
            return Err(CoreError::InvalidParams(
                "variance_low_threshold must be <= variance_high_threshold".into(),
            ));
        }
        if variance_window < 3 {
            return Err(CoreError::InvalidParams(
                "variance_window must be >= 3".into(),
            ));
        }
        if variance_window.is_multiple_of(2) {
            return Err(CoreError::InvalidParams(
                "variance_window must be odd".into(),
            ));
        }
        Ok(Self {
            gradient_low_threshold,
            gradient_high_threshold,
            variance_low_threshold,
            variance_high_threshold,
            variance_window,
        })
    }
}

impl Default for ClassificationParams {
    fn default() -> Self {
        Self {
            gradient_low_threshold: 0.05,
            gradient_high_threshold: 0.40,
            variance_low_threshold: 0.001,
            variance_high_threshold: 0.010,
            variance_window: 5,
        }
    }
}

/// Orchestration axis for sharpening strength distribution.
///
/// Orthogonal to [`SharpenMode`] (Rgb/Lightness).
/// `SharpenStrategy` controls whether strength is applied uniformly or modulated
/// per-pixel by a region-based gain map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case", tag = "strategy")]
pub enum SharpenStrategy {
    /// Current behaviour: single global strength applied everywhere.
    #[default]
    Uniform,
    /// Per-pixel gain modulated by region classification.
    ContentAdaptive {
        classification: ClassificationParams,
        gain_table: GainTable,
        /// Maximum backoff iterations if adaptive result exceeds budget. Default: 4.
        max_backoff_iterations: u8,
        /// Scale reduction per backoff iteration. Must be in (0.0, 1.0). Default: 0.8.
        backoff_scale_factor: f32,
    },
}

/// Per-class pixel coverage computed from a [`RegionMap`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct RegionCoverage {
    pub total_pixels: u32,
    pub flat: u32,
    pub textured: u32,
    pub strong_edge: u32,
    pub microtexture: u32,
    pub risky_halo_zone: u32,
    pub flat_fraction: f32,
    pub textured_fraction: f32,
    pub strong_edge_fraction: f32,
    pub microtexture_fraction: f32,
    pub risky_halo_zone_fraction: f32,
}

impl RegionCoverage {
    /// Compute coverage statistics from a region map.
    pub fn from_region_map(map: &RegionMap) -> Self {
        let mut counts = [0u32; REGION_CLASS_COUNT];
        for &c in map.data() {
            counts[c as usize] += 1;
        }
        let total = map.width * map.height;
        let frac = |c: u32| if total > 0 { c as f32 / total as f32 } else { 0.0 };
        Self {
            total_pixels: total,
            flat: counts[0],
            textured: counts[1],
            strong_edge: counts[2],
            microtexture: counts[3],
            risky_halo_zone: counts[4],
            flat_fraction: frac(counts[0]),
            textured_fraction: frac(counts[1]),
            strong_edge_fraction: frac(counts[2]),
            microtexture_fraction: frac(counts[3]),
            risky_halo_zone_fraction: frac(counts[4]),
        }
    }
}

/// Outcome of the adaptive validation / backoff phase.
///
/// `target_metric` is not duplicated here — it lives in [`AutoSharpParams::target_artifact_ratio`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum AdaptiveValidationOutcome {
    /// Adaptive result met budget on first try.
    PassedDirect { measured_metric: f32 },
    /// Budget met after scaling down global strength.
    PassedAfterBackoff {
        iterations: u8,
        final_scale: f32,
        measured_metric: f32,
    },
    /// Budget not met after all backoff iterations; best result returned.
    FailedBudgetExceeded {
        iterations: u8,
        best_scale: f32,
        best_metric: f32,
    },
}

// ---------------------------------------------------------------------------
// Probe and fit result types
// ---------------------------------------------------------------------------

/// A single measured sample of the artifact-vs-strength relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ProbeSample {
    /// Sharpening strength `s`.
    pub strength: f32,
    /// P_total(s): fraction of channel components outside [0, 1] after sharpening.
    pub artifact_ratio: f32,
    /// The metric value used for fitting and selection, depending on `MetricMode`:
    /// - `AbsoluteTotal`: same as `artifact_ratio`
    /// - `RelativeToBase`: `max(0, artifact_ratio - baseline)`
    pub metric_value: f32,
    /// Per-component breakdown of the artifact metric (v0.2 scaffold).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<MetricBreakdown>,
}

/// Cubic polynomial in f64 arithmetic (for numerical stability).
///
/// `P_hat(s) = a*s^3 + b*s^2 + c*s + d`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct CubicPolynomial {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl CubicPolynomial {
    pub fn evaluate(&self, s: f64) -> f64 {
        self.a * s.powi(3) + self.b * s.powi(2) + self.c * s + self.d
    }
}

// ---------------------------------------------------------------------------
// Pipeline output
// ---------------------------------------------------------------------------

/// Diagnostics emitted by the pipeline; serializable for CLI JSON output and GUI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct AutoSharpDiagnostics {
    // --- Size ---
    pub input_size: ImageSize,
    pub output_size: ImageSize,

    // --- Configuration ---
    pub sharpen_mode: SharpenMode,
    pub metric_mode: MetricMode,
    pub artifact_metric: ArtifactMetric,
    pub target_artifact_ratio: f32,

    // --- Baseline (resize-stage artifact contribution) ---
    /// Artifact ratio of the downscaled image before any sharpening is applied.
    pub baseline_artifact_ratio: f32,

    // --- Probe data ---
    pub probe_samples: Vec<ProbeSample>,

    // --- Fit / solve results ---
    pub fit_status: FitStatus,
    pub fit_coefficients: Option<CubicPolynomial>,
    /// Quality metrics for the polynomial fit (residuals, R², condition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_quality: Option<FitQuality>,
    pub crossing_status: CrossingStatus,

    // --- Robustness assessment ---
    /// Robustness flags computed from probe data and fit quality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robustness: Option<RobustnessFlags>,

    // --- Selection result ---
    /// Sharpening strength that was applied to produce the final image.
    pub selected_strength: f32,
    pub selection_mode: SelectionMode,
    /// Why the polynomial root was not used (None when selection_mode == PolynomialRoot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<FallbackReason>,
    /// Whether the target artifact budget is achievable given the baseline and probe range.
    pub budget_reachable: bool,

    // --- Final measurement (pre-clamp) ---
    /// P_total(s*) on the final sharpened image, before clamping.
    pub measured_artifact_ratio: f32,
    /// Metric value of the final output (relative or absolute depending on mode).
    pub measured_metric_value: f32,
    /// Per-component breakdown of the final artifact metric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_components: Option<MetricBreakdown>,

    /// Weights used for composite score computation.
    pub metric_weights: MetricWeights,

    // --- Content-adaptive (v0.3) ---
    /// Per-class region coverage. None when `SharpenStrategy::Uniform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_coverage: Option<RegionCoverage>,
    /// Outcome of adaptive validation. None when `SharpenStrategy::Uniform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_validation: Option<AdaptiveValidationOutcome>,

    // --- Timing ---
    /// Per-stage wall-clock timing.
    #[serde(default)]
    pub timing: StageTiming,

    // --- Experimental (v0.4) ---

    /// Input color-space ingress diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_ingress: Option<InputIngressDiagnostics>,

    /// Resize strategy diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize_strategy_diagnostics: Option<ResizeStrategyDiagnostics>,

    /// Chroma guard diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chroma_guard: Option<ChromaGuardDiagnostics>,

    /// Quality evaluator result (advisory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_result: Option<QualityEvaluation>,

    /// Actionable recommendations derived from pipeline diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<Recommendation>,

    // --- Two-pass probe placement (step 3) ---
    /// Coarse/dense pass diagnostics. Present only when `ProbeConfig::TwoPass` was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_pass_diagnostics: Option<ProbePassDiagnostics>,

    // --- Base resize quality (step 4) ---
    /// Quality assessment of the base resized image before sharpening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_resize_quality: Option<BaseResizeQuality>,
    /// Effective target artifact ratio after applying the ringing-aware envelope.
    /// `effective = target_artifact_ratio × base_resize_quality.envelope_scale`.
    /// Equals `target_artifact_ratio` when `base_resize_quality` is `None`.
    #[serde(default)]
    pub effective_target_artifact_ratio: f32,
}

/// Return type of the top-level pipeline function.
pub struct ProcessOutput {
    /// Final processed image (clamped according to `ClampPolicy`).
    pub image: LinearRgbImage,
    pub diagnostics: AutoSharpDiagnostics,
}

// ---------------------------------------------------------------------------
// Experimental types (v0.4)
// ---------------------------------------------------------------------------

// --- Branch C: RAW-friendly ingress ---

/// Input color space declaration for the pipeline entry point.
///
/// Tells the pipeline how to interpret the pixel data it receives.
/// When `None` (the default), the pipeline assumes data has already been
/// linearized by the IO layer (`InputColorSpace::Srgb` semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum InputColorSpace {
    /// Standard sRGB input (default). The IO layer has already applied
    /// sRGB→linear; the pipeline treats data as linear RGB in [0, 1].
    #[default]
    Srgb,
    /// Data is already in linear RGB [0, 1]. Validates range, emits
    /// a diagnostic warning if values exceed [0, 1].
    LinearRgb,
    /// Data is in linear RGB but may exceed [0, 1] (HDR / RAW output).
    /// The pipeline normalizes to [0, 1] and records the scale factor.
    RawLinear,
}

/// Diagnostics from the input color-space ingress stage.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct InputIngressDiagnostics {
    /// Which color space was declared by the caller.
    pub declared_color_space: InputColorSpace,
    /// (min, max) of raw channel values. Present for `RawLinear`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value_max: Option<f32>,
    /// Scale factor applied to bring values into [0, 1]. Present for `RawLinear`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization_scale: Option<f32>,
    /// Fraction of values > 1.0. Present for `LinearRgb` validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_of_range_fraction: Option<f32>,
}

// --- Branch B: Region-adaptive resize kernels ---

/// Available resize kernels for downscaling.
///
/// Maps to `image::imageops::FilterType` variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum ResizeKernel {
    Lanczos3,
    MitchellNetravali,
    CatmullRom,
    Gaussian,
}

/// Per-class kernel assignment for content-adaptive resizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct KernelTable {
    pub flat: ResizeKernel,
    pub textured: ResizeKernel,
    pub strong_edge: ResizeKernel,
    pub microtexture: ResizeKernel,
    pub risky_halo_zone: ResizeKernel,
}

impl Default for KernelTable {
    fn default() -> Self {
        Self {
            flat: ResizeKernel::Gaussian,
            textured: ResizeKernel::Lanczos3,
            strong_edge: ResizeKernel::Lanczos3,
            microtexture: ResizeKernel::CatmullRom,
            risky_halo_zone: ResizeKernel::MitchellNetravali,
        }
    }
}

impl KernelTable {
    /// Look up the kernel for a given region class.
    #[inline]
    pub fn kernel_for(&self, class: RegionClass) -> ResizeKernel {
        match class {
            RegionClass::Flat => self.flat,
            RegionClass::Textured => self.textured,
            RegionClass::StrongEdge => self.strong_edge,
            RegionClass::Microtexture => self.microtexture,
            RegionClass::RiskyHaloZone => self.risky_halo_zone,
        }
    }
}

/// How to select the resize kernel for downscaling.
///
/// Orthogonal to [`SharpenStrategy`] — controls the resize stage, not sharpening.
/// When the pipeline receives `None`, it falls back to the existing Lanczos3 path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case", tag = "strategy")]
pub enum ResizeStrategy {
    /// Use a single kernel for the entire image.
    Uniform { kernel: ResizeKernel },
    /// Classify the **source** image and pick a kernel per region, then blend.
    ContentAdaptive {
        classification: ClassificationParams,
        kernel_table: KernelTable,
    },
}

/// Diagnostics from the resize strategy stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ResizeStrategyDiagnostics {
    /// Which distinct kernels were actually used.
    pub kernels_used: Vec<ResizeKernel>,
    /// Per-kernel pixel count in the output image.
    pub per_kernel_pixel_count: std::collections::BTreeMap<String, u32>,
}

// --- Branch D: Alternative color handling ---

/// Extended sharpening mode with chroma monitoring.
///
/// Supplements the existing [`SharpenMode`] axis. When set, the pipeline
/// uses the extended sharpening path instead of the standard one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum ExperimentalSharpenMode {
    /// Sharpen luminance, then monitor per-pixel chroma shift and apply
    /// soft clamping when the shift exceeds the threshold.
    ///
    /// When `chroma_region_factors` and a region map are both available,
    /// the threshold is modulated per-pixel by region class.  When
    /// `saturation_guard` is set, already-saturated pixels receive a
    /// tighter threshold.
    LumaPlusChromaGuard {
        /// Maximum allowed chroma shift as a fraction of original chroma magnitude.
        /// Values above this trigger soft clamping. Default: 0.10 (10%).
        max_chroma_shift: f32,
        /// Per-region-class multipliers for `max_chroma_shift`.
        /// Only effective when the pipeline also produces a region map
        /// (i.e. `SharpenStrategy::ContentAdaptive`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chroma_region_factors: Option<ChromaRegionFactors>,
        /// Saturation-dependent threshold tightening.
        /// Active regardless of region map availability.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        saturation_guard: Option<SaturationGuardParams>,
    },
}

/// Per-region-class multipliers that scale the chroma guard threshold.
///
/// Lower values = tighter guard (more chroma protection).
/// Semantics mirror [`GainTable`]: one field per [`RegionClass`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ChromaRegionFactors {
    pub flat: f32,
    pub textured: f32,
    pub strong_edge: f32,
    pub microtexture: f32,
    pub risky_halo_zone: f32,
}

impl ChromaRegionFactors {
    /// Look up the factor for a given region class.
    #[inline]
    pub fn factor_for(&self, class: RegionClass) -> f32 {
        match class {
            RegionClass::Flat => self.flat,
            RegionClass::Textured => self.textured,
            RegionClass::StrongEdge => self.strong_edge,
            RegionClass::Microtexture => self.microtexture,
            RegionClass::RiskyHaloZone => self.risky_halo_zone,
        }
    }
}

impl Default for ChromaRegionFactors {
    fn default() -> Self {
        Self {
            flat: 1.00,
            textured: 0.90,
            strong_edge: 0.65,
            microtexture: 0.80,
            risky_halo_zone: 0.45,
        }
    }
}

/// Saturation-aware chroma guard parameters.
///
/// Tightens the chroma shift threshold for already-saturated pixels.
/// `effective_scale = 1.0 − (1.0 − min_scale) × saturation^gamma`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct SaturationGuardParams {
    /// Minimum scale factor applied to fully-saturated pixels. Default: 0.6.
    pub min_scale: f32,
    /// Gamma exponent controlling the saturation→scale curve. Default: 1.5.
    pub gamma: f32,
}

impl Default for SaturationGuardParams {
    fn default() -> Self {
        Self { min_scale: 0.6, gamma: 1.5 }
    }
}

/// Color space used for artifact evaluation during probing and final measurement.
///
/// Orthogonal to [`ArtifactMetric`] — controls _which_ color representation
/// the metric operates on, not _which_ metric function is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum EvaluationColorSpace {
    /// Evaluate artifacts in linear RGB (current behavior).
    #[default]
    Rgb,
    /// Evaluate artifacts on the luminance channel only.
    LumaOnly,
    /// Evaluate artifacts in an approximate CIE Lab space.
    LabApprox,
}

/// Diagnostics from the chroma guard sharpening path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ChromaGuardDiagnostics {
    /// Fraction of pixels where chroma soft-clamping was applied.
    pub pixels_clamped_fraction: f32,
    /// Mean chroma shift magnitude across all pixels.
    pub mean_chroma_shift: f32,
    /// Maximum chroma shift magnitude.
    pub max_chroma_shift: f32,

    // --- Context-aware guard diagnostics (step 5) ---

    /// Minimum effective threshold across all pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_threshold_min: Option<f32>,
    /// Mean effective threshold across all pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_threshold_mean: Option<f32>,
    /// Maximum effective threshold across all pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_threshold_max: Option<f32>,

    /// Per-region-class clamp statistics.
    /// Present only when a region map was available (ContentAdaptive strategy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_region: Option<ChromaPerRegionDiagnostics>,
}

/// Chroma guard clamp statistics for a single region class.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ChromaRegionClampStats {
    pub pixel_count: u32,
    pub clamped_count: u32,
    pub clamped_fraction: f32,
    pub mean_shift: f32,
    pub max_shift: f32,
}

/// Per-region breakdown of chroma guard behavior.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ChromaPerRegionDiagnostics {
    pub flat: ChromaRegionClampStats,
    pub textured: ChromaRegionClampStats,
    pub strong_edge: ChromaRegionClampStats,
    pub microtexture: ChromaRegionClampStats,
    pub risky_halo_zone: ChromaRegionClampStats,
}

// --- Branch A: Learned evaluator ---

/// Configuration for the quality evaluator.
///
/// The evaluator runs after final sharpening and produces advisory diagnostics.
/// It does **not** alter the pipeline's s* selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorConfig {
    /// Hand-crafted feature extraction + linear quality model.
    Heuristic,
}

/// Features extracted from an image for quality prediction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ImageFeatures {
    /// Fraction of pixels classified as edges (Sobel magnitude > threshold).
    pub edge_density: f32,
    /// Mean Sobel gradient magnitude across all pixels.
    pub mean_gradient_magnitude: f32,
    /// Variance of gradient magnitudes.
    pub gradient_variance: f32,
    /// Mean local variance (5×5 window) across all pixels.
    pub mean_local_variance: f32,
    /// Variance of local variances (texture heterogeneity).
    pub local_variance_variance: f32,
    /// Variance of the Laplacian response (frequency content proxy).
    pub laplacian_variance: f32,
    /// Shannon entropy of the 64-bin luminance histogram.
    pub luminance_histogram_entropy: f32,
}

/// Quality evaluation result from a [`QualityEvaluator`](crate::evaluator::QualityEvaluator).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct QualityEvaluation {
    /// Predicted overall quality score in [0, 1] (higher = better).
    pub predicted_quality_score: f32,
    /// Optional suggested sharpening strength (advisory, not enforced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_strength: Option<f32>,
    /// Confidence in the prediction, in [0, 1].
    pub confidence: f32,
    /// Raw feature vector used for the prediction.
    pub features: ImageFeatures,
}

// ---------------------------------------------------------------------------
// Recommendations (v0.5)
// ---------------------------------------------------------------------------

/// What kind of change the recommendation suggests.
///
/// Used for UI labeling and advice deduplication.  The real action is in
/// [`Recommendation::patch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum RecommendationKind {
    SwitchToContentAdaptive,
    LowerStrongEdgeGain,
    RaiseArtifactBudget,
    SwitchToLightness,
    WidenProbeRange,
    LowerSigma,
}

/// Display severity for a recommendation.  Affects UI styling only — does not
/// change which patch gets applied or how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Suggestion,
    Warning,
}

/// Self-contained partial update to [`AutoSharpParams`].
///
/// Every present field is a full replacement value — no deep-merge logic
/// required.  For nested types like [`SharpenStrategy::ContentAdaptive`], the
/// patch carries the entire variant, not a nested diff.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct ParamPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_strategy: Option<SharpenStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_artifact_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_mode: Option<SharpenMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_strengths: Option<ProbeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_sigma: Option<f32>,
}

/// An actionable recommendation derived from pipeline diagnostics.
///
/// Each recommendation maps a diagnostic observation to a concrete
/// [`ParamPatch`].  "Apply" in the UI means: merge the patch into
/// `AutoSharpParams`, rerun the solver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct Recommendation {
    pub kind: RecommendationKind,
    pub severity: Severity,
    /// Confidence in \[0, 1\].  Display-only — does not affect patch content.
    pub confidence: f32,
    /// Human-readable explanation of why this recommendation was generated.
    pub reason: String,
    /// Self-contained param patch.  Apply via `updateParams(patch)`.
    pub patch: ParamPatch,
}

// ---------------------------------------------------------------------------
// Tests — content-adaptive types
// ---------------------------------------------------------------------------

#[cfg(test)]
mod adaptive_tests {
    use super::*;

    #[test]
    fn region_class_as_usize_stable_ordering() {
        assert_eq!(RegionClass::Flat as usize, 0);
        assert_eq!(RegionClass::Textured as usize, 1);
        assert_eq!(RegionClass::StrongEdge as usize, 2);
        assert_eq!(RegionClass::Microtexture as usize, 3);
        assert_eq!(RegionClass::RiskyHaloZone as usize, 4);
    }

    #[test]
    fn region_map_valid_construction() {
        let data = vec![RegionClass::Flat; 12];
        let map = RegionMap::new(4, 3, data).unwrap();
        assert_eq!(map.width, 4);
        assert_eq!(map.height, 3);
        assert_eq!(map.get(0, 0), RegionClass::Flat);
        assert_eq!(map.get(3, 2), RegionClass::Flat);
    }

    #[test]
    fn region_map_wrong_length_fails() {
        let data = vec![RegionClass::Flat; 10];
        assert!(RegionMap::new(4, 3, data).is_err());
    }

    #[test]
    fn gain_map_valid_construction() {
        let data = vec![1.0f32; 6];
        let map = GainMap::new(3, 2, data).unwrap();
        assert_eq!(map.width, 3);
        assert_eq!(map.height, 2);
        assert!((map.get(0, 0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn gain_map_wrong_length_fails() {
        let data = vec![1.0f32; 5];
        assert!(GainMap::new(3, 2, data).is_err());
    }

    #[test]
    fn gain_table_v03_default_values() {
        let gt = GainTable::v03_default();
        assert!((gt.flat - 0.75).abs() < 1e-6);
        assert!((gt.textured - 0.95).abs() < 1e-6);
        assert!((gt.strong_edge - 1.00).abs() < 1e-6);
        assert!((gt.microtexture - 1.10).abs() < 1e-6);
        assert!((gt.risky_halo_zone - 0.70).abs() < 1e-6);
    }

    #[test]
    fn gain_table_gain_for_each_class() {
        let gt = GainTable::v03_default();
        assert!((gt.gain_for(RegionClass::Flat) - 0.75).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::Textured) - 0.95).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::StrongEdge) - 1.00).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::Microtexture) - 1.10).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::RiskyHaloZone) - 0.70).abs() < 1e-6);
    }

    #[test]
    fn gain_table_out_of_bounds_rejected() {
        assert!(GainTable::new(0.2, 1.0, 1.0, 1.0, 1.0).is_err());
        assert!(GainTable::new(1.0, 5.0, 1.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn gain_table_at_bounds_accepted() {
        assert!(GainTable::new(0.25, 4.0, 1.0, 1.0, 1.0).is_ok());
    }

    #[test]
    fn classification_params_default_valid() {
        let cp = ClassificationParams::default();
        assert!(cp.gradient_low_threshold <= cp.gradient_high_threshold);
        assert!(cp.variance_low_threshold <= cp.variance_high_threshold);
        assert!(cp.variance_window >= 3);
        assert!(cp.variance_window % 2 == 1);
    }

    #[test]
    fn classification_params_inverted_gradient_rejected() {
        let result = ClassificationParams::new(0.5, 0.1, 0.001, 0.01, 5);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_inverted_variance_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.1, 0.01, 5);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_even_window_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.001, 0.01, 4);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_window_too_small_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.001, 0.01, 1);
        assert!(result.is_err());
    }

    #[test]
    fn sharpen_strategy_default_is_uniform() {
        assert!(matches!(SharpenStrategy::default(), SharpenStrategy::Uniform));
    }

    #[test]
    fn sharpen_strategy_content_adaptive_construction() {
        let strategy = SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        };
        assert!(matches!(strategy, SharpenStrategy::ContentAdaptive { .. }));
    }

    #[test]
    fn region_coverage_invariant() {
        let rc = RegionCoverage::from_region_map(&RegionMap::new(
            2, 2,
            vec![
                RegionClass::Flat,
                RegionClass::Textured,
                RegionClass::StrongEdge,
                RegionClass::Flat,
            ],
        ).unwrap());
        assert_eq!(rc.total_pixels, 4);
        assert_eq!(rc.flat + rc.textured + rc.strong_edge + rc.microtexture + rc.risky_halo_zone, 4);
        assert_eq!(rc.flat, 2);
        assert_eq!(rc.textured, 1);
        assert_eq!(rc.strong_edge, 1);
    }
}
