import type { AutoSharpDiagnostics } from "@/types/wasm-types";

const selectionStyles: Record<string, { dot: string; text: string }> = {
  polynomial_root: { dot: "bg-chart-3", text: "text-chart-3" },
  best_sample_within_budget: { dot: "bg-primary", text: "text-primary" },
  least_bad_sample: { dot: "bg-chart-5", text: "text-chart-5" },
  budget_unreachable: { dot: "bg-destructive", text: "text-destructive" },
};

const selectionLabels: Record<string, string> = {
  polynomial_root: "Polynomial Root",
  best_sample_within_budget: "Best Sample",
  least_bad_sample: "Least Bad",
  budget_unreachable: "Unreachable",
};

const policyLabels: Record<string, string> = {
  gamut_only: "Gamut Only",
  hybrid: "Hybrid",
  composite_only: "Composite Only",
};

const fallbackLabels: Record<string, string> = {
  fit_failed: "Fit Failed",
  fit_unstable: "Fit Unstable",
  root_out_of_range: "Root Out of Range",
  metric_non_monotonic: "Non-Monotonic",
  budget_too_strict_for_content: "Budget Too Strict",
  direct_search_configured: "Direct Search",
};

export function StatusIndicators({
  diagnostics,
}: {
  diagnostics: AutoSharpDiagnostics;
}) {
  const style = selectionStyles[diagnostics.selection_mode] ?? {
    dot: "bg-muted-foreground",
    text: "text-muted-foreground",
  };

  return (
    <div className="flex flex-wrap items-center gap-3">
      {/* Selection mode */}
      <div className="flex items-center gap-1.5">
        <div className={`w-2 h-2 rounded-full ${style.dot}`} />
        <span className={`text-[13px] font-mono font-medium ${style.text}`}>
          {selectionLabels[diagnostics.selection_mode] ?? diagnostics.selection_mode}
        </span>
      </div>

      {/* Budget status */}
      <div className="flex items-center gap-1.5">
        <div className={`w-2 h-2 rounded-full ${diagnostics.budget_reachable ? "bg-chart-3" : "bg-destructive"}`} />
        <span className={`text-[13px] font-mono ${diagnostics.budget_reachable ? "text-chart-3" : "text-destructive"}`}>
          {diagnostics.budget_reachable ? "Budget OK" : "Unreachable"}
        </span>
      </div>

      {/* Selection policy (shown when non-default) */}
      {diagnostics.selection_policy && diagnostics.selection_policy !== "gamut_only" && (
        <div className="flex items-center gap-1.5">
          <div className="w-2 h-2 rounded-full bg-chart-4" />
          <span className="text-[13px] font-mono text-chart-4">
            {policyLabels[diagnostics.selection_policy] ?? diagnostics.selection_policy}
          </span>
        </div>
      )}

      {/* Fallback reason */}
      {diagnostics.fallback_reason && (
        <div className="flex items-center gap-1.5">
          <div className="w-2 h-2 rounded-full bg-primary/60" />
          <span className="text-[13px] font-mono text-primary/80">
            {fallbackLabels[diagnostics.fallback_reason] ?? diagnostics.fallback_reason}
          </span>
        </div>
      )}
    </div>
  );
}
