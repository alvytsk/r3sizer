import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AutoSharpDiagnostics } from "@/shared/types/wasm-types";

/* ---------- math helpers ---------- */

function evaluateCubic(a: number, b: number, c: number, d: number, x: number) {
  return a * x * x * x + b * x * x + c * x + d;
}

/** ~5 "nice" tick values across [min, max]. */
function niceTicks(min: number, max: number, count = 5): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || min === max) return [min];
  const span = max - min;
  const rawStep = span / count;
  const mag = 10 ** Math.floor(Math.log10(rawStep));
  const norm = rawStep / mag;
  const niceStep = (norm >= 5 ? 5 : norm >= 2 ? 2 : 1) * mag;
  const start = Math.ceil(min / niceStep) * niceStep;
  const ticks: number[] = [];
  for (let v = start; v <= max + niceStep * 0.5; v += niceStep) {
    ticks.push(v);
  }
  return ticks;
}

/* ---------- darkroom palette (unchanged) ---------- */

const AMBER = "oklch(0.78 0.16 75)";
const BLUE = "oklch(0.65 0.14 230)";
const RED = "oklch(0.6 0.2 25)";
const GRID = "oklch(0.30 0.01 270)";
const TEXT_DIM = "oklch(0.5 0.01 80)";
const MONO = "JetBrains Mono Variable, monospace";

/* ---------- geometry ---------- */

const HEIGHT = 260;
const MARGIN = { top: 14, right: 16, bottom: 38, left: 54 };

