import { Activity } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AutoSharpDiagnostics } from "@/shared/lib";

function Value({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return (
    <span className={`font-mono font-medium tabular-nums text-foreground ${className}`}>
      {children}
    </span>
  );
}

export function StatusBar({ diagnostics }: { diagnostics: AutoSharpDiagnostics | null }) {
  const { t } = useTranslation();

  return (
    <footer className="border-t border-border/40 px-4 flex items-center gap-3 bg-background flex-shrink-0 h-9 text-xs text-muted-foreground">
      {diagnostics ? (
        <>
          <span className="flex items-center gap-1.5">
            <span
              className={`led ${diagnostics.selection_mode === "polynomial_root" ? "led-green" : "led-amber"}`}
            />
            {t("status.sharpness")} <Value>{diagnostics.selected_strength.toFixed(2)}</Value>
          </span>
          <span aria-hidden>·</span>
          <span>
            {t("status.artifacts")}{" "}
            <Value>{(diagnostics.measured_artifact_ratio * 100).toFixed(2)}%</Value>
          </span>
          <span aria-hidden>·</span>
          <Value>
            {diagnostics.output_size.width}&times;{diagnostics.output_size.height}
          </Value>
          <Value className="ml-auto text-primary">
            {(diagnostics.timing.total_us / 1000).toFixed(0)} ms
          </Value>
        </>
      ) : (
        <span className="flex items-center gap-2">
          <Activity className="h-3 w-3" />
          {t("status.ready")}
        </span>
      )}
    </footer>
  );
}
