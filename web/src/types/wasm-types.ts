// Re-export all auto-generated types and defaults from Rust.
// Only manually-defined types and constants belong in this file.
//
// To regenerate generated.ts:
//   cargo test -p r3sizer-core --features typegen,experimental export_typescript_bindings -- --nocapture

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
  SharpenModel,
  MetricMode,
  ArtifactMetric,
  FitStrategy,
  ClampPolicy,
  DiagnosticsLevel,
  Provenance,
  CrossingStatus,
  SelectionMode,
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
  StageProvenance,
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
