import { useState, useMemo } from "react";
import {
  Scatter,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  ReferenceArea,
  ResponsiveContainer,
  ComposedChart,
} from "recharts";
import type { AutoSharpDiagnostics } from "@/types/wasm-types";

function evaluateCubic(a: number, b: number, c: number, d: number, x: number) {
  return a * x * x * x + b * x * x + c * x + d;
}

// Darkroom palette
const AMBER = "oklch(0.78 0.16 75)";
const BLUE = "oklch(0.65 0.14 230)";
const RED = "oklch(0.6 0.2 25)";
const GRID = "oklch(0.30 0.01 270)";
const TEXT_DIM = "oklch(0.5 0.01 80)";

type ChartEvent = { activeLabel?: string | number } | null;

export function ProbeChart({
  diagnostics,
}: {
  diagnostics: AutoSharpDiagnostics;
}) {
  const [refAreaLeft, setRefAreaLeft] = useState<number | null>(null);
  const [refAreaRight, setRefAreaRight] = useState<number | null>(null);
  const [xZoom, setXZoom] = useState<[number, number] | null>(null);
  const [isSelecting, setIsSelecting] = useState(false);

  const probeData = diagnostics.probe_samples.map((s) => ({
    strength: s.strength,
    metric_value: s.metric_value,
    withinBudget: s.metric_value <= diagnostics.target_artifact_ratio,
  }));
  const withinBudget = probeData.filter((d) => d.withinBudget);
  const overBudget = probeData.filter((d) => !d.withinBudget);

  const allStrengths = diagnostics.probe_samples.map((s) => s.strength);
  const dataMinX = Math.min(...allStrengths);
  const dataMaxX = Math.max(...allStrengths);
  const xDomain: [number, number] = xZoom ?? [dataMinX, dataMaxX];

  // Full-range curve; Recharts clips to xDomain via allowDataOverflow
  const curveData = useMemo(() => {
    if (
      !diagnostics.fit_coefficients ||
      !("status" in diagnostics.fit_status) ||
      diagnostics.fit_status.status !== "success"
    )
      return [];
    const { a, b, c, d } = diagnostics.fit_coefficients;
    const strengths = diagnostics.probe_samples.map((s) => s.strength);
    const minS = Math.min(...strengths);
    const maxS = Math.max(...strengths);
    const step = (maxS - minS) / 200;
    const pts: { strength: number; fitted: number }[] = [];
    for (let s = minS; s <= maxS + step * 0.01; s += step) {
      pts.push({ strength: s, fitted: evaluateCubic(a, b, c, d, s) });
    }
    return pts;
  }, [diagnostics]);

  // Y domain clamped to visible X range so zooming also rescales Y
  const yDomain = useMemo((): [number, number] => {
    const [x0, x1] = xZoom ?? [dataMinX, dataMaxX];
    const ys = [
      ...diagnostics.probe_samples
        .filter((s) => s.strength >= x0 && s.strength <= x1)
        .map((s) => s.metric_value),
      ...curveData
        .filter((d) => d.strength >= x0 && d.strength <= x1)
        .map((d) => d.fitted),
      diagnostics.target_artifact_ratio,
    ].filter(isFinite);
    if (ys.length === 0) return [0, 0.01];
    const mn = Math.min(...ys);
    const mx = Math.max(...ys);
    const pad = Math.max((mx - mn) * 0.18, mx * 0.05, 1e-7);
    return [Math.max(0, mn - pad), mx + pad];
  }, [diagnostics, xZoom, curveData, dataMinX, dataMaxX]);

  const handleMouseDown = (e: ChartEvent) => {
    if (e?.activeLabel != null) {
      setIsSelecting(true);
      setRefAreaLeft(Number(e.activeLabel));
      setRefAreaRight(null);
    }
  };

  const handleMouseMove = (e: ChartEvent) => {
    if (isSelecting && e?.activeLabel != null) {
      setRefAreaRight(Number(e.activeLabel));
    }
  };

  const handleMouseUp = () => {
    if (
      refAreaLeft != null &&
      refAreaRight != null &&
      refAreaLeft !== refAreaRight
    ) {
      const [l, r] =
        refAreaLeft < refAreaRight
          ? [refAreaLeft, refAreaRight]
          : [refAreaRight, refAreaLeft];
      if (r - l > (dataMaxX - dataMinX) * 0.02) {
        setXZoom([l, r]);
      }
    }
    setIsSelecting(false);
    setRefAreaLeft(null);
    setRefAreaRight(null);
  };

  const handleMouseLeave = () => {
    if (isSelecting) {
      setIsSelecting(false);
      setRefAreaLeft(null);
      setRefAreaRight(null);
    }
  };

  return (
    <div className="rounded-sm border border-border/30 bg-background p-2 pt-3">
      <div className="flex items-baseline justify-between px-2 mb-2">
        <span className="text-[10px] font-mono uppercase tracking-[0.15em] text-primary/70">
          P(s) Probe Curve
        </span>
        {xZoom ? (
          <button
            onClick={() => {
              setXZoom(null);
              setRefAreaLeft(null);
              setRefAreaRight(null);
            }}
            className="text-[10px] font-mono text-primary/70 hover:text-primary transition-colors px-1.5 py-0.5 rounded border border-primary/20 hover:border-primary/40"
          >
            reset zoom
          </button>
        ) : (
          <span className="text-[10px] font-mono text-muted-foreground/40 italic">
            drag to zoom
          </span>
        )}
      </div>
      <div
        style={{ userSelect: "none", cursor: isSelecting ? "crosshair" : "default" }}
        onMouseLeave={handleMouseLeave}
      >
        <ResponsiveContainer width="100%" height={260}>
          <ComposedChart
            margin={{ top: 5, right: 15, bottom: 20, left: 5 }}
            onMouseDown={handleMouseDown}
            onMouseMove={handleMouseMove}
            onMouseUp={handleMouseUp}
          >
            <CartesianGrid stroke={GRID} strokeDasharray="2 4" />
            <XAxis
              dataKey="strength"
              type="number"
              name="Strength"
              domain={xDomain}
              allowDataOverflow
              label={{
                value: "Sharpening Strength (s)",
                position: "bottom",
                offset: 5,
                style: {
                  fontSize: 10,
                  fontFamily: "JetBrains Mono Variable, monospace",
                  fill: TEXT_DIM,
                },
              }}
              tick={{
                fontSize: 9,
                fontFamily: "JetBrains Mono Variable, monospace",
                fill: TEXT_DIM,
              }}
              tickFormatter={(v) => Number(v).toFixed(2)}
              stroke={GRID}
            />
            <YAxis
              type="number"
              name="Metric"
              domain={yDomain}
              allowDataOverflow
              label={{
                value: "Metric P(s)",
                angle: -90,
                position: "insideLeft",
                offset: 10,
                style: {
                  fontSize: 10,
                  fontFamily: "JetBrains Mono Variable, monospace",
                  fill: TEXT_DIM,
                },
              }}
              tick={{
                fontSize: 9,
                fontFamily: "JetBrains Mono Variable, monospace",
                fill: TEXT_DIM,
              }}
              tickFormatter={(v) => Number(v).toExponential(1)}
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
                style: {
                  fontSize: 9,
                  fontFamily: "JetBrains Mono Variable, monospace",
                  fill: RED,
                },
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
                  style: {
                    fontSize: 9,
                    fontFamily: "JetBrains Mono Variable, monospace",
                    fill: AMBER,
                  },
                }}
              />
            )}

            {refAreaLeft != null && refAreaRight != null && (
              <ReferenceArea
                x1={refAreaLeft}
                x2={refAreaRight}
                fill={AMBER}
                fillOpacity={0.07}
                stroke={AMBER}
                strokeOpacity={0.3}
                strokeWidth={1}
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
    </div>
  );
}
