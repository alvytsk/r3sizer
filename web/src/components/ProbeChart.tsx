import {
  Scatter,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  ResponsiveContainer,
  ComposedChart,
} from "recharts";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { AutoSharpDiagnostics } from "@/types/wasm-types";

function evaluateCubic(
  a: number,
  b: number,
  c: number,
  d: number,
  x: number
) {
  return a * x * x * x + b * x * x + c * x + d;
}

export function ProbeChart({
  diagnostics,
}: {
  diagnostics: AutoSharpDiagnostics;
}) {
  const probeData = diagnostics.probe_samples.map((s) => ({
    strength: s.strength,
    metric_value: s.metric_value,
    withinBudget: s.metric_value <= diagnostics.target_artifact_ratio,
  }));

  const withinBudget = probeData.filter((d) => d.withinBudget);
  const overBudget = probeData.filter((d) => !d.withinBudget);

  let curveData: { strength: number; fitted: number }[] = [];
  if (
    diagnostics.fit_coefficients &&
    diagnostics.fit_status &&
    "status" in diagnostics.fit_status &&
    diagnostics.fit_status.status === "success"
  ) {
    const { a, b, c, d } = diagnostics.fit_coefficients;
    const strengths = diagnostics.probe_samples.map((s) => s.strength);
    const minS = Math.min(...strengths);
    const maxS = Math.max(...strengths);
    const step = (maxS - minS) / 100;
    for (let s = minS; s <= maxS; s += step) {
      curveData.push({ strength: s, fitted: evaluateCubic(a, b, c, d, s) });
    }
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">
          P(s) Probe Curve
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={300}>
          <ComposedChart margin={{ top: 5, right: 20, bottom: 20, left: 10 }}>
            <CartesianGrid strokeDasharray="3 3" className="opacity-30" />
            <XAxis
              dataKey="strength"
              type="number"
              name="Strength"
              label={{
                value: "Sharpening Strength (s)",
                position: "bottom",
                offset: 5,
                style: { fontSize: 11 },
              }}
              tick={{ fontSize: 10 }}
            />
            <YAxis
              type="number"
              name="Metric"
              label={{
                value: "Metric P(s)",
                angle: -90,
                position: "insideLeft",
                offset: 10,
                style: { fontSize: 11 },
              }}
              tick={{ fontSize: 10 }}
            />
            <Tooltip
              formatter={(value) => Number(value).toExponential(3)}
              labelFormatter={(label) => `s = ${Number(label).toFixed(3)}`}
            />

            <ReferenceLine
              y={diagnostics.target_artifact_ratio}
              stroke="hsl(var(--destructive))"
              strokeDasharray="5 5"
              label={{
                value: `P₀ = ${diagnostics.target_artifact_ratio.toExponential(1)}`,
                position: "right",
                style: { fontSize: 10, fill: "hsl(var(--destructive))" },
              }}
            />

            {diagnostics.selected_strength > 0 && (
              <ReferenceLine
                x={diagnostics.selected_strength}
                stroke="hsl(var(--chart-1))"
                strokeDasharray="5 5"
                label={{
                  value: `s* = ${diagnostics.selected_strength.toFixed(3)}`,
                  position: "top",
                  style: { fontSize: 10, fill: "hsl(var(--chart-1))" },
                }}
              />
            )}

            {curveData.length > 0 && (
              <Line
                data={curveData}
                dataKey="fitted"
                stroke="hsl(var(--chart-2))"
                strokeWidth={2}
                dot={false}
                name="Fitted cubic"
                type="monotone"
              />
            )}

            <Scatter
              data={withinBudget}
              dataKey="metric_value"
              fill="hsl(var(--chart-2))"
              name="Within budget"
              r={5}
            />
            <Scatter
              data={overBudget}
              dataKey="metric_value"
              fill="hsl(var(--destructive))"
              name="Over budget"
              r={5}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  );
}
