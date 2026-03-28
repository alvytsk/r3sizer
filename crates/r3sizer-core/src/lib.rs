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
pub mod paper_sharpen;
pub mod pipeline;
pub mod resize;
pub mod sharpen;
pub mod solve;
pub mod types;

// Re-export the complete public surface.
pub use pipeline::process_auto_sharp_downscale;
pub use types::{
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams,
    ClampPolicy, ClassificationParams, CrossingStatus, CubicPolynomial, DiagnosticsLevel,
    FallbackReason, FitQuality, FitStatus, FitStrategy, GainMap, GainTable, ImageSize,
    LinearRgbImage, MetricBreakdown, MetricComponent, MetricMode, MetricWeights, ProbeSample,
    ProbeConfig, ProcessOutput, Provenance, RegionClass, RegionCoverage, RegionMap,
    RobustnessFlags, SelectionMode, SharpenMode, SharpenModel, SharpenStrategy, StageTiming,
    StageProvenance, REGION_CLASS_COUNT,
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
