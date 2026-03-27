import { Badge } from "@/components/ui/badge";
import type { AutoSharpDiagnostics } from "@/types/wasm-types";

const selectionColors: Record<string, string> = {
  polynomial_root:
    "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200",
  best_sample_within_budget:
    "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200",
  least_bad_sample:
    "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200",
  budget_unreachable:
    "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200",
};

const selectionLabels: Record<string, string> = {
  polynomial_root: "Polynomial Root",
  best_sample_within_budget: "Best Sample",
  least_bad_sample: "Least Bad",
  budget_unreachable: "Unreachable",
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
  return (
    <div className="flex flex-wrap gap-2">
      <Badge
        variant="outline"
        className={selectionColors[diagnostics.selection_mode] ?? ""}
      >
        {selectionLabels[diagnostics.selection_mode] ??
          diagnostics.selection_mode}
      </Badge>

      <Badge variant={diagnostics.budget_reachable ? "default" : "destructive"}>
        {diagnostics.budget_reachable ? "Budget OK" : "Budget Unreachable"}
      </Badge>

      {diagnostics.fallback_reason && (
        <Badge variant="secondary">
          {fallbackLabels[diagnostics.fallback_reason] ??
            diagnostics.fallback_reason}
        </Badge>
      )}
    </div>
  );
}
