//! Stable public API prelude for `r3sizer-core`.
//!
//! Import everything you need for the common embedding path:
//!
//! ```rust,ignore
//! use r3sizer_core::prelude::*;
//! ```
//!
//! ## Stability tiers
//!
//! | Tier | Modules | Guarantee |
//! |------|---------|-----------|
//! | **Stable** | `prelude`, `pipeline`, `color`, `chroma_guard` (public API) | No breaking changes within a major version |
//! | **Experimental** | `evaluator`, `base_quality`, `contrast`, `recommendations` | May change in any minor version; gated by feature flags in a future release |
//! | **Internal** | `fit`, `solve`, `metrics` internals | Accessible for advanced use but no SemVer guarantee |
//!
//! Items re-exported here are **stable**.  Reach into sub-modules directly only
//! when you need experimental or advanced features and can tolerate churn.

// ── Primary image type ────────────────────────────────────────────────────────
pub use crate::LinearRgbImage;

// ── Configuration ─────────────────────────────────────────────────────────────
pub use crate::{
    ArtifactMetric, AutoSharpParams, ClampPolicy, DiagnosticsLevel, FitStrategy, MetricMode,
    MetricWeights, PipelineMode, ProbeConfig, SelectionPolicy, SharpenMode, SharpenStrategy,
};

// ── Two-phase pipeline entry points ──────────────────────────────────────────
pub use crate::{prepare_base, process_auto_sharp_downscale, process_from_prepared, PreparedBase};

// ── Output and diagnostics ────────────────────────────────────────────────────
pub use crate::{AutoSharpDiagnostics, ProcessOutput, StageTiming};

// ── Error type ────────────────────────────────────────────────────────────────
pub use crate::CoreError;
