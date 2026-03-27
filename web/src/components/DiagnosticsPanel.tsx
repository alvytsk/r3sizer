import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useProcessorStore } from "@/stores/processor-store";
import { StatusIndicators } from "./StatusIndicators";
import { ProbeChart } from "./ProbeChart";
import { TimingBar } from "./TimingBar";

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex justify-between text-xs">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono">{value}</span>
    </div>
  );
}

const provenanceColors: Record<string, string> = {
  paper_confirmed: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200",
  paper_supported: "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200",
  engineering_choice: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200",
  engineering_proxy: "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-200",
  placeholder: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200",
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
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Diagnostics</CardTitle>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="summary" className="w-full">
          <TabsList className="grid grid-cols-5 w-full h-8">
            <TabsTrigger value="summary" className="text-xs">
              Summary
            </TabsTrigger>
            <TabsTrigger value="fit" className="text-xs">
              Fit
            </TabsTrigger>
            <TabsTrigger value="timing" className="text-xs">
              Timing
            </TabsTrigger>
            <TabsTrigger value="provenance" className="text-xs">
              Provenance
            </TabsTrigger>
            <TabsTrigger value="json" className="text-xs">
              JSON
            </TabsTrigger>
          </TabsList>

          <TabsContent value="summary" className="space-y-3 mt-3">
            <StatusIndicators diagnostics={diagnostics} />
            <div className="space-y-1">
              <Stat
                label="Selected strength"
                value={diagnostics.selected_strength.toFixed(4)}
              />
              <Stat
                label="Target artifact ratio"
                value={diagnostics.target_artifact_ratio.toExponential(2)}
              />
              <Stat
                label="Measured artifact ratio"
                value={diagnostics.measured_artifact_ratio.toExponential(3)}
              />
              <Stat
                label="Baseline artifact ratio"
                value={diagnostics.baseline_artifact_ratio.toExponential(3)}
              />
              <Stat
                label="Input size"
                value={`${diagnostics.input_size.width} x ${diagnostics.input_size.height}`}
              />
              <Stat
                label="Output size"
                value={`${diagnostics.output_size.width} x ${diagnostics.output_size.height}`}
              />
            </div>
            <ProbeChart diagnostics={diagnostics} />
          </TabsContent>

          <TabsContent value="fit" className="space-y-3 mt-3">
            <div className="space-y-1">
              <Stat
                label="Fit status"
                value={
                  diagnostics.fit_status?.status ?? "unknown"
                }
              />
              <Stat
                label="Crossing status"
                value={diagnostics.crossing_status}
              />
            </div>

            {diagnostics.fit_quality && (
              <div className="space-y-1">
                <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                  Fit Quality
                </p>
                <Stat
                  label="R²"
                  value={diagnostics.fit_quality.r_squared.toFixed(6)}
                />
                <Stat
                  label="Max residual"
                  value={diagnostics.fit_quality.max_residual.toExponential(3)}
                />
                <Stat
                  label="Min pivot"
                  value={diagnostics.fit_quality.min_pivot.toExponential(3)}
                />
                <Stat
                  label="RSS"
                  value={diagnostics.fit_quality.residual_sum_of_squares.toExponential(
                    3
                  )}
                />
              </div>
            )}

            {diagnostics.fit_coefficients && (
              <div className="space-y-1">
                <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                  Coefficients
                </p>
                <Stat
                  label="a (x³)"
                  value={diagnostics.fit_coefficients.a.toExponential(4)}
                />
                <Stat
                  label="b (x²)"
                  value={diagnostics.fit_coefficients.b.toExponential(4)}
                />
                <Stat
                  label="c (x)"
                  value={diagnostics.fit_coefficients.c.toExponential(4)}
                />
                <Stat
                  label="d"
                  value={diagnostics.fit_coefficients.d.toExponential(4)}
                />
              </div>
            )}

            {diagnostics.robustness && (
              <div className="space-y-1">
                <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                  Robustness
                </p>
                <div className="flex flex-wrap gap-1">
                  {(
                    [
                      ["Monotonic", diagnostics.robustness.monotonic],
                      ["Quasi-monotonic", diagnostics.robustness.quasi_monotonic],
                      ["R² OK", diagnostics.robustness.r_squared_ok],
                      ["Well-conditioned", diagnostics.robustness.well_conditioned],
                      ["LOO stable", diagnostics.robustness.loo_stable],
                    ] as const
                  ).map(([label, ok]) => (
                    <Badge
                      key={label}
                      variant={ok ? "default" : "destructive"}
                      className="text-[10px]"
                    >
                      {label}
                    </Badge>
                  ))}
                </div>
                <Stat
                  label="Max LOO root change"
                  value={diagnostics.robustness.max_loo_root_change.toFixed(4)}
                />
              </div>
            )}
          </TabsContent>

          <TabsContent value="timing" className="mt-3">
            <TimingBar timing={diagnostics.timing} />
          </TabsContent>

          <TabsContent value="provenance" className="mt-3">
            <div className="space-y-2">
              {Object.entries(diagnostics.provenance).map(([stage, level]) => (
                <div key={stage} className="flex items-center justify-between">
                  <span className="text-xs capitalize">
                    {stage.replace(/_/g, " ")}
                  </span>
                  <Badge
                    variant="outline"
                    className={`text-[10px] ${provenanceColors[level as string] ?? ""}`}
                  >
                    {provenanceLabels[level as string] ?? level}
                  </Badge>
                </div>
              ))}
            </div>
          </TabsContent>

          <TabsContent value="json" className="mt-3">
            <pre className="text-[10px] font-mono bg-muted p-3 rounded overflow-auto max-h-[400px]">
              {JSON.stringify(diagnostics, null, 2)}
            </pre>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
}
