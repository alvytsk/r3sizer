import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useProcessorStore } from "@/stores/processor-store";
import { StatusIndicators } from "./StatusIndicators";
import { ProbeChart } from "./ProbeChart";
import { TimingBar } from "./TimingBar";
import type { AutoSharpDiagnostics, RobustnessFlags } from "@/types/wasm-types";

const COMPONENT_LABELS: Record<string, string> = {
  gamut_excursion: "Gamut Excursion",
  halo_ringing: "Halo Ringing",
  edge_overshoot: "Edge Overshoot",
  texture_flattening: "Texture Flattening",
};

function Readout({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex justify-between text-[13px] py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono text-foreground/90">{value}</span>
    </div>
  );
}

// ─── Diagnosis card ──────────────────────────────────────────────────────────

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

// ─── Fit tab components ───────────────────────────────────────────────────────

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
      {/* Track + fill */}
      <div className="relative h-1.5">
        <div className="absolute inset-0 rounded-full bg-border/25" />
        <div
          className={`absolute top-0 left-0 h-full rounded-full ${
            ok ? "bg-chart-3/65" : "bg-destructive/55"
          }`}
          style={{ width: `${pct}%` }}
        />
        {/* Threshold tick */}
        <div
          className="absolute top-1/2 -translate-y-1/2 w-px h-3 bg-primary/45"
          style={{ left: `${THRESHOLD * 100}%` }}
        />
      </div>
      {/* Threshold label */}
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

// ─── Provenance ───────────────────────────────────────────────────────────────

const provenanceColors: Record<string, string> = {
  paper_confirmed: "text-chart-3",
  paper_supported: "text-chart-2",
  engineering_choice: "text-primary",
  engineering_proxy: "text-chart-5",
  placeholder: "text-destructive",
};

const provenanceDots: Record<string, string> = {
  paper_confirmed: "bg-chart-3",
  paper_supported: "bg-chart-2",
  engineering_choice: "bg-primary",
  engineering_proxy: "bg-chart-5",
  placeholder: "bg-destructive",
};

const provenanceLabels: Record<string, string> = {
  paper_confirmed: "Confirmed",
  paper_supported: "Supported",
  engineering_choice: "Eng. Choice",
  engineering_proxy: "Eng. Proxy",
  placeholder: "Placeholder",
};

// ─── Main panel ───────────────────────────────────────────────────────────────

export function DiagnosticsPanel() {
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  if (!diagnostics) return null;

  return (
    <div className="p-3">
      <Tabs defaultValue="summary" className="w-full">
        <TabsList variant="line" className="grid grid-cols-5 w-full h-8">
          <TabsTrigger value="summary" className="text-[13px] font-mono">
            Summary
          </TabsTrigger>
          <TabsTrigger value="fit" className="text-[13px] font-mono">
            Fit
          </TabsTrigger>
          <TabsTrigger value="timing" className="text-[13px] font-mono">
            Timing
          </TabsTrigger>
          <TabsTrigger value="provenance" className="text-[13px] font-mono">
            Prov.
          </TabsTrigger>
          <TabsTrigger value="json" className="text-[13px] font-mono">
            JSON
          </TabsTrigger>
        </TabsList>

        {/* ── Summary ── */}
        <TabsContent value="summary" className="space-y-3 mt-3">
          <StatusIndicators diagnostics={diagnostics} />
          <DiagnosisCard diagnostics={diagnostics} />
          <div className="space-y-0.5 border-t border-border/30 pt-2">
            <Readout
              label="Selected strength"
              value={diagnostics.selected_strength.toFixed(4)}
            />
            <Readout
              label="Target P\u2080"
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

            {/* v0.2 metric breakdown */}
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
        </TabsContent>

        {/* ── Fit ── */}
        <TabsContent value="fit" className="space-y-3 mt-3">

          {/* Status chips */}
          <div className="flex gap-2">
            <StatusChip
              heading="Fit"
              value={diagnostics.fit_status?.status ?? "unknown"}
              variant={
                diagnostics.fit_status?.status === "success" ? "ok"
                : diagnostics.fit_status?.status === "failed" ? "error"
                : diagnostics.fit_status?.status === "skipped" ? "warn"
                : "neutral"
              }
            />
            <StatusChip
              heading="Root"
              value={diagnostics.crossing_status}
              variant={
                diagnostics.crossing_status === "found" ? "ok"
                : diagnostics.crossing_status === "not_found_in_range" ? "warn"
                : "neutral"
              }
            />
          </div>

          {/* Fit failure reason */}
          {"status" in diagnostics.fit_status &&
            diagnostics.fit_status.status !== "success" &&
            "reason" in diagnostics.fit_status && (
              <p className="text-[11px] font-mono text-destructive/70 leading-relaxed">
                {diagnostics.fit_status.reason}
              </p>
            )}

          {/* Polynomial */}
          {diagnostics.fit_coefficients && (
            <div className="border-t border-border/30 pt-3">
              <PolyCoeffTable {...diagnostics.fit_coefficients} />
            </div>
          )}

          {/* Fit quality */}
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

          {/* Robustness */}
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

        {/* ── Provenance ── */}
        <TabsContent value="provenance" className="mt-3">
          <div className="space-y-1">
            {Object.entries(diagnostics.provenance).map(([stage, level]) => (
              <div key={stage} className="flex items-center justify-between py-0.5">
                <span className="text-[13px] text-muted-foreground capitalize">
                  {stage.replace(/_/g, " ")}
                </span>
                <div className="flex items-center gap-1.5">
                  <div
                    className={`w-1.5 h-1.5 rounded-full ${
                      provenanceDots[level as string] ?? "bg-muted-foreground"
                    }`}
                  />
                  <span
                    className={`text-xs font-mono ${
                      provenanceColors[level as string] ?? "text-muted-foreground"
                    }`}
                  >
                    {provenanceLabels[level as string] ?? level}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </TabsContent>

        {/* ── JSON ── */}
        <TabsContent value="json" className="mt-3">
          <pre className="text-xs font-mono bg-background p-3 rounded-sm border border-border/30 overflow-auto max-h-[400px] text-muted-foreground leading-relaxed">
            {JSON.stringify(diagnostics, null, 2)}
          </pre>
        </TabsContent>
      </Tabs>
    </div>
  );
}
