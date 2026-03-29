import type { StageTiming } from "@/types/wasm-types";

interface StageEntry {
  key: string;
  label: string;
  color: string;
  hint: string;
}

const STAGES: StageEntry[] = [
  { key: "ingress_us", label: "Ingress", color: "bg-chart-4/80", hint: "Input color-space conversion" },
  { key: "resize_us", label: "Resize", color: "bg-chart-2", hint: "Lanczos3 downscale" },
  { key: "contrast_us", label: "Contrast", color: "bg-chart-4", hint: "Percentile stretch (stub)" },
  { key: "classification_us", label: "Classify", color: "bg-chart-5/80", hint: "Region classification (adaptive)" },
  { key: "baseline_us", label: "Baseline", color: "bg-chart-3", hint: "Pre-sharpen artifact measure" },
  { key: "probing_us", label: "Probing", color: "bg-chart-1", hint: "N-point probe sweep" },
  { key: "fit_us", label: "Fit", color: "bg-primary/70", hint: "Cubic Vandermonde solve" },
  { key: "robustness_us", label: "Robustness", color: "bg-chart-5", hint: "Monotonicity + LOO checks" },
  { key: "final_sharpen_us", label: "Sharpen", color: "bg-primary", hint: "Final sharpening at s*" },
  { key: "adaptive_validation_us", label: "Validate", color: "bg-chart-2/70", hint: "Adaptive validation + backoff" },
  { key: "evaluator_us", label: "Evaluator", color: "bg-chart-3/70", hint: "Quality evaluation (advisory)" },
  { key: "clamp_us", label: "Clamp", color: "bg-border", hint: "Output clamping to [0,1]" },
];

function getUs(timing: StageTiming, key: string): number | undefined {
  return (timing as unknown as Record<string, number | undefined>)[key];
}

function formatUs(us: number): string {
  if (us >= 1_000_000) return `${(us / 1_000_000).toFixed(2)}s`;
  if (us >= 1_000) return `${(us / 1_000).toFixed(1)}ms`;
  return `${us}\u00b5s`;
}

export function TimingBar({ timing }: { timing: StageTiming }) {
  const total = timing.total_us || 1;

  // Filter out optional stages that are absent (undefined/null)
  const activeStages = STAGES.filter((s) => getUs(timing, s.key) != null);

  const stages = activeStages.map((s) => {
    const us = getUs(timing, s.key) ?? 0;
    return { ...s, us, pct: (us / total) * 100 };
  }).sort((a, b) => b.us - a.us);

  const maxUs = stages[0]?.us ?? 1;
  const dominantKey = stages[0]?.key;

  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between">
        <span className="text-[10px] font-mono uppercase tracking-[0.15em] text-primary/70">
          Pipeline Timing
        </span>
        <span className="font-mono text-sm text-foreground/90">
          {formatUs(timing.total_us)}
        </span>
      </div>

      <div className="flex h-1.5 rounded-[2px] overflow-hidden bg-background border border-border/20">
        {activeStages.map(({ key, color }) => {
          const us = getUs(timing, key) ?? 0;
          const pct = (us / total) * 100;
          if (pct < 0.5) return null;
          return (
            <div
              key={key}
              className={`${color} opacity-70`}
              style={{ width: `${pct}%` }}
              title={`${key.replace("_us", "")}: ${formatUs(us)}`}
            />
          );
        })}
      </div>

      <div className="space-y-0.5">
        {stages.map(({ key, label, color, hint, us, pct }) => {
          const isDominant = key === dominantKey;
          const barWidth = maxUs > 0 ? (us / maxUs) * 100 : 0;
          return (
            <div
              key={key}
              className={`flex items-center gap-2 py-1 px-1.5 rounded-sm ${
                isDominant ? "bg-primary/5 border border-primary/10" : ""
              }`}
              title={hint}
            >
              <div className="flex items-center gap-1.5 w-[84px] shrink-0">
                <div className={`w-1.5 h-1.5 shrink-0 rounded-[1px] ${color}`} />
                <span
                  className={`text-[11px] font-mono truncate ${
                    isDominant ? "text-foreground/90" : "text-muted-foreground"
                  }`}
                >
                  {label}
                </span>
              </div>
              <div className="flex-1 h-2.5 rounded-[2px] bg-background border border-border/20 overflow-hidden">
                <div
                  className={`h-full ${color} transition-all duration-300 ${
                    us === 0 ? "opacity-15" : "opacity-75"
                  }`}
                  style={{ width: `${barWidth}%` }}
                />
              </div>
              <span
                className={`text-[11px] font-mono w-[52px] text-right shrink-0 tabular-nums ${
                  isDominant ? "text-foreground/90" : "text-foreground/55"
                }`}
              >
                {formatUs(us)}
              </span>
              <span
                className={`text-[10px] font-mono w-[32px] text-right shrink-0 tabular-nums ${
                  isDominant ? "text-primary/80" : "text-muted-foreground/50"
                }`}
              >
                {pct < 0.1 ? "<0.1%" : `${pct.toFixed(1)}%`}
              </span>
            </div>
          );
        })}
      </div>

      {stages[0] && stages[0].pct > 50 && (
        <p className="text-[11px] font-mono text-muted-foreground/55 border-t border-border/20 pt-2 leading-relaxed">
          <span className="text-primary/60">{stages[0].label}</span>
          {" "}dominates at {stages[0].pct.toFixed(0)}% —{" "}
          {stages[0].hint.toLowerCase()}.
        </p>
      )}
    </div>
  );
}
