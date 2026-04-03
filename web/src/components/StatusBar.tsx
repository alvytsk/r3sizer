import { Activity } from "lucide-react";
import type { AutoSharpDiagnostics } from "@/types/wasm-types";

export function StatusBar({ diagnostics }: { diagnostics: AutoSharpDiagnostics | null }) {
  return (
    <footer className="footer-separator border-t border-border/40 px-5 flex items-center gap-5 bg-background/90 backdrop-blur-sm flex-shrink-0 h-9">
      {diagnostics ? (
        <>
          <span className="flex items-center gap-1.5">
            <span className={`led ${diagnostics.selection_mode === "polynomial_root" ? "led-green" : "led-amber"}`} />
            <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">S*</span>
            <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.selected_strength.toFixed(4)}</span>
          </span>
          <span className="w-px h-3 bg-border/30 flex-shrink-0" />
          <span className="flex items-center gap-1.5">
            <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">P</span>
            <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.measured_artifact_ratio.toExponential(2)}</span>
          </span>
          <span className="w-px h-3 bg-border/30 flex-shrink-0" />
          <span className="flex items-center gap-1.5">
            <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Out</span>
            <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.output_size.width}&times;{diagnostics.output_size.height}</span>
          </span>
          <span className="ml-auto flex items-center gap-1.5">
            <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Total</span>
            <span className="text-[11px] font-mono text-primary tabular-nums">{(diagnostics.timing.total_us / 1000).toFixed(0)}ms</span>
          </span>
        </>
      ) : (
        <span className="flex items-center gap-2">
          <Activity className="h-3 w-3 text-muted-foreground/30" />
          <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/30">ready</span>
        </span>
      )}
    </footer>
  );
}
