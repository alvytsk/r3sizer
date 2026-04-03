import type { AutoSharpDiagnostics } from "@/types/wasm-types";

export const COMPONENT_LABELS: Record<string, string> = {
  gamut_excursion: "Gamut Excursion",
  halo_ringing: "Halo Ringing",
  edge_overshoot: "Edge Overshoot",
  texture_flattening: "Texture Flattening",
};

export type ChipVariant = "ok" | "warn" | "error" | "neutral";

export type Severity = "ok" | "warn" | "error";

export const severityStyle: Record<Severity, { border: string; bg: string; dot: string; headline: string }> = {
  ok: {
    border: "border-chart-3/25",
    bg: "bg-chart-3/5",
    dot: "bg-chart-3",
    headline: "text-chart-3",
  },
  warn: {
    border: "border-primary/25",
    bg: "bg-primary/5",
    dot: "bg-primary",
    headline: "text-primary",
  },
  error: {
    border: "border-destructive/30",
    bg: "bg-destructive/5",
    dot: "bg-destructive",
    headline: "text-destructive",
  },
};

export interface DiagnosisEntry {
  severity: Severity;
  headline: string;
  detail: string;
}

export function buildDiagnosis(d: AutoSharpDiagnostics): DiagnosisEntry[] {
  const entries: DiagnosisEntry[] = [];

  switch (d.selection_mode) {
    case "polynomial_root":
      entries.push({
        severity: "ok",
        headline: "Optimal: polynomial root",
        detail:
          "Cubic P(s) = P\u2080 solved analytically. Sharpening strength is theoretically optimal given the probe data.",
      });
      break;
    case "best_sample_within_budget":
      entries.push({
        severity: "warn",
        headline: "Fallback: best probe sample within budget",
        detail:
          "No polynomial root found in the probed range. The probe sample closest to P\u2080 without exceeding it was used.",
      });
      break;
    case "least_bad_sample":
      entries.push({
        severity: "warn",
        headline: "Degraded: least-artifact sample",
        detail:
          "No probe satisfied the budget. The sample with the lowest artifact ratio was selected. Result exceeds P\u2080.",
      });
      break;
    case "budget_unreachable":
      entries.push({
        severity: "error",
        headline: "Failed: budget unreachable",
        detail:
          "The target P\u2080 cannot be met — all probed strengths produce artifacts above budget. Consider increasing P\u2080 or reducing output resolution.",
      });
      break;
  }

  if (d.fallback_reason) {
    const reasons: Record<string, { severity: Severity; text: string }> = {
      fit_failed: {
        severity: "error",
        text: "Cubic fit did not converge. The 4\u00d74 Vandermonde system returned no solution — the polynomial path was skipped entirely.",
      },
      fit_unstable: {
        severity: "error",
        text: "Numerical instability detected (min_pivot too small). Polynomial coefficients are unreliable; direct search was used instead.",
      },
      root_out_of_range: {
        severity: "warn",
        text: "Polynomial root fell outside [s_min, s_max]. Extrapolation is unsafe, so the best in-range probe was used.",
      },
      metric_non_monotonic: {
        severity: "warn",
        text: "P(s) did not increase monotonically with strength. The cubic model is unreliable; a direct probe sample was chosen.",
      },
      budget_too_strict_for_content: {
        severity: "error",
        text: `Baseline artifact ratio (${d.baseline_artifact_ratio.toExponential(3)}) already exceeds P\u2080. Even the unsharpened downscale is over budget — the content is too fine for this target resolution and threshold.`,
      },
      direct_search_configured: {
        severity: "ok",
        text: "Fit strategy is set to DirectSearch. Polynomial fitting was skipped by configuration; the best probe sample was selected directly.",
      },
    };
    const r = reasons[d.fallback_reason];
    if (r) {
      entries.push({
        severity: r.severity,
        headline: `Fallback reason: ${d.fallback_reason.replace(/_/g, " ")}`,
        detail: r.text,
      });
    }
  }

  if (d.robustness) {
    const { monotonic, quasi_monotonic, r_squared_ok, well_conditioned, loo_stable } =
      d.robustness;

    if (!quasi_monotonic) {
      entries.push({
        severity: "warn",
        headline: "Robustness: non-monotonic probe curve",
        detail:
          "P(s) has multiple inversions across probes. The cubic fit may not represent the true relationship between strength and artifacts.",
      });
    } else if (!monotonic) {
      entries.push({
        severity: "warn",
        headline: "Robustness: minor non-monotonicity",
        detail:
          "One probe inversion detected; quasi-monotonicity holds. The fit is acceptable but slightly less certain.",
      });
    }

    if (!r_squared_ok && d.fit_quality) {
      entries.push({
        severity: "warn",
        headline: `Robustness: poor fit quality (R\u00b2 = ${d.fit_quality.r_squared.toFixed(3)})`,
        detail:
          "R\u00b2 < 0.85 — the cubic does not closely track probe data. The root estimate has higher uncertainty. Adding more probes or adjusting the probe range may help.",
      });
    }

    if (!well_conditioned) {
      entries.push({
        severity: "warn",
        headline: "Robustness: ill-conditioned Vandermonde matrix",
        detail:
          "The min_pivot of the Vandermonde system is near zero. Probe strengths may be too close together or too sparse, causing numerical instability in the polynomial solve.",
      });
    }

    if (!loo_stable) {
      entries.push({
        severity: "warn",
        headline: `Robustness: LOO unstable (\u0394s* = ${d.robustness.max_loo_root_change.toFixed(3)})`,
        detail:
          "Leave-one-out cross-validation shows that removing any single probe shifts the root estimate significantly. The selected strength is sensitive to measurement noise.",
      });
    }
  }

  return entries;
}
