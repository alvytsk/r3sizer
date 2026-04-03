import type { AutoSharpDiagnostics, RobustnessFlags } from "@/types/wasm-types";
import { StatusChip } from "./shared";
import type { ChipVariant } from "./utils";

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

export function FitTab({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  return (
    <div className="space-y-3 mt-3">
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
    </div>
  );
}
