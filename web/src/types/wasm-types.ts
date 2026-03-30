// Re-export all auto-generated types and defaults from Rust.
// Only manually-defined types and constants belong in this file.
//
// To regenerate generated.ts:
//   cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture

import type {
  AutoSharpParams,
  AutoSharpDiagnostics,
  SharpenStrategy,
  ResizeStrategy,
} from "./generated";

import {
  DEFAULT_PARAMS as _GENERATED_DEFAULT_PARAMS,
  DEFAULT_CLASSIFICATION_PARAMS,
  DEFAULT_GAIN_TABLE,
  DEFAULT_KERNEL_TABLE,
} from "./generated";

// ── Re-export all generated types ───────────────────────────────────────

export type {
  SharpenMode,
  MetricMode,
  ArtifactMetric,
  FitStrategy,
  ClampPolicy,
  DiagnosticsLevel,
  CrossingStatus,
  SelectionMode,
  SelectionPolicy,
  FallbackReason,
  MetricComponent,
  RegionClass,
  ImageSize,
  MetricWeights,
  GainTable,
  ClassificationParams,
  ProbeConfig,
  SharpenStrategy,
  AutoSharpParams,
  CubicPolynomial,
  FitQuality,
  RobustnessFlags,
  FitStatus,
  MetricBreakdown,
  ProbeSample,
  StageTiming,
  RegionCoverage,
  AdaptiveValidationOutcome,
  AutoSharpDiagnostics,
  // Experimental types
  InputColorSpace,
  ResizeKernel,
  KernelTable,
  ResizeStrategy,
  ResizeStrategyDiagnostics,
  ExperimentalSharpenMode,
  EvaluationColorSpace,
  ChromaGuardDiagnostics,
  EvaluatorConfig,
  ImageFeatures,
  QualityEvaluation,
  InputIngressDiagnostics,
  // Recommendations (v0.5)
  RecommendationKind,
  Severity,
  ParamPatch,
  Recommendation,
} from "./generated";

export {
  DEFAULT_METRIC_WEIGHTS,
  DEFAULT_GAIN_TABLE,
  DEFAULT_CLASSIFICATION_PARAMS,
  DEFAULT_SHARPEN_STRATEGY,
  DEFAULT_KERNEL_TABLE,
} from "./generated";

// ── Web-specific defaults ───────────────────────────────────────────────

// Override: web UI needs "full" diagnostics for the diagnostics panel.
// Rust defaults to "summary" which strips per-probe breakdowns.
export const DEFAULT_PARAMS: AutoSharpParams = {
  ..._GENERATED_DEFAULT_PARAMS,
  diagnostics_level: "full",
};

// ── Types not in Rust (WASM boundary or TS-only helpers) ────────────────

/** Extract the content-adaptive variant from the SharpenStrategy union. */
export type ContentAdaptiveStrategy = Extract<
  SharpenStrategy,
  { strategy: "content_adaptive" }
>;

/** Extract the content-adaptive variant from the ResizeStrategy union. */
export type ContentAdaptiveResizeStrategy = Extract<
  ResizeStrategy,
  { strategy: "content_adaptive" }
>;

/** Return type from the WASM process_image() call via the Web Worker. */
export interface ProcessResult {
  imageData: Uint8Array;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics;
}

// ── Pipeline presets for benchmarking ───────────────────────────────────

// Shared building blocks for the stable presets.
const _CA_STRATEGY = {
  strategy: "content_adaptive" as const,
  classification: { ...DEFAULT_CLASSIFICATION_PARAMS },
  gain_table: { ...DEFAULT_GAIN_TABLE },
  max_backoff_iterations: 4,
  backoff_scale_factor: 0.8,
};
const _CHROMA_GUARD = {
  luma_plus_chroma_guard: {
    max_chroma_shift: 0.25,
    chroma_region_factors: {
      flat: 1.0, textured: 0.9, strong_edge: 0.65,
      microtexture: 0.8, risky_halo_zone: 0.45,
    },
    saturation_guard: { min_scale: 0.6, gamma: 1.5 },
  },
};

/** Named pipeline presets. */
export const PIPELINE_PRESETS: Record<string, Partial<AutoSharpParams>> = {
  // photo (default): P0=0.003 — natural photographs.
  //   Coarse range [0.003, 1.00], 7 probes.
  photo: {
    target_artifact_ratio: 0.003,
    probe_strengths: {
      TwoPass: {
        coarse_count: 7, coarse_min: 0.003, coarse_max: 1.00,
        dense_count: 4, window_margin: 0.5,
      },
    },
    sharpen_strategy: { ..._CA_STRATEGY },
    experimental_sharpen_mode: { ..._CHROMA_GUARD },
    evaluator_config: "heuristic",
  },
  // precision: P0=0.001 — text, UI, architecture.
  //   Coarse range [0.003, 0.50], 7 probes.
  precision: {
    target_artifact_ratio: 0.001,
    probe_strengths: {
      TwoPass: {
        coarse_count: 7, coarse_min: 0.003, coarse_max: 0.50,
        dense_count: 4, window_margin: 0.5,
      },
    },
    sharpen_strategy: { ..._CA_STRATEGY },
    experimental_sharpen_mode: { ..._CHROMA_GUARD },
    evaluator_config: "heuristic",
  },
};

export const DEFAULT_CONTENT_ADAPTIVE_STRATEGY: ContentAdaptiveStrategy = {
  strategy: "content_adaptive",
  classification: { ...DEFAULT_CLASSIFICATION_PARAMS },
  gain_table: { ...DEFAULT_GAIN_TABLE },
  max_backoff_iterations: 4,
  backoff_scale_factor: 0.8,
};

export const DEFAULT_CONTENT_ADAPTIVE_RESIZE_STRATEGY: ContentAdaptiveResizeStrategy = {
  strategy: "content_adaptive",
  classification: { ...DEFAULT_CLASSIFICATION_PARAMS },
  kernel_table: { ...DEFAULT_KERNEL_TABLE },
};
