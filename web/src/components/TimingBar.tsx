import type { StageTiming } from "@/types/wasm-types";

const STAGES: { key: keyof StageTiming; label: string; color: string }[] = [
  { key: "resize_us", label: "Resize", color: "bg-blue-500" },
  { key: "contrast_us", label: "Contrast", color: "bg-purple-500" },
  { key: "baseline_us", label: "Baseline", color: "bg-green-500" },
  { key: "probing_us", label: "Probing", color: "bg-yellow-500" },
  { key: "fit_us", label: "Fit", color: "bg-orange-500" },
  { key: "robustness_us", label: "Robustness", color: "bg-red-500" },
  { key: "final_sharpen_us", label: "Final Sharpen", color: "bg-pink-500" },
  { key: "clamp_us", label: "Clamp", color: "bg-indigo-500" },
];

function formatUs(us: number): string {
  if (us >= 1_000_000) return `${(us / 1_000_000).toFixed(1)}s`;
  if (us >= 1_000) return `${(us / 1_000).toFixed(1)}ms`;
  return `${us}us`;
}

export function TimingBar({ timing }: { timing: StageTiming }) {
  const total = timing.total_us || 1;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>Pipeline timing</span>
        <span className="font-mono">{formatUs(timing.total_us)} total</span>
      </div>
      <div className="flex h-4 rounded overflow-hidden">
        {STAGES.map(({ key, label, color }) => {
          const pct = (timing[key] / total) * 100;
          if (pct < 0.5) return null;
          return (
            <div
              key={key}
              className={`${color} relative group`}
              style={{ width: `${pct}%` }}
              title={`${label}: ${formatUs(timing[key])}`}
            />
          );
        })}
      </div>
      <div className="grid grid-cols-4 gap-1 text-[10px]">
        {STAGES.map(({ key, label, color }) => (
          <div key={key} className="flex items-center gap-1">
            <div className={`w-2 h-2 rounded-sm ${color}`} />
            <span className="text-muted-foreground">
              {label}: {formatUs(timing[key])}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
