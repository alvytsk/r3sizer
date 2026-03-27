import type { StageTiming } from "@/types/wasm-types";

const STAGES: { key: keyof StageTiming; label: string; color: string; barColor: string }[] = [
  { key: "resize_us", label: "Resize", color: "bg-chart-2", barColor: "bg-chart-2/80" },
  { key: "contrast_us", label: "Contrast", color: "bg-chart-4", barColor: "bg-chart-4/80" },
  { key: "baseline_us", label: "Baseline", color: "bg-chart-3", barColor: "bg-chart-3/80" },
  { key: "probing_us", label: "Probing", color: "bg-chart-1", barColor: "bg-chart-1/80" },
  { key: "fit_us", label: "Fit", color: "bg-amber-dim", barColor: "bg-amber-dim/80" },
  { key: "robustness_us", label: "Robust.", color: "bg-chart-5", barColor: "bg-chart-5/80" },
  { key: "final_sharpen_us", label: "Sharpen", color: "bg-amber-bright", barColor: "bg-amber-bright/80" },
  { key: "clamp_us", label: "Clamp", color: "bg-surface-raised", barColor: "bg-surface-raised/80" },
];

function formatUs(us: number): string {
  if (us >= 1_000_000) return `${(us / 1_000_000).toFixed(1)}s`;
  if (us >= 1_000) return `${(us / 1_000).toFixed(1)}ms`;
  return `${us}\u00b5s`;
}

export function TimingBar({ timing }: { timing: StageTiming }) {
  const total = timing.total_us || 1;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between text-xs font-mono">
        <span className="uppercase tracking-[0.15em] text-primary/70">Pipeline</span>
        <span className="text-foreground/80">{formatUs(timing.total_us)}</span>
      </div>

      {/* Stacked bar */}
      <div className="flex h-2.5 rounded-sm overflow-hidden bg-background border border-border/30">
        {STAGES.map(({ key, label, barColor }) => {
          const pct = (timing[key] / total) * 100;
          if (pct < 0.5) return null;
          return (
            <div
              key={key}
              className={`${barColor} transition-all duration-300`}
              style={{ width: `${pct}%` }}
              title={`${label}: ${formatUs(timing[key])}`}
            />
          );
        })}
      </div>

      {/* Legend grid */}
      <div className="grid grid-cols-4 gap-x-2 gap-y-0.5">
        {STAGES.map(({ key, label, color }) => (
          <div key={key} className="flex items-center gap-1">
            <div className={`w-1.5 h-1.5 rounded-[1px] ${color}`} />
            <span className="text-[11px] font-mono text-muted-foreground truncate">
              {label}
            </span>
            <span className="text-[11px] font-mono text-foreground/60 ml-auto">
              {formatUs(timing[key])}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
