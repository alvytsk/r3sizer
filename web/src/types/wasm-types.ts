export type SharpenMode = "lightness" | "rgb";
export type SharpenModel = "practical_usm" | "paper_lightness_approx";
export type MetricMode = "absolute_total" | "relative_to_base";
export type ArtifactMetric = "channel_clipping_ratio" | "pixel_out_of_gamut_ratio";
export type FitStrategy = "Cubic" | "DirectSearch";
export type ClampPolicy = "Clamp" | "Normalize";
export type DiagnosticsLevel = "summary" | "full";
export type FitStatus =
  | { status: "success" }
  | { status: "failed"; reason: string }
  | { status: "skipped"; reason: string };
export type SelectionMode =
  | "polynomial_root"
  | "best_sample_within_budget"
  | "least_bad_sample"
  | "budget_unreachable";
export type CrossingStatus = "found" | "not_found_in_range" | "not_attempted";
export type Provenance =
  | "paper_confirmed"
  | "paper_supported"
  | "engineering_choice"
  | "engineering_proxy"
  | "placeholder";
export type FallbackReason =
  | "fit_failed"
  | "fit_unstable"
  | "root_out_of_range"
  | "metric_non_monotonic"
  | "budget_too_strict_for_content"
  | "direct_search_configured"
  | null;

export type ProbeConfig =
  | { Range: { min: number; max: number; count: number } }
  | { Explicit: number[] };

export interface MetricWeights {
  gamut_excursion: number;
  halo_ringing: number;
  edge_overshoot: number;
  texture_flattening: number;
}

export interface AutoSharpParams {
  target_width: number;
  target_height: number;
  probe_strengths: ProbeConfig;
  target_artifact_ratio: number;
  enable_contrast_leveling: boolean;
  sharpen_sigma: number;
  fit_strategy: FitStrategy;
  output_clamp: ClampPolicy;
  sharpen_mode: SharpenMode;
  sharpen_model: SharpenModel;
  metric_mode: MetricMode;
  artifact_metric: ArtifactMetric;
  metric_weights: MetricWeights;
  diagnostics_level: DiagnosticsLevel;
}

export interface ImageSize {
  width: number;
  height: number;
}

export interface FitQuality {
  residual_sum_of_squares: number;
  r_squared: number;
  max_residual: number;
  min_pivot: number;
}

export interface RobustnessFlags {
  monotonic: boolean;
  quasi_monotonic: boolean;
  r_squared_ok: boolean;
  well_conditioned: boolean;
  loo_stable: boolean;
  max_loo_root_change: number;
}

export interface CubicPolynomial {
  a: number;
  b: number;
  c: number;
  d: number;
}

export interface StageTiming {
  resize_us: number;
  contrast_us: number;
  baseline_us: number;
  probing_us: number;
  fit_us: number;
  robustness_us: number;
  final_sharpen_us: number;
  clamp_us: number;
  total_us: number;
}

export type MetricComponentName =
  | "gamut_excursion"
  | "halo_ringing"
  | "edge_overshoot"
  | "texture_flattening";

export interface MetricBreakdown {
  components: Record<MetricComponentName, number>;
  selected_metric: MetricComponentName;
  selection_score: number;
  composite_score: number;
  aggregate: number; // deprecated legacy alias for selection_score
}

export interface ProbeSample {
  strength: number;
  artifact_ratio: number;
  metric_value: number;
  breakdown: MetricBreakdown | null;
}

export interface StageProvenance {
  resize: Provenance;
  sharpen: Provenance;
  metric: Provenance;
  fit: Provenance;
  solve: Provenance;
  contrast: Provenance;
  lightness_reconstruction: Provenance;
}

export interface AutoSharpDiagnostics {
  input_size: ImageSize;
  output_size: ImageSize;
  sharpen_mode: SharpenMode;
  sharpen_model: SharpenModel;
  metric_mode: MetricMode;
  artifact_metric: ArtifactMetric;
  target_artifact_ratio: number;
  baseline_artifact_ratio: number;
  probe_samples: ProbeSample[];
  fit_status: FitStatus;
  fit_coefficients: CubicPolynomial | null;
  fit_quality: FitQuality | null;
  crossing_status: CrossingStatus;
  selected_strength: number;
  selection_mode: SelectionMode;
  fallback_reason: FallbackReason;
  robustness: RobustnessFlags | null;
  budget_reachable: boolean;
  measured_artifact_ratio: number;
  measured_metric_value: number;
  metric_components: MetricBreakdown | null;
  metric_weights: MetricWeights;
  metric_weights_provenance: Provenance;
  timing: StageTiming;
  provenance: StageProvenance;
}

export interface ProcessResult {
  imageData: Uint8Array;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics;
}

export const DEFAULT_METRIC_WEIGHTS: MetricWeights = {
  gamut_excursion: 1.0,
  halo_ringing: 0.3,
  edge_overshoot: 0.3,
  texture_flattening: 0.1,
};

export const DEFAULT_PARAMS: AutoSharpParams = {
  target_width: 800,
  target_height: 600,
  probe_strengths: { Explicit: [0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0] },
  target_artifact_ratio: 0.001,
  enable_contrast_leveling: false,
  sharpen_sigma: 1.0,
  fit_strategy: "Cubic",
  output_clamp: "Clamp",
  sharpen_mode: "lightness",
  sharpen_model: "practical_usm",
  metric_mode: "relative_to_base",
  artifact_metric: "channel_clipping_ratio",
  metric_weights: { ...DEFAULT_METRIC_WEIGHTS },
  diagnostics_level: "full",
};
