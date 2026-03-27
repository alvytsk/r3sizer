use serde::{Deserialize, Serialize};

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
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Sharpening and metric configuration
// ---------------------------------------------------------------------------

/// How sharpening is applied to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

// ---------------------------------------------------------------------------
// Solver / diagnostics status enums
// ---------------------------------------------------------------------------

/// Status of the polynomial fit attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub enum ProbeConfig {
    /// `count` values linearly spaced over `[min, max]`.
    Range { min: f32, max: f32, count: usize },
    /// Caller-supplied explicit list (must have >= 4 distinct, positive values).
    Explicit(Vec<f32>),
}

impl ProbeConfig {
    /// Resolve to a sorted `Vec<f32>`.
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
        };
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(values)
    }
}

/// Polynomial fit strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitStrategy {
    /// Least-squares cubic fit; fall back to direct sampled search if numerically unstable.
    Cubic,
    /// Skip fitting; pick best strength directly from probe samples.
    DirectSearch,
}

/// How to handle out-of-range values at the final output stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClampPolicy {
    /// Hard clamp: values < 0.0 -> 0.0, values > 1.0 -> 1.0.
    Clamp,
    /// Rescale entire image by its global maximum.
    Normalize,
}

/// All parameters controlling the auto-sharpness downscale pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for AutoSharpParams {
    fn default() -> Self {
        Self {
            target_width: 800,
            target_height: 600,
            probe_strengths: ProbeConfig::Explicit(
                vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0],
            ),
            target_artifact_ratio: 0.001,
            enable_contrast_leveling: false,
            sharpen_sigma: 1.0,
            fit_strategy: FitStrategy::Cubic,
            output_clamp: ClampPolicy::Clamp,
            sharpen_mode: SharpenMode::Lightness,
            metric_mode: MetricMode::RelativeToBase,
        }
    }
}

impl AutoSharpParams {
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
        self.probe_strengths.resolve()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Probe and fit result types
// ---------------------------------------------------------------------------

/// A single measured sample of the artifact-vs-strength relationship.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProbeSample {
    /// Sharpening strength `s`.
    pub strength: f32,
    /// P_total(s): fraction of channel components outside [0, 1] after sharpening.
    pub artifact_ratio: f32,
    /// The metric value used for fitting and selection, depending on `MetricMode`:
    /// - `AbsoluteTotal`: same as `artifact_ratio`
    /// - `RelativeToBase`: `max(0, artifact_ratio - baseline)`
    pub metric_value: f32,
}

/// Cubic polynomial in f64 arithmetic (for numerical stability).
///
/// `P_hat(s) = a*s^3 + b*s^2 + c*s + d`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
pub struct AutoSharpDiagnostics {
    // --- Size ---
    pub input_size: ImageSize,
    pub output_size: ImageSize,

    // --- Configuration ---
    pub sharpen_mode: SharpenMode,
    pub metric_mode: MetricMode,
    pub target_artifact_ratio: f32,

    // --- Baseline (resize-stage artifact contribution) ---
    /// Artifact ratio of the downscaled image before any sharpening is applied.
    pub baseline_artifact_ratio: f32,

    // --- Probe data ---
    pub probe_samples: Vec<ProbeSample>,

    // --- Fit / solve results ---
    pub fit_status: FitStatus,
    pub fit_coefficients: Option<CubicPolynomial>,
    pub crossing_status: CrossingStatus,

    // --- Selection result ---
    /// Sharpening strength that was applied to produce the final image.
    pub selected_strength: f32,
    pub selection_mode: SelectionMode,
    /// Whether the target artifact budget is achievable given the baseline and probe range.
    pub budget_reachable: bool,

    // --- Final measurement (pre-clamp) ---
    /// P_total(s*) on the final sharpened image, before clamping.
    pub measured_artifact_ratio: f32,
    /// Metric value of the final output (relative or absolute depending on mode).
    pub measured_metric_value: f32,
}

/// Return type of the top-level pipeline function.
pub struct ProcessOutput {
    /// Final processed image (clamped according to `ClampPolicy`).
    pub image: LinearRgbImage,
    pub diagnostics: AutoSharpDiagnostics,
}