export function ProbeChart({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(640);
  const [selStart, setSelStart] = useState<number | null>(null);
  const [selCurrent, setSelCurrent] = useState<number | null>(null);
  const [xZoom, setXZoom] = useState<[number, number] | null>(null);
  const [hover, setHover] = useState<{ sx: number; sy: number; s: number; v: number } | null>(null);

  /* measure container width (ResponsiveContainer replacement) */
  useEffect(() => {
    const el = wrapRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w) setWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const probeData = diagnostics.probe_samples;
  const allStrengths = probeData.map((s) => s.strength);
  const dataMinX = allStrengths.length ? Math.min(...allStrengths) : 0;
  const dataMaxX = allStrengths.length ? Math.max(...allStrengths) : 1;
  const xDomain: [number, number] = xZoom ?? [dataMinX, dataMaxX];

  const curveData = useMemo<{ s: number; fitted: number }[]>(() => {
    if (
      !diagnostics.fit_coefficients ||
      !("status" in diagnostics.fit_status) ||
      diagnostics.fit_status.status !== "success"
    )
      return [];
    const { a, b, c, d } = diagnostics.fit_coefficients;
    if (allStrengths.length === 0) return [];
    const minS = dataMinX;
    const maxS = dataMaxX;
    const step = (maxS - minS) / 200;
    const pts: { s: number; fitted: number }[] = [];
    for (let s = minS; s <= maxS + step * 0.01; s += step) {
      pts.push({ s, fitted: evaluateCubic(a, b, c, d, s) });
    }
    return pts;
  }, [diagnostics, allStrengths.length, dataMinX, dataMaxX]);

  const yDomain = useMemo<[number, number]>(() => {
    const [x0, x1] = xZoom ?? [dataMinX, dataMaxX];
    const ys = [
      ...probeData.filter((s) => s.strength >= x0 && s.strength <= x1).map((s) => s.metric_value),
      ...curveData.filter((d) => d.s >= x0 && d.s <= x1).map((d) => d.fitted),
      diagnostics.target_artifact_ratio,
    ].filter(Number.isFinite);
    if (ys.length === 0) return [0, 0.01];
    const mn = Math.min(...ys);
    const mx = Math.max(...ys);
    const pad = Math.max((mx - mn) * 0.18, mx * 0.05, 1e-7);
    return [Math.max(0, mn - pad), mx + pad];
  }, [probeData, curveData, xZoom, dataMinX, dataMaxX, diagnostics.target_artifact_ratio]);

  /* scales */
  const plotW = Math.max(10, width - MARGIN.left - MARGIN.right);
  const plotH = HEIGHT - MARGIN.top - MARGIN.bottom;
  const sx = (x: number) =>
    MARGIN.left + ((x - xDomain[0]) / (xDomain[1] - xDomain[0] || 1)) * plotW;
  const sy = (y: number) =>
    MARGIN.top + plotH - ((y - yDomain[0]) / (yDomain[1] - yDomain[0] || 1)) * plotH;
  const invX = (px: number) =>
    xDomain[0] + ((px - MARGIN.left) / plotW) * (xDomain[1] - xDomain[0]);

  const xTicks = niceTicks(xDomain[0], xDomain[1]);
  const yTicks = niceTicks(yDomain[0], yDomain[1]);

  /* fitted curve path (clipped to plot) */
  const curvePath = useMemo(() => {
    if (curveData.length === 0) return "";
    let d = "";
    for (let i = 0; i < curveData.length; i++) {
      const p = curveData[i];
      d += `${i === 0 ? "M" : "L"}${sx(p.s).toFixed(2)} ${sy(p.fitted).toFixed(2)} `;
    }
    return d.trim();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [curveData, sy, sx]);

  const withinBudget = probeData.filter((d) => d.metric_value <= diagnostics.target_artifact_ratio);
  const overBudget = probeData.filter((d) => d.metric_value > diagnostics.target_artifact_ratio);

  /* drag-to-zoom */
  const isSelecting = selStart != null && selCurrent != null;

  const onPointerDown = useCallback(
    (e: React.PointerEvent<SVGRectElement>) => {
      const rect = e.currentTarget.getBoundingClientRect();
      const px = e.clientX - rect.left;
      if (px < MARGIN.left || px > MARGIN.left + plotW) return;
      (e.target as Element).setPointerCapture?.(e.pointerId);
      setHover(null);
      setSelStart(invX(px));
      setSelCurrent(invX(px));
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [plotW, invX],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<SVGRectElement>) => {
      const rect = e.currentTarget.getBoundingClientRect();
      const px = e.clientX - rect.left;
      if (selStart != null) {
        setSelCurrent(invX(Math.max(MARGIN.left, Math.min(MARGIN.left + plotW, px))));
        return;
      }
      if (px < MARGIN.left || px > MARGIN.left + plotW) {
        setHover(null);
        return;
      }
      const target = invX(px);
      let best: { s: number; v: number; d: number } | null = null;
      for (const p of probeData) {
        const d = Math.abs(p.strength - target);
        if (!best || d < best.d) best = { s: p.strength, v: p.metric_value, d };
      }
      if (best) setHover({ sx: sx(best.s), sy: sy(best.v), s: best.s, v: best.v });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selStart, plotW, probeData, sx, sy, invX],
  );

  const endSelection = useCallback(() => {
    if (selStart != null && selCurrent != null && selStart !== selCurrent) {
      const [l, r] = selStart < selCurrent ? [selStart, selCurrent] : [selCurrent, selStart];
      if (r - l > (dataMaxX - dataMinX) * 0.02) setXZoom([l, r]);
    }
    setSelStart(null);
    setSelCurrent(null);
  }, [selStart, selCurrent, dataMinX, dataMaxX]);

  const selRect =
    isSelecting && selStart != null && selCurrent != null
      ? {
          x: Math.min(sx(selStart), sx(selCurrent)),
          w: Math.abs(sx(selCurrent) - sx(selStart)),
        }
      : null;

  const P0 = diagnostics.target_artifact_ratio;
  const sStar = diagnostics.selected_strength;

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
              setSelStart(null);
              setSelCurrent(null);
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
        ref={wrapRef}
        style={{
          userSelect: "none",
          cursor: isSelecting ? "crosshair" : "default",
          position: "relative",
        }}
      >
        <svg width={width} height={HEIGHT} role="img" aria-label="P(s) probe curve">
          <defs>
            <clipPath id="probe-plot-clip">
              <rect x={MARGIN.left} y={MARGIN.top} width={plotW} height={plotH} />
            </clipPath>
          </defs>

          {/* grid + Y ticks */}
          {yTicks.map((t, i) => {
            const y = sy(t);
            if (y < MARGIN.top - 0.5 || y > MARGIN.top + plotH + 0.5) return null;
            return (
              <g key={`y${i}`}>
                <line
                  x1={MARGIN.left}
                  y1={y}
                  x2={MARGIN.left + plotW}
                  y2={y}
                  stroke={GRID}
                  strokeDasharray="2 4"
                />
                <text
                  x={MARGIN.left - 6}
                  y={y}
                  dy="0.32em"
                  textAnchor="end"
                  fontSize={9}
                  fontFamily={MONO}
                  fill={TEXT_DIM}
                >
                  {t.toExponential(1)}
                </text>
              </g>
            );
          })}

          {/* X ticks */}
          {xTicks.map((t, i) => {
            const x = sx(t);
            if (x < MARGIN.left - 0.5 || x > MARGIN.left + plotW + 0.5) return null;
            return (
              <g key={`x${i}`}>
                <line
                  x1={x}
                  y1={MARGIN.top}
                  x2={x}
                  y2={MARGIN.top + plotH}
                  stroke={GRID}
                  strokeDasharray="2 4"
                />
                <line
                  x1={x}
                  y1={MARGIN.top + plotH}
                  x2={x}
                  y2={MARGIN.top + plotH + 4}
                  stroke={GRID}
                />
                <text
                  x={x}
                  y={MARGIN.top + plotH + 16}
                  textAnchor="middle"
                  fontSize={9}
                  fontFamily={MONO}
                  fill={TEXT_DIM}
                >
                  {t.toFixed(2)}
                </text>
              </g>
            );
          })}

          {/* plot frame */}
          <rect
            x={MARGIN.left}
            y={MARGIN.top}
            width={plotW}
            height={plotH}
            fill="none"
            stroke={GRID}
          />

          {/* fitted cubic */}
          {curvePath && (
            <path
              d={curvePath}
              fill="none"
              stroke={BLUE}
              strokeWidth={1.5}
              clipPath="url(#probe-plot-clip)"
            />
          )}

          {/* scatter */}
          <g clipPath="url(#probe-plot-clip)">
            {withinBudget.map((d, i) => (
              <circle key={`w${i}`} cx={sx(d.strength)} cy={sy(d.metric_value)} r={4} fill={BLUE} />
            ))}
            {overBudget.map((d, i) => (
              <circle key={`o${i}`} cx={sx(d.strength)} cy={sy(d.metric_value)} r={4} fill={RED} />
            ))}
          </g>

          {/* P0 reference (horizontal, red) */}
          {P0 >= yDomain[0] && P0 <= yDomain[1] && (
            <g>
              <line
                x1={MARGIN.left}
                y1={sy(P0)}
                x2={MARGIN.left + plotW}
                y2={sy(P0)}
                stroke={RED}
                strokeDasharray="4 4"
                strokeWidth={1}
              />
              <text
                x={MARGIN.left + plotW - 3}
                y={sy(P0) - 4}
                textAnchor="end"
                fontSize={9}
                fontFamily={MONO}
                fill={RED}
              >
                {`P\u2080 = ${P0.toExponential(1)}`}
              </text>
            </g>
          )}

          {/* s* reference (vertical, amber) */}
          {sStar > 0 && sStar >= xDomain[0] && sStar <= xDomain[1] && (
            <g>
              <line
                x1={sx(sStar)}
                y1={MARGIN.top}
                x2={sx(sStar)}
                y2={MARGIN.top + plotH}
                stroke={AMBER}
                strokeDasharray="4 4"
                strokeWidth={1}
              />
              <text
                x={sx(sStar) + 3}
                y={MARGIN.top + 10}
                fontSize={9}
                fontFamily={MONO}
                fill={AMBER}
              >
                {`s* = ${sStar.toFixed(3)}`}
              </text>
            </g>
          )}

          {/* drag selection */}
          {selRect && (
            <rect
              x={selRect.x}
              y={MARGIN.top}
              width={selRect.w}
              height={plotH}
              fill={AMBER}
              fillOpacity={0.07}
              stroke={AMBER}
              strokeOpacity={0.3}
              strokeWidth={1}
            />
          )}

          {/* hover marker */}
          {hover && !isSelecting && (
            <g pointerEvents="none">
              <line
                x1={hover.sx}
                y1={MARGIN.top}
                x2={hover.sx}
                y2={MARGIN.top + plotH}
                stroke={GRID}
              />
              <circle
                cx={hover.sx}
                cy={hover.sy}
                r={4.5}
                fill="none"
                stroke={BLUE}
                strokeWidth={1.2}
              />
            </g>
          )}

          {/* axis titles */}
          <text
            x={MARGIN.left + plotW / 2}
            y={HEIGHT - 4}
            textAnchor="middle"
            fontSize={10}
            fontFamily={MONO}
            fill={TEXT_DIM}
          >
            Sharpening Strength (s)
          </text>
          <text
            transform={`translate(13, ${MARGIN.top + plotH / 2}) rotate(-90)`}
            textAnchor="middle"
            fontSize={10}
            fontFamily={MONO}
            fill={TEXT_DIM}
          >
            Metric P(s)
          </text>

          {/* interaction overlay */}
          <rect
            x={MARGIN.left}
            y={MARGIN.top}
            width={plotW}
            height={plotH}
            fill="transparent"
            style={{ cursor: isSelecting ? "crosshair" : "default" }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={endSelection}
            onPointerLeave={() => {
              if (selStart != null) endSelection();
              setHover(null);
            }}
          />
        </svg>

        {/* tooltip */}
        {hover && !isSelecting && (
          <div
            style={{
              position: "absolute",
              left: Math.min(Math.max(hover.sx + 10, 4), width - 120),
              top: Math.max(hover.sy - 38, 4),
              background: "oklch(0.22 0.006 270)",
              border: "1px solid oklch(0.28 0.01 270)",
              borderRadius: 4,
              padding: "3px 6px",
              fontSize: 10,
              fontFamily: MONO,
              color: "oklch(0.88 0.01 80)",
              pointerEvents: "none",
              whiteSpace: "nowrap",
            }}
          >
            <div>{`s = ${hover.s.toFixed(3)}`}</div>
            <div>{`P = ${hover.v.toExponential(3)}`}</div>
          </div>
        )}
      </div>
    </div>
  );
}
