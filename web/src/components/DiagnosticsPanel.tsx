import { useState, useCallback } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useProcessorStore } from "@/stores/processor-store";
import { StatusIndicators } from "./StatusIndicators";
import { ProbeChart } from "./ProbeChart";
import { TimingBar } from "./TimingBar";
import type {
  AutoSharpDiagnostics,
  RobustnessFlags,
  RegionCoverage,
  AdaptiveValidationOutcome,
  Recommendation,
  RecommendationKind,
  Severity as RecSeverity,
} from "@/types/wasm-types";

const COMPONENT_LABELS: Record<string, string> = {
  gamut_excursion: "Gamut Excursion",
  halo_ringing: "Halo Ringing",
  edge_overshoot: "Edge Overshoot",
  texture_flattening: "Texture Flattening",
};

const REGION_LABELS: [keyof RegionCoverage, keyof RegionCoverage, string][] = [
  ["flat", "flat_fraction", "Flat"],
  ["textured", "textured_fraction", "Textured"],
  ["strong_edge", "strong_edge_fraction", "Strong Edge"],
  ["microtexture", "microtexture_fraction", "Microtexture"],
  ["risky_halo_zone", "risky_halo_zone_fraction", "Risky Halo"],
];

const REGION_COLORS = [
  "bg-chart-4",
  "bg-chart-2",
  "bg-chart-1",
  "bg-chart-3",
  "bg-chart-5",
];

