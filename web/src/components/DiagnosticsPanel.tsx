import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useProcessorStore } from "@/stores/processor-store";
import { StatusIndicators } from "./StatusIndicators";
import { ProbeChart } from "./ProbeChart";
import { TimingBar } from "./TimingBar";

function Readout({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex justify-between text-[13px] py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono text-foreground/90">{value}</span>
    </div>
  );
}

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

        <TabsContent value="summary" className="space-y-3 mt-3">
          <StatusIndicators diagnostics={diagnostics} />
          <div className="space-y-0.5 border-t border-border/30 pt-2">
            <Readout
              label="Selected strength"
              value={diagnostics.selected_strength.toFixed(4)}
            />
            <Readout
              label="Target P"
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

        <TabsContent value="fit" className="space-y-3 mt-3">
          <div className="space-y-0.5">
            <Readout
              label="Fit status"
              value={diagnostics.fit_status?.status ?? "unknown"}
            />
            <Readout
              label="Crossing status"
              value={diagnostics.crossing_status}
            />
          </div>

          {diagnostics.fit_quality && (
            <div className="space-y-0.5 border-t border-border/30 pt-2">
              <div className="text-xs font-mono uppercase tracking-[0.15em] text-primary/70 mb-1">
                Fit Quality
              </div>
              <Readout
                label="R\u00b2"
                value={diagnostics.fit_quality.r_squared.toFixed(6)}
              />
              <Readout
                label="Max residual"
                value={diagnostics.fit_quality.max_residual.toExponential(3)}
              />
              <Readout
                label="Min pivot"
                value={diagnostics.fit_quality.min_pivot.toExponential(3)}
              />
              <Readout
                label="RSS"
                value={diagnostics.fit_quality.residual_sum_of_squares.toExponential(3)}
              />
            </div>
          )}

          {diagnostics.fit_coefficients && (
            <div className="space-y-0.5 border-t border-border/30 pt-2">
              <div className="text-xs font-mono uppercase tracking-[0.15em] text-primary/70 mb-1">
                Coefficients
              </div>
              <Readout
                label="a (x\u00b3)"
                value={diagnostics.fit_coefficients.a.toExponential(4)}
              />
              <Readout
                label="b (x\u00b2)"
                value={diagnostics.fit_coefficients.b.toExponential(4)}
              />
              <Readout
                label="c (x)"
                value={diagnostics.fit_coefficients.c.toExponential(4)}
              />
              <Readout
                label="d"
                value={diagnostics.fit_coefficients.d.toExponential(4)}
              />
            </div>
          )}

          {diagnostics.robustness && (
            <div className="space-y-1.5 border-t border-border/30 pt-2">
              <div className="text-xs font-mono uppercase tracking-[0.15em] text-primary/70 mb-1">
                Robustness
              </div>
              <div className="grid grid-cols-2 gap-x-3 gap-y-0.5">
                {(
                  [
                    ["Monotonic", diagnostics.robustness.monotonic],
                    ["Quasi-mono", diagnostics.robustness.quasi_monotonic],
                    ["R\u00b2 OK", diagnostics.robustness.r_squared_ok],
                    ["Well-cond.", diagnostics.robustness.well_conditioned],
                    ["LOO stable", diagnostics.robustness.loo_stable],
                  ] as const
                ).map(([label, ok]) => (
                  <div key={label} className="flex items-center gap-1.5 text-[13px]">
                    <div className={`w-1.5 h-1.5 rounded-full ${ok ? "bg-chart-3" : "bg-destructive"}`} />
                    <span className={ok ? "text-foreground/70" : "text-destructive/80"}>
                      {label}
                    </span>
                  </div>
                ))}
              </div>
              <Readout
                label="Max LOO \u0394"
                value={diagnostics.robustness.max_loo_root_change.toFixed(4)}
              />
            </div>
          )}
        </TabsContent>

        <TabsContent value="timing" className="mt-3">
          <TimingBar timing={diagnostics.timing} />
        </TabsContent>

        <TabsContent value="provenance" className="mt-3">
          <div className="space-y-1">
            {Object.entries(diagnostics.provenance).map(([stage, level]) => (
              <div key={stage} className="flex items-center justify-between py-0.5">
                <span className="text-[13px] text-muted-foreground capitalize">
                  {stage.replace(/_/g, " ")}
                </span>
                <div className="flex items-center gap-1.5">
                  <div className={`w-1.5 h-1.5 rounded-full ${provenanceDots[level as string] ?? "bg-muted-foreground"}`} />
                  <span className={`text-xs font-mono ${provenanceColors[level as string] ?? "text-muted-foreground"}`}>
                    {provenanceLabels[level as string] ?? level}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </TabsContent>

        <TabsContent value="json" className="mt-3">
          <pre className="text-xs font-mono bg-background p-3 rounded-sm border border-border/30 overflow-auto max-h-[400px] text-muted-foreground leading-relaxed">
            {JSON.stringify(diagnostics, null, 2)}
          </pre>
        </TabsContent>
      </Tabs>
    </div>
  );
}
