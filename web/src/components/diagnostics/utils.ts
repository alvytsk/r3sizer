import type { TFunction } from "i18next";
import type { AutoSharpDiagnostics } from "@/types/wasm-types";

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

export function buildDiagnosis(d: AutoSharpDiagnostics, t: TFunction): DiagnosisEntry[] {
  const entries: DiagnosisEntry[] = [];

  switch (d.selection_mode) {
    case "polynomial_root":
      entries.push({
        severity: "ok",
        headline: t("diagnostics.selection.polynomialRoot"),
        detail: t("diagnostics.selection.polynomialRootDetail"),
      });
      break;
    case "best_sample_within_budget":
      entries.push({
        severity: "warn",
        headline: t("diagnostics.selection.bestSample"),
        detail: t("diagnostics.selection.bestSampleDetail"),
      });
      break;
    case "least_bad_sample":
      entries.push({
        severity: "warn",
        headline: t("diagnostics.selection.leastBad"),
        detail: t("diagnostics.selection.leastBadDetail"),
      });
      break;
    case "budget_unreachable":
      entries.push({
        severity: "error",
        headline: t("diagnostics.selection.budgetUnreachable"),
        detail: t("diagnostics.selection.budgetUnreachableDetail"),
      });
      break;
  }

  if (d.fallback_reason) {
    const reasons: Record<string, { severity: Severity; key: string; interpolation?: Record<string, string> }> = {
      fit_failed: {
        severity: "error",
        key: "diagnostics.fallback.fitFailed",
      },
      fit_unstable: {
        severity: "error",
        key: "diagnostics.fallback.fitUnstable",
      },
      root_out_of_range: {
        severity: "warn",
        key: "diagnostics.fallback.rootOutOfRange",
      },
      metric_non_monotonic: {
        severity: "warn",
        key: "diagnostics.fallback.metricNonMonotonic",
      },
      budget_too_strict_for_content: {
        severity: "error",
        key: "diagnostics.fallback.budgetTooStrict",
        interpolation: { value: d.baseline_artifact_ratio.toExponential(3) },
      },
      direct_search_configured: {
        severity: "ok",
        key: "diagnostics.fallback.directSearchConfigured",
      },
    };
    const r = reasons[d.fallback_reason];
    if (r) {
      entries.push({
        severity: r.severity,
        headline: t("diagnostics.fallback.fallbackReason", { reason: d.fallback_reason.replace(/_/g, " ") }),
        detail: t(r.key, r.interpolation),
      });
    }
  }

  if (d.robustness) {
    const { monotonic, quasi_monotonic, r_squared_ok, well_conditioned, loo_stable } =
      d.robustness;

    if (!quasi_monotonic) {
      entries.push({
        severity: "warn",
        headline: t("diagnostics.robustness.nonMonotonic"),
        detail: t("diagnostics.robustness.nonMonotonicDetail"),
      });
    } else if (!monotonic) {
      entries.push({
        severity: "warn",
        headline: t("diagnostics.robustness.minorNonMono"),
        detail: t("diagnostics.robustness.minorNonMonoDetail"),
      });
    }

    if (!r_squared_ok && d.fit_quality) {
      entries.push({
        severity: "warn",
        headline: t("diagnostics.robustness.poorFit", { value: d.fit_quality.r_squared.toFixed(3) }),
        detail: t("diagnostics.robustness.poorFitDetail"),
      });
    }

    if (!well_conditioned) {
      entries.push({
        severity: "warn",
        headline: t("diagnostics.robustness.illConditioned"),
        detail: t("diagnostics.robustness.illConditionedDetail"),
      });
    }

    if (!loo_stable) {
      entries.push({
        severity: "warn",
        headline: t("diagnostics.robustness.looUnstable", { value: d.robustness.max_loo_root_change.toFixed(3) }),
        detail: t("diagnostics.robustness.looUnstableDetail"),
      });
    }
  }

  return entries;
}
