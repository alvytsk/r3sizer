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

// Darkroom palette
const AMBER = "oklch(0.78 0.16 75)";
const BLUE = "oklch(0.65 0.14 230)";
const RED = "oklch(0.6 0.2 25)";
const GRID = "oklch(0.30 0.01 270)";
const TEXT_DIM = "oklch(0.5 0.01 80)";

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

  const curveData: { strength: number; fitted: number }[] = [];
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
    <div className="rounded-sm border border-border/30 bg-background p-2 pt-3">
      <div className="flex items-baseline justify-between px-2 mb-2">
        <span className="text-[10px] font-mono uppercase tracking-[0.15em] text-primary/70">
          P(s) Probe Curve
        </span>
      </div>
      <ResponsiveContainer width="100%" height={260}>
        <ComposedChart margin={{ top: 5, right: 15, bottom: 20, left: 5 }}>
          <CartesianGrid stroke={GRID} strokeDasharray="2 4" />
          <XAxis
            dataKey="strength"
            type="number"
            name="Strength"
            label={{
              value: "Sharpening Strength (s)",
              position: "bottom",
              offset: 5,
              style: { fontSize: 10, fontFamily: "JetBrains Mono Variable, monospace", fill: TEXT_DIM },
            }}
            tick={{ fontSize: 9, fontFamily: "JetBrains Mono Variable, monospace", fill: TEXT_DIM }}
            stroke={GRID}
          />
          <YAxis
            type="number"
            name="Metric"
            label={{
              value: "Metric P(s)",
              angle: -90,
              position: "insideLeft",
              offset: 10,
              style: { fontSize: 10, fontFamily: "JetBrains Mono Variable, monospace", fill: TEXT_DIM },
            }}
            tick={{ fontSize: 9, fontFamily: "JetBrains Mono Variable, monospace", fill: TEXT_DIM }}
            stroke={GRID}
          />
          <Tooltip
            formatter={(value) => Number(value).toExponential(3)}
            labelFormatter={(label) => `s = ${Number(label).toFixed(3)}`}
            contentStyle={{
              background: "oklch(0.22 0.006 270)",
              border: "1px solid oklch(0.28 0.01 270)",
              borderRadius: "4px",
              fontSize: "10px",
              fontFamily: "JetBrains Mono Variable, monospace",
              color: "oklch(0.88 0.01 80)",
            }}
          />

          <ReferenceLine
            y={diagnostics.target_artifact_ratio}
            stroke={RED}
            strokeDasharray="4 4"
            strokeWidth={1}
            label={{
              value: `P\u2080 = ${diagnostics.target_artifact_ratio.toExponential(1)}`,
              position: "right",
              style: { fontSize: 9, fontFamily: "JetBrains Mono Variable, monospace", fill: RED },
            }}
          />

          {diagnostics.selected_strength > 0 && (
            <ReferenceLine
              x={diagnostics.selected_strength}
              stroke={AMBER}
              strokeDasharray="4 4"
              strokeWidth={1}
              label={{
                value: `s* = ${diagnostics.selected_strength.toFixed(3)}`,
                position: "top",
                style: { fontSize: 9, fontFamily: "JetBrains Mono Variable, monospace", fill: AMBER },
              }}
            />
          )}

          {curveData.length > 0 && (
            <Line
              data={curveData}
              dataKey="fitted"
              stroke={BLUE}
              strokeWidth={1.5}
              dot={false}
              name="Fitted cubic"
              type="monotone"
            />
          )}

          <Scatter
            data={withinBudget}
            dataKey="metric_value"
            fill={BLUE}
            name="Within budget"
            r={4}
          />
          <Scatter
            data={overBudget}
            dataKey="metric_value"
            fill={RED}
            name="Over budget"
            r={4}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
