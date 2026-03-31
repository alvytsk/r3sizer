//! r3sizer-core — pure image processing library
//!
//! Pipeline summary:
//!   1. sRGB → linear RGB  (`color`)
//!   2. Downscale in linear space  (`resize`)
//!   3. Optional contrast leveling  (`contrast`)
//!   4. Probe multiple sharpening strengths  (`sharpen`, `metrics`)
//!   5. Fit cubic P(s) approximation  (`fit`)
//!   6. Solve P_hat(s*) = P0  (`solve`)
//!   7. Final sharpen, clamp, return  (`pipeline`)
//!
//! All intermediate calculations use `f32` pixel buffers.
//! Polynomial fitting uses `f64` for numerical stability.

pub mod classifier;
pub mod color;
pub mod contrast;
pub mod fit;
pub mod metrics;
pub mod pipeline;
pub mod resize;
pub mod sharpen;
pub mod solve;
pub mod types;

pub mod base_quality;
pub mod color_space;
pub mod resize_strategy;
pub mod chroma_guard;
pub mod evaluator;
pub mod recommendations;

// Re-export the complete public surface.
pub use pipeline::{
    PreparedBase, prepare_base, process_from_prepared, process_from_prepared_with_probes,
    resolve_initial_strengths, resolve_dense_strengths,
    run_probes_standalone, run_probes_from_detail, compute_probe_detail,
    process_auto_sharp_downscale, process_auto_sharp_downscale_with_progress,
};
pub use types::{
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, BaseResizeQuality,
    ChromaGuardDiagnostics, ChromaPerRegionDiagnostics, ChromaRegionClampStats,
    ChromaRegionFactors, ClampPolicy, ClassificationParams, CrossingStatus, CubicPolynomial,
    DiagnosticsLevel, EvaluationColorSpace, EvaluatorConfig, ExperimentalSharpenMode,
    FallbackReason, FitQuality, FitStatus, FitStrategy, GainMap, GainTable, ImageFeatures,
    ImageSize, InputColorSpace, InputIngressDiagnostics, KernelTable, LinearRgbImage,
    MetricBreakdown, MetricComponent, MetricMode, MetricWeights, PipelineMode, ProbePassDiagnostics,
    ProbeSample, ProbeConfig, ProcessOutput, QualityEvaluation, RegionClass, RegionCoverage,
    RegionMap, Recommendation, RecommendationKind, Severity, ParamPatch,
    ResizeKernel, ResizeStrategy, ResizeStrategyDiagnostics, RobustnessFlags, SelectionMode,
    SelectionPolicy, SaturationGuardParams, SharpenMode, SharpenStrategy, StageTiming,
    REGION_CLASS_COUNT,
};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("polynomial fitting failed: {0}")]
    FitFailed(String),

    #[error("no valid sharpening root found: {reason}")]
    NoValidRoot { reason: String },

    #[error("buffer length mismatch: expected {expected_len} components, got {got_len}")]
    BufferLengthMismatch {
        expected_len: usize,
        got_len: usize,
    },

    #[error("empty image: width or height is zero")]
    EmptyImage,
}