function RegionCoverageBar({ coverage }: { coverage: RegionCoverage }) {
  return (
    <div className="space-y-1.5">
      <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50">
        Region Coverage
      </div>
      <div className="flex h-2 rounded-[2px] overflow-hidden bg-background border border-border/20">
        {REGION_LABELS.map(([, fracKey], i) => {
          const pct = (coverage[fracKey] as number) * 100;
          if (pct < 0.3) return null;
          return (
            <div
              key={fracKey}
              className={`${REGION_COLORS[i]} opacity-70`}
              style={{ width: `${pct}%` }}
            />
          );
        })}
      </div>
      <div className="space-y-0.5">
        {REGION_LABELS.map(([countKey, fracKey, label], i) => {
          const frac = coverage[fracKey] as number;
          const count = coverage[countKey] as number;
          return (
            <div key={countKey} className="flex items-center gap-2 text-[11px]">
              <div className={`w-1.5 h-1.5 rounded-[1px] shrink-0 ${REGION_COLORS[i]}`} />
              <span className="text-muted-foreground flex-1">{label}</span>
              <span className="font-mono tabular-nums text-foreground/70 w-[42px] text-right">
                {(frac * 100).toFixed(1)}%
              </span>
              <span className="font-mono tabular-nums text-muted-foreground/50 w-[52px] text-right">
                {count.toLocaleString()}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function AdaptiveValidationCard({ outcome }: { outcome: AdaptiveValidationOutcome }) {
  const isPassed =
    outcome.outcome === "passed_direct" || outcome.outcome === "passed_after_backoff";
  const borderColor = isPassed ? "border-chart-3/25" : "border-destructive/30";
  const bgColor = isPassed ? "bg-chart-3/5" : "bg-destructive/5";
  const dotColor = isPassed ? "bg-chart-3" : "bg-destructive";
  const headlineColor = isPassed ? "text-chart-3" : "text-destructive";

  let headline: string;
  let detail: string;
  if (outcome.outcome === "passed_direct") {
    headline = "Adaptive: passed direct";
    detail = `No backoff needed. Measured metric: ${outcome.measured_metric.toExponential(3)}`;
  } else if (outcome.outcome === "passed_after_backoff") {
    headline = `Adaptive: passed after ${outcome.iterations} backoff`;
    detail = `Final scale: ${outcome.final_scale.toFixed(3)}, measured metric: ${outcome.measured_metric.toExponential(3)}`;
  } else {
    headline = `Adaptive: budget exceeded (${outcome.iterations} iterations)`;
    detail = `Best scale: ${outcome.best_scale.toFixed(3)}, best metric: ${outcome.best_metric.toExponential(3)}`;
  }

  return (
    <div className={`rounded-sm border ${borderColor} ${bgColor} px-3 py-2`}>
      <div className="flex items-center gap-1.5 mb-0.5">
        <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${dotColor}`} />
        <span className={`text-[10px] font-mono font-medium uppercase tracking-[0.12em] ${headlineColor}`}>
          {headline}
        </span>
      </div>
      <p className="text-[12px] text-muted-foreground leading-relaxed pl-3">{detail}</p>
    </div>
  );
}

function Readout({ label, value }: { label: React.ReactNode; value: string | number }) {
  return (
    <div className="flex justify-between text-[13px] py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono text-foreground/90">{value}</span>
    </div>
  );
}

type Severity = "ok" | "warn" | "error";

interface DiagnosisEntry {
  severity: Severity;
  headline: string;
  detail: string;
}

function buildDiagnosis(d: AutoSharpDiagnostics): DiagnosisEntry[] {
  const entries: DiagnosisEntry[] = [];

  // Selection mode
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

  // Fallback reason
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

  // Robustness failures
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

const severityStyle = {
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

function DiagnosisCard({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  const entries = buildDiagnosis(diagnostics);
  if (entries.length === 0) return null;

  return (
    <div className="space-y-1.5">
      {entries.map((entry, i) => {
        const s = severityStyle[entry.severity];
        return (
          <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2`}>
            <div className="flex items-center gap-1.5 mb-0.5">
              <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${s.dot}`} />
              <span
                className={`text-[10px] font-mono font-medium uppercase tracking-[0.12em] ${s.headline}`}
              >
                {entry.headline}
              </span>
            </div>
            <p className="text-[12px] text-muted-foreground leading-relaxed pl-3">
              {entry.detail}
            </p>
          </div>
        );
      })}
    </div>
  );
}

type ChipVariant = "ok" | "warn" | "error" | "neutral";

const CHIP_STYLES: Record<ChipVariant, { border: string; bg: string; text: string }> = {
  ok:      { border: "border-chart-3/25",     bg: "bg-chart-3/5",      text: "text-chart-3"          },
  warn:    { border: "border-primary/25",      bg: "bg-primary/5",      text: "text-primary"          },
  error:   { border: "border-destructive/30",  bg: "bg-destructive/8",  text: "text-destructive"      },
  neutral: { border: "border-border/25",       bg: "bg-background",     text: "text-muted-foreground" },
};

function StatusChip({
  heading,
  value,
  variant,
}: {
  heading: string;
  value: string;
  variant: ChipVariant;
}) {
  const s = CHIP_STYLES[variant];
  return (
    <div className={`flex-1 rounded-sm border px-2.5 py-2 ${s.border} ${s.bg}`}>
      <div className="text-[9px] font-mono uppercase tracking-[0.2em] text-muted-foreground/50 mb-1">
        {heading}
      </div>
      <div className={`text-[12px] font-mono font-medium leading-none ${s.text}`}>
        {value.replace(/_/g, " ")}
      </div>
    </div>
  );
}

function PolyCoeffTable({
  a, b, c, d,
}: {
  a: number; b: number; c: number; d: number;
}) {
  const rows: { sup: string; coeff: number }[] = [
    { sup: "s³", coeff: a },
    { sup: "s²", coeff: b },
    { sup: "s",  coeff: c },
    { sup: "1",  coeff: d },
  ];
  return (
    <div className="rounded-sm border border-border/25 bg-background px-3 py-2.5">
      <div className="text-[9px] font-mono uppercase tracking-[0.2em] text-muted-foreground/45 mb-2">
        P(s) = as³ + bs² + cs + d
      </div>
      <div className="space-y-1">
        {rows.map(({ sup, coeff }) => (
          <div key={sup} className="flex items-baseline gap-2">
            <span className="text-[11px] font-mono text-primary/55 w-5 shrink-0">{sup}</span>
            <div className="flex-1 border-b border-border/10" />
            <span
              className={`text-[12px] font-mono tabular-nums ${
                coeff < 0 ? "text-chart-5/90" : "text-foreground/80"
              }`}
            >
              {coeff.toExponential(4)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function R2Gauge({ value }: { value: number }) {
  const THRESHOLD = 0.85;
  const ok = value >= THRESHOLD;
  const pct = Math.min(100, Math.max(0, value * 100));
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-mono text-muted-foreground">R²</span>
        <div className="flex items-center gap-1.5">
          <span
            className={`text-[12px] font-mono tabular-nums ${ok ? "text-chart-3" : "text-destructive"}`}
          >
            {value.toFixed(6)}
          </span>
          <span
            className={`text-[10px] font-mono px-1 rounded border ${
              ok
                ? "text-chart-3/70 border-chart-3/25 bg-chart-3/5"
                : "text-destructive/80 border-destructive/25 bg-destructive/5"
            }`}
          >
            {ok ? "ok" : "low"}
          </span>
        </div>
      </div>
      <div className="relative h-1.5">
        <div className="absolute inset-0 rounded-full bg-border/25" />
        <div
          className={`absolute top-0 left-0 h-full rounded-full ${
            ok ? "bg-chart-3/65" : "bg-destructive/55"
          }`}
          style={{ width: `${pct}%` }}
        />
        <div
          className="absolute top-1/2 -translate-y-1/2 w-px h-3 bg-primary/45"
          style={{ left: `${THRESHOLD * 100}%` }}
        />
      </div>
      <div className="relative h-3">
        <span
          className="absolute text-[9px] font-mono text-muted-foreground/35 -translate-x-1/2"
          style={{ left: `${THRESHOLD * 100}%` }}
        >
          0.85
        </span>
      </div>
    </div>
  );
}

function PivotBadge({ pivot }: { pivot: number }) {
  const ok = pivot > 1e-8;
  const marginal = pivot > 1e-12;
  if (ok)
    return (
      <span className="text-[10px] font-mono text-chart-3/70 border border-chart-3/20 bg-chart-3/5 px-1 py-px rounded">
        stable
      </span>
    );
  if (marginal)
    return (
      <span className="text-[10px] font-mono text-primary/80 border border-primary/20 bg-primary/5 px-1 py-px rounded">
        marginal
      </span>
    );
  return (
    <span className="text-[10px] font-mono text-destructive/80 border border-destructive/25 bg-destructive/5 px-1 py-px rounded">
      ill-cond.
    </span>
  );
}

type RobCheckKey = "monotonic" | "quasi_monotonic" | "r_squared_ok" | "well_conditioned" | "loo_stable";

const ROBUSTNESS_CHECKS: { key: RobCheckKey; short: string; full: string }[] = [
  { key: "monotonic",       short: "mono",  full: "Strict monotonicity"       },
  { key: "quasi_monotonic", short: "quasi", full: "Quasi-monotonicity"        },
  { key: "r_squared_ok",    short: "R²≥.85",full: "Fit R² ≥ 0.85"            },
  { key: "well_conditioned",short: "cond.", full: "Matrix conditioning"       },
  { key: "loo_stable",      short: "LOO",   full: "Leave-one-out stability"   },
];

const ROBUSTNESS_FAIL_HINTS: Record<RobCheckKey, string> = {
  monotonic:       "P(s) decreased at some probe — mild curve irregularity.",
  quasi_monotonic: "Multiple inversions in P(s) — cubic model is unreliable for this data.",
  r_squared_ok:    "R² < 0.85 — add more probes or widen the probe range.",
  well_conditioned:"min_pivot ≤ 1e-8 — probe spacings cause numerical instability in the solve.",
  loo_stable:      "Removing any single probe shifts s* significantly — result is noise-sensitive.",
};

function RobustnessGrid({ robustness }: { robustness: RobustnessFlags }) {
  const failedKeys = ROBUSTNESS_CHECKS.filter(({ key }) => !robustness[key]).map(
    ({ key }) => key
  );

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-5 gap-1">
        {ROBUSTNESS_CHECKS.map(({ key, short, full }) => {
          const ok = robustness[key];
          return (
            <div
              key={key}
              title={full}
              className={`rounded-sm border text-center py-1.5 px-0.5 ${
                ok
                  ? "border-border/20 bg-transparent"
                  : "border-destructive/30 bg-destructive/5"
              }`}
            >
              <div
                className={`text-[9px] font-mono leading-none mb-1 ${
                  ok ? "text-muted-foreground/50" : "text-destructive/65"
                }`}
              >
                {short}
              </div>
              <div
                className={`text-[13px] font-mono leading-none ${
                  ok ? "text-chart-3/65" : "text-destructive"
                }`}
              >
                {ok ? "✓" : "✗"}
              </div>
            </div>
          );
        })}
      </div>

      <div className="flex items-center justify-between text-[11px] font-mono py-0.5">
        <span className="text-muted-foreground/60">Max LOO Δs*</span>
        <span
          className={`tabular-nums ${
            !robustness.loo_stable ? "text-destructive" : "text-foreground/70"
          }`}
        >
          {robustness.max_loo_root_change.toFixed(4)}
        </span>
      </div>

      {failedKeys.length > 0 && (
        <div className="space-y-1 border-t border-border/15 pt-2">
          {failedKeys.map((key) => (
            <div key={key} className="flex items-start gap-1.5">
              <span className="text-[10px] font-mono text-destructive/50 mt-0.5 shrink-0">✗</span>
              <span className="text-[11px] font-mono text-muted-foreground/55 leading-snug">
                {ROBUSTNESS_FAIL_HINTS[key]}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const FIT_STATUS_VARIANTS: Record<string, ChipVariant> = {
  success: "ok",
  failed: "error",
  skipped: "warn",
};

function fitStatusVariant(status: string | undefined): ChipVariant {
  if (!status) return "neutral";
  return FIT_STATUS_VARIANTS[status] ?? "neutral";
}

const CROSSING_STATUS_VARIANTS: Record<string, ChipVariant> = {
  found: "ok",
  not_found_in_range: "warn",
};

function crossingStatusVariant(status: string): ChipVariant {
  return CROSSING_STATUS_VARIANTS[status] ?? "neutral";
}

interface Advice {
  icon: string;
  title: string;
  body: string;
  kind: "success" | "tip" | "warning";
}

function buildAdvice(d: AutoSharpDiagnostics): Advice[] {
  const advice: Advice[] = [];
  const ratio = d.measured_artifact_ratio;
  const target = d.target_artifact_ratio;
  const strength = d.selected_strength;

  // Recommendation kinds present — used to suppress overlapping generic advice.
  const recKinds = new Set<RecommendationKind>(
    (d.recommendations ?? []).map((r) => r.kind)
  );

  // Overall result quality
  if (d.selection_mode === "polynomial_root" && ratio <= target * 1.1) {
    advice.push({
      icon: "\u2713",
      title: "Optimal result",
      body: "The polynomial solve found an analytical root. Sharpening strength is well-calibrated for this image.",
      kind: "success",
    });
  }

  // Budget issues
  if (d.selection_mode === "budget_unreachable") {
    advice.push({
      icon: "!",
      title: "Budget unreachable",
      body: "All probe strengths exceeded the artifact budget. Try increasing Target P(s) (e.g. from 1e-3 to 1e-2), reducing output resolution, or switching to Lightness sharpening mode.",
      kind: "warning",
    });
  } else if (ratio > target * 2) {
    advice.push({
      icon: "!",
      title: "Measured artifacts exceed target",
      body: `Measured P is ${(ratio / target).toFixed(1)}x the target. Consider raising Target P(s) or lowering sigma to reduce sharpening intensity.`,
      kind: "warning",
    });
  }

  // Strength advice
  if (strength < 0.02 && d.selection_mode !== "budget_unreachable") {
    advice.push({
      icon: "\u2193",
      title: "Very low sharpening applied",
      body: "Strength is below 0.02 — the image may appear soft. You can raise Target P(s) to allow more sharpening, or this image may simply not need much.",
      kind: "tip",
    });
  }

  const probeMax = d.probe_samples.length > 0
    ? Math.max(...d.probe_samples.map((p) => p.strength))
    : 0;
  if (strength > 0 && probeMax > 0 && strength >= probeMax * 0.95) {
    advice.push({
      icon: "\u2191",
      title: "Strength near probe limit",
      body: "Selected strength is at the upper edge of probe range. Consider adding higher probe values (e.g. 0.7, 1.0) so the solver has more room to find an optimal point.",
      kind: "tip",
    });
  }

  // Baseline warning
  if (d.baseline_artifact_ratio > target * 0.5 && d.baseline_artifact_ratio > 0) {
    advice.push({
      icon: "\u26A0",
      title: "High baseline artifacts",
      body: `The resize step alone produces ${(d.baseline_artifact_ratio * 100).toFixed(2)}% artifacts before any sharpening. This content may be too detailed for the target resolution. Try a larger output size or a smoother resize kernel (Gaussian).`,
      kind: "warning",
    });
  }

  // Sigma advice (suppressed when recommendation covers this)
  if (d.sharpen_mode === "rgb" && !recKinds.has("switch_to_lightness")) {
    advice.push({
      icon: "\u2192",
      title: "Consider Lightness mode",
      body: "RGB mode sharpens all color channels independently, which can amplify color fringing. Lightness mode only sharpens luminance — it typically produces fewer color artifacts.",
      kind: "tip",
    });
  }

  // Fit quality (suppressed when recommendation covers this)
  if (d.fit_quality && d.fit_quality.r_squared < 0.85 && !recKinds.has("widen_probe_range")) {
    advice.push({
      icon: "\u223C",
      title: "Poor polynomial fit",
      body: `R\u00b2 = ${d.fit_quality.r_squared.toFixed(3)} is below 0.85. The cubic model doesn't closely match probe data. Try adding more probe points or widening the probe range for a better fit.`,
      kind: "warning",
    });
  }

  // Robustness
  if (d.robustness && !d.robustness.loo_stable) {
    advice.push({
      icon: "\u2248",
      title: "Result is noise-sensitive",
      body: "Leave-one-out analysis shows the selected strength shifts significantly when any single probe is removed. Adding more probe samples will stabilize the result.",
      kind: "tip",
    });
  }

  // Region-based advice (halo-risk suppressed when recommendation covers this)
  if (d.region_coverage) {
    const rc = d.region_coverage;
    if (rc.risky_halo_zone_fraction > 0.15
      && !recKinds.has("switch_to_content_adaptive")
      && !recKinds.has("lower_strong_edge_gain")) {
      advice.push({
        icon: "\u25CB",
        title: "High halo-risk content",
        body: `${(rc.risky_halo_zone_fraction * 100).toFixed(0)}% of the image is in the halo risk zone (strong edges next to flat areas). Consider Content Adaptive strategy with reduced strong_edge gain, or lower sigma.`,
        kind: "tip",
      });
    }
    if (rc.flat_fraction > 0.7) {
      advice.push({
        icon: "\u2014",
        title: "Mostly flat image",
        body: "Over 70% of the image is flat regions. Sharpening has little to enhance — the result should be clean. If you see noise amplification, reduce sigma.",
        kind: "tip",
      });
    }
  }

  if (advice.length === 0 && (!d.recommendations || d.recommendations.length === 0)) {
    advice.push({
      icon: "\u2713",
      title: "Looking good",
      body: "No issues detected. The current settings appear well-suited for this image.",
      kind: "success",
    });
  }

  return advice;
}

const REC_SEVERITY_STYLES: Record<RecSeverity, { border: string; bg: string; title: string }> = {
  warning: {
    border: "border-primary/25",
    bg: "bg-primary/5",
    title: "text-primary",
  },
  suggestion: {
    border: "border-chart-2/25",
    bg: "bg-chart-2/5",
    title: "text-chart-2",
  },
  info: {
    border: "border-muted-foreground/15",
    bg: "bg-muted/5",
    title: "text-muted-foreground",
  },
};

const REC_KIND_LABELS: Record<RecommendationKind, string> = {
  switch_to_content_adaptive: "Content-adaptive sharpening recommended",
  lower_strong_edge_gain: "Reduce strong-edge gain",
  raise_artifact_budget: "Raise artifact budget",
  switch_to_lightness: "Switch to lightness mode",
  widen_probe_range: "Widen probe range",
  lower_sigma: "Lower blur sigma",
  switch_to_hybrid: "Switch to hybrid selection policy",
};

const ADVICE_STYLES: Record<Advice["kind"], { border: string; bg: string; icon: string; title: string }> = {
  success: {
    border: "border-chart-3/25",
    bg: "bg-chart-3/5",
    icon: "text-chart-3",
    title: "text-chart-3",
  },
  tip: {
    border: "border-chart-2/25",
    bg: "bg-chart-2/5",
    icon: "text-chart-2",
    title: "text-chart-2",
  },
  warning: {
    border: "border-primary/25",
    bg: "bg-primary/5",
    icon: "text-primary",
    title: "text-primary",
  },
};

function RecommendationCards({ recommendations }: { recommendations: Recommendation[] }) {
  const updateParams = useProcessorStore((s) => s.updateParams);

  if (recommendations.length === 0) return null;

  const applyPatch = (rec: Recommendation) => {
    // Build a clean Partial<AutoSharpParams> from non-null patch fields.
    const p = rec.patch;
    updateParams({
      ...(p.sharpen_strategy != null && { sharpen_strategy: p.sharpen_strategy }),
      ...(p.target_artifact_ratio != null && { target_artifact_ratio: p.target_artifact_ratio }),
      ...(p.sharpen_mode != null && { sharpen_mode: p.sharpen_mode }),
      ...(p.probe_strengths != null && { probe_strengths: p.probe_strengths }),
      ...(p.sharpen_sigma != null && { sharpen_sigma: p.sharpen_sigma }),
    });
  };

  const applyAll = () => {
    for (const rec of recommendations) {
      applyPatch(rec);
    }
  };

  return (
    <>
      {recommendations.map((rec, i) => {
        const s = REC_SEVERITY_STYLES[rec.severity];
        return (
          <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2.5`}>
            <div className="flex items-start gap-2">
              <span className={`text-[14px] font-mono leading-none mt-0.5 shrink-0 ${s.title}`}>
                {"\u2606"}
              </span>
              <div className="space-y-1 min-w-0 flex-1">
                <div className={`text-[12px] font-mono font-medium ${s.title}`}>
                  {REC_KIND_LABELS[rec.kind] ?? rec.kind}
                </div>
                <p className="text-[12px] text-muted-foreground leading-relaxed">
                  {rec.reason}
                </p>
                <button
                  type="button"
                  className="text-[11px] font-mono font-medium text-primary hover:text-primary/80 transition-colors mt-0.5"
                  onClick={() => applyPatch(rec)}
                >
                  Apply
                </button>
              </div>
            </div>
          </div>
        );
      })}
      {recommendations.length > 1 && (
        <button
          type="button"
          className="w-full text-[11px] font-mono font-medium text-primary/70 hover:text-primary border border-primary/20 hover:border-primary/40 hover:bg-primary/5 rounded-md transition-colors text-center py-1.5"
          onClick={applyAll}
        >
          Apply all recommendations
        </button>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// JSON viewer with syntax highlighting, copy & download
// ---------------------------------------------------------------------------

/** Simple JSON syntax highlighter — no external deps. */
function highlightJson(json: string): string {
  return json.replace(
    /("(?:\\.|[^"\\])*")\s*:/g,
    '<span class="text-sky-400">$1</span>:'
  ).replace(
    /:\s*("(?:\\.|[^"\\])*")/g,
    ': <span class="text-amber-300">$1</span>'
  ).replace(
    /:\s*(-?\d+\.?\d*(?:e[+-]?\d+)?)/gi,
    ': <span class="text-emerald-400">$1</span>'
  ).replace(
    /:\s*(true|false)/g,
    ': <span class="text-violet-400">$1</span>'
  ).replace(
    /:\s*(null)/g,
    ': <span class="text-rose-400/60">$1</span>'
  );
}

function JsonViewer({ data }: { data: unknown }) {
  const [copied, setCopied] = useState(false);
  const json = JSON.stringify(data, null, 2);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(json).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [json]);

  const handleDownload = useCallback(() => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `r3sizer-diag-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [json]);

  return (
    <div className="space-y-2">
      <div className="flex gap-1.5">
        <button
          onClick={handleCopy}
          className="text-[11px] font-mono px-2 py-0.5 rounded border border-border/40 bg-muted hover:bg-muted/80 text-muted-foreground transition-colors"
        >
          {copied ? "Copied!" : "Copy"}
        </button>
        <button
          onClick={handleDownload}
          className="text-[11px] font-mono px-2 py-0.5 rounded border border-border/40 bg-muted hover:bg-muted/80 text-muted-foreground transition-colors"
        >
          Download
        </button>
      </div>
      <pre
        className="text-[11px] leading-[1.6] font-mono bg-[#0d1117] text-[#c9d1d9] p-3 rounded-md border border-border/20 overflow-auto max-h-[450px] selection:bg-sky-500/20"
        dangerouslySetInnerHTML={{ __html: highlightJson(json) }}
      />
    </div>
  );
}

export function DiagnosticsPanel() {
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  if (!diagnostics) return null;

  return (
    <div className="p-3">
      <Tabs defaultValue="advice" className="w-full">
        <TabsList variant="line" className="grid grid-cols-5 w-full h-8">
          <TabsTrigger value="advice" className="text-[13px] font-mono">
            Advice
          </TabsTrigger>
          <TabsTrigger value="summary" className="text-[13px] font-mono">
            Summary
          </TabsTrigger>
          <TabsTrigger value="fit" className="text-[13px] font-mono">
            Fit
          </TabsTrigger>
          <TabsTrigger value="timing" className="text-[13px] font-mono">
            Timing
          </TabsTrigger>
          <TabsTrigger value="json" className="text-[13px] font-mono">
            JSON
          </TabsTrigger>
        </TabsList>

        {/* ── Summary ── */}
        <TabsContent value="summary" className="space-y-3 mt-3">
          <StatusIndicators diagnostics={diagnostics} />
          <DiagnosisCard diagnostics={diagnostics} />
          {diagnostics.adaptive_validation && (
            <AdaptiveValidationCard outcome={diagnostics.adaptive_validation} />
          )}
          {diagnostics.region_coverage && (
            <RegionCoverageBar coverage={diagnostics.region_coverage} />
          )}
          <div className="space-y-0.5 border-t border-border/30 pt-2">
            <Readout
              label="Selected strength"
              value={diagnostics.selected_strength.toFixed(4)}
            />
            <Readout
              label={<>Target P<sub>0</sub></>}
              value={diagnostics.target_artifact_ratio.toExponential(2)}
            />
            <Readout
              label="Measured P"
              value={diagnostics.measured_artifact_ratio.toExponential(3)}
            />
            <Readout
              label="Baseline P"
              value={diagnostics.baseline_artifact_ratio.toExponential(3)}
            />

            {diagnostics.metric_components && (
              <div className="mt-2 pt-2 border-t border-border/20 space-y-0.5">
                <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50 mb-1">
                  Metric Breakdown
                </div>
                {Object.entries(diagnostics.metric_components.components).map(
                  ([name, value]) => (
                    <div key={name} className="flex justify-between text-[12px] py-px">
                      <span className="text-muted-foreground/70">{COMPONENT_LABELS[name] ?? name}</span>
                      <span className="font-mono text-foreground/80">{(value as number).toExponential(2)}</span>
                    </div>
                  )
                )}
                <div className="flex justify-between text-[12px] pt-1 border-t border-border/10">
                  <span className="text-muted-foreground/50 italic">composite</span>
                  <span className="font-mono text-muted-foreground/60">
                    {diagnostics.metric_components.composite_score.toExponential(2)}
                  </span>
                </div>
              </div>
            )}

            <Readout
              label="Input"
              value={`${diagnostics.input_size.width}\u00d7${diagnostics.input_size.height}`}
            />
            <Readout
              label="Output"
              value={`${diagnostics.output_size.width}\u00d7${diagnostics.output_size.height}`}
            />
          </div>
          <ProbeChart diagnostics={diagnostics} />

          {/* ── Extended diagnostics ── */}
          {(diagnostics.input_ingress || diagnostics.resize_strategy_diagnostics ||
            diagnostics.chroma_guard || diagnostics.evaluator_result) && (
            <div className="space-y-2 border-t border-border/30 pt-2">

              {diagnostics.input_ingress && (
                <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
                  <div className="text-[11px] font-mono font-semibold text-muted-foreground">Ingress</div>
                  <Readout label="Color space" value={diagnostics.input_ingress.declared_color_space} />
                  {diagnostics.input_ingress.raw_value_min != null && (
                    <Readout label="Raw range" value={`${diagnostics.input_ingress.raw_value_min.toFixed(3)} – ${diagnostics.input_ingress.raw_value_max?.toFixed(3) ?? "?"}`} />
                  )}
                  {diagnostics.input_ingress.normalization_scale != null && (
                    <Readout label="Norm scale" value={diagnostics.input_ingress.normalization_scale.toFixed(4)} />
                  )}
                  {diagnostics.input_ingress.out_of_range_fraction != null && (
                    <Readout label="Out of range" value={`${(diagnostics.input_ingress.out_of_range_fraction * 100).toFixed(2)}%`} />
                  )}
                </div>
              )}

              {diagnostics.resize_strategy_diagnostics && (
                <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
                  <div className="text-[11px] font-mono font-semibold text-muted-foreground">Resize Strategy</div>
                  <Readout label="Kernels used" value={diagnostics.resize_strategy_diagnostics.kernels_used.join(", ")} />
                  {Object.entries(diagnostics.resize_strategy_diagnostics.per_kernel_pixel_count).map(
                    ([kernel, count]) => (
                      <Readout key={kernel} label={kernel} value={String(count)} />
                    )
                  )}
                </div>
              )}

              {diagnostics.chroma_guard && (
                <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
                  <div className="text-[11px] font-mono font-semibold text-muted-foreground">Chroma Guard</div>
                  <Readout label="Pixels clamped" value={`${(diagnostics.chroma_guard.pixels_clamped_fraction * 100).toFixed(2)}%`} />
                  <Readout label="Mean shift" value={diagnostics.chroma_guard.mean_chroma_shift.toFixed(4)} />
                  <Readout label="Max shift" value={diagnostics.chroma_guard.max_chroma_shift.toFixed(4)} />
                </div>
              )}

              {diagnostics.evaluator_result && (
                <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
                  <div className="text-[11px] font-mono font-semibold text-muted-foreground">Quality Evaluator</div>
                  <Readout label="Quality score" value={diagnostics.evaluator_result.predicted_quality_score.toFixed(3)} />
                  <Readout label="Confidence" value={diagnostics.evaluator_result.confidence.toFixed(3)} />
                  {diagnostics.evaluator_result.suggested_strength != null && (
                    <Readout label="Suggested s*" value={diagnostics.evaluator_result.suggested_strength.toFixed(4)} />
                  )}
                  <details className="mt-1">
                    <summary className="text-[10px] font-mono text-muted-foreground/50 cursor-pointer hover:text-primary transition-colors">
                      Features
                    </summary>
                    <div className="pt-1 space-y-0.5">
                      <Readout label="Edge density" value={diagnostics.evaluator_result.features.edge_density.toExponential(2)} />
                      <Readout label="Mean gradient" value={diagnostics.evaluator_result.features.mean_gradient_magnitude.toExponential(2)} />
                      <Readout label="Gradient var" value={diagnostics.evaluator_result.features.gradient_variance.toExponential(2)} />
                      <Readout label="Mean local var" value={diagnostics.evaluator_result.features.mean_local_variance.toExponential(2)} />
                      <Readout label="Local var var" value={diagnostics.evaluator_result.features.local_variance_variance.toExponential(2)} />
                      <Readout label="Laplacian var" value={diagnostics.evaluator_result.features.laplacian_variance.toExponential(2)} />
                      <Readout label="Luma entropy" value={diagnostics.evaluator_result.features.luminance_histogram_entropy.toFixed(3)} />
                    </div>
                  </details>
                </div>
              )}
            </div>
          )}
        </TabsContent>

        {/* ── Advice ── */}
        <TabsContent value="advice" className="space-y-2 mt-3">
          {buildAdvice(diagnostics).map((item, i) => {
            const s = ADVICE_STYLES[item.kind];
            return (
              <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2.5`}>
                <div className="flex items-start gap-2">
                  <span className={`text-[14px] font-mono leading-none mt-0.5 shrink-0 ${s.icon}`}>
                    {item.icon}
                  </span>
                  <div className="space-y-1 min-w-0">
                    <div className={`text-[12px] font-mono font-medium ${s.title}`}>
                      {item.title}
                    </div>
                    <p className="text-[12px] text-muted-foreground leading-relaxed">
                      {item.body}
                    </p>
                  </div>
                </div>
              </div>
            );
          })}
          <RecommendationCards recommendations={diagnostics.recommendations ?? []} />
        </TabsContent>

        {/* ── Fit ── */}
        <TabsContent value="fit" className="space-y-3 mt-3">

          <div className="flex gap-2">
            <StatusChip
              heading="Fit"
              value={diagnostics.fit_status?.status ?? "unknown"}
              variant={fitStatusVariant(diagnostics.fit_status?.status)}
            />
            <StatusChip
              heading="Root"
              value={diagnostics.crossing_status}
              variant={crossingStatusVariant(diagnostics.crossing_status)}
            />
          </div>

          {"status" in diagnostics.fit_status &&
            diagnostics.fit_status.status !== "success" &&
            "reason" in diagnostics.fit_status && (
              <p className="text-[11px] font-mono text-destructive/70 leading-relaxed">
                {diagnostics.fit_status.reason}
              </p>
            )}

          {diagnostics.fit_coefficients && (
            <div className="border-t border-border/30 pt-3">
              <PolyCoeffTable {...diagnostics.fit_coefficients} />
            </div>
          )}

          {diagnostics.fit_quality && (
            <div className="border-t border-border/30 pt-3 space-y-2">
              <div className="text-[9px] font-mono uppercase tracking-[0.2em] text-muted-foreground/45">
                Quality
              </div>
              <R2Gauge value={diagnostics.fit_quality.r_squared} />
              <div className="space-y-0.5 pt-1">
                <div className="flex items-center justify-between text-[11px] font-mono py-0.5">
                  <span className="text-muted-foreground">Max residual</span>
                  <span className="tabular-nums text-foreground/75">
                    {diagnostics.fit_quality.max_residual.toExponential(3)}
                  </span>
                </div>
                <div className="flex items-center justify-between text-[11px] font-mono py-0.5">
                  <span className="text-muted-foreground">RSS</span>
                  <span className="tabular-nums text-foreground/75">
                    {diagnostics.fit_quality.residual_sum_of_squares.toExponential(3)}
                  </span>
                </div>
                <div className="flex items-center justify-between text-[11px] font-mono py-0.5">
                  <span className="text-muted-foreground">Min pivot</span>
                  <div className="flex items-center gap-1.5">
                    <span className="tabular-nums text-foreground/75">
                      {diagnostics.fit_quality.min_pivot.toExponential(3)}
                    </span>
                    <PivotBadge pivot={diagnostics.fit_quality.min_pivot} />
                  </div>
                </div>
              </div>
            </div>
          )}

          {diagnostics.robustness && (
            <div className="border-t border-border/30 pt-3 space-y-2">
              <div className="text-[9px] font-mono uppercase tracking-[0.2em] text-muted-foreground/45">
                Robustness
              </div>
              <RobustnessGrid robustness={diagnostics.robustness} />
            </div>
          )}

        </TabsContent>

        {/* ── Timing ── */}
        <TabsContent value="timing" className="mt-3">
          <TimingBar timing={diagnostics.timing} />
        </TabsContent>

        {/* ── JSON ── */}
        <TabsContent value="json" className="mt-3">
          <JsonViewer data={diagnostics} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
