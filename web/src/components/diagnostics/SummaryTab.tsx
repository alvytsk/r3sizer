import type {
  AutoSharpDiagnostics,
  RegionCoverage,
  AdaptiveValidationOutcome,
} from "@/types/wasm-types";
import { StatusIndicators } from "../StatusIndicators";
import { ProbeChart } from "../ProbeChart";
import { Readout } from "./shared";
import { COMPONENT_LABELS, buildDiagnosis, severityStyle } from "./utils";

const REGION_LABELS: [keyof RegionCoverage, keyof RegionCoverage, string][] = [
  ["flat", "flat_fraction", "Flat"],
  ["textured", "textured_fraction", "Textured"],
  ["strong_edge", "strong_edge_fraction", "Strong Edge"],
  ["microtexture", "microtexture_fraction", "Microtexture"],
  ["risky_halo_zone", "risky_halo_zone_fraction", "Risky Halo"],
];

const REGION_COLORS = [
  "bg-chart-4",
  "bg-chart-2",
  "bg-chart-1",
  "bg-chart-3",
  "bg-chart-5",
];

function RegionCoverageBar({ coverage }: { coverage: RegionCoverage }) {
  return (
    <div className="space-y-1.5">
      <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50">
        Region Coverage
      </div>
      <div className="flex h-2 rounded-[2px] overflow-hidden bg-background border border-border/20">
        {REGION_LABELS.map(([, fracKey], i) => {
          const pct = (coverage[fracKey] as number) * 100;
          if (pct < 0.3) return null;
          return (
            <div
              key={fracKey}
              className={`${REGION_COLORS[i]} opacity-70`}
              style={{ width: `${pct}%` }}
            />
          );
        })}
      </div>
      <div className="space-y-0.5">
        {REGION_LABELS.map(([countKey, fracKey, label], i) => {
          const frac = coverage[fracKey] as number;
          const count = coverage[countKey] as number;
          return (
            <div key={countKey} className="flex items-center gap-2 text-[11px]">
              <div className={`w-1.5 h-1.5 rounded-[1px] shrink-0 ${REGION_COLORS[i]}`} />
              <span className="text-muted-foreground flex-1">{label}</span>
              <span className="font-mono tabular-nums text-foreground/70 w-[42px] text-right">
                {(frac * 100).toFixed(1)}%
              </span>
              <span className="font-mono tabular-nums text-muted-foreground/50 w-[52px] text-right">
                {count.toLocaleString()}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function AdaptiveValidationCard({ outcome }: { outcome: AdaptiveValidationOutcome }) {
  const isPassed =
    outcome.outcome === "passed_direct" || outcome.outcome === "passed_after_backoff";
  const borderColor = isPassed ? "border-chart-3/25" : "border-destructive/30";
  const bgColor = isPassed ? "bg-chart-3/5" : "bg-destructive/5";
  const dotColor = isPassed ? "bg-chart-3" : "bg-destructive";
  const headlineColor = isPassed ? "text-chart-3" : "text-destructive";

  let headline: string;
  let detail: string;
  if (outcome.outcome === "passed_direct") {
    headline = "Adaptive: passed direct";
    detail = `No backoff needed. Measured metric: ${outcome.measured_metric.toExponential(3)}`;
  } else if (outcome.outcome === "passed_after_backoff") {
    headline = `Adaptive: passed after ${outcome.iterations} backoff`;
    detail = `Final scale: ${outcome.final_scale.toFixed(3)}, measured metric: ${outcome.measured_metric.toExponential(3)}`;
  } else {
    headline = `Adaptive: budget exceeded (${outcome.iterations} iterations)`;
    detail = `Best scale: ${outcome.best_scale.toFixed(3)}, best metric: ${outcome.best_metric.toExponential(3)}`;
  }

  return (
    <div className={`rounded-sm border ${borderColor} ${bgColor} px-3 py-2`}>
      <div className="flex items-center gap-1.5 mb-0.5">
        <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${dotColor}`} />
        <span className={`text-[10px] font-mono font-medium uppercase tracking-[0.12em] ${headlineColor}`}>
          {headline}
        </span>
      </div>
      <p className="text-[12px] text-muted-foreground leading-relaxed pl-3">{detail}</p>
    </div>
  );
}

function DiagnosisCard({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  const entries = buildDiagnosis(diagnostics);
  if (entries.length === 0) return null;

  return (
    <div className="space-y-1.5">
      {entries.map((entry, i) => {
        const s = severityStyle[entry.severity];
        return (
          <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2`}>
            <div className="flex items-center gap-1.5 mb-0.5">
              <div className={`w-1.5 h-1.5 rounded-full shrink-0 ${s.dot}`} />
              <span
                className={`text-[10px] font-mono font-medium uppercase tracking-[0.12em] ${s.headline}`}
              >
                {entry.headline}
              </span>
            </div>
            <p className="text-[12px] text-muted-foreground leading-relaxed pl-3">
              {entry.detail}
            </p>
          </div>
        );
      })}
    </div>
  );
}

export function SummaryTab({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  return (
    <div className="space-y-3 mt-3">
      <StatusIndicators diagnostics={diagnostics} />
      <DiagnosisCard diagnostics={diagnostics} />
      {diagnostics.adaptive_validation && (
        <AdaptiveValidationCard outcome={diagnostics.adaptive_validation} />
      )}
      {diagnostics.region_coverage && (
        <RegionCoverageBar coverage={diagnostics.region_coverage} />
      )}
      <div className="space-y-0.5 border-t border-border/30 pt-2">
        <Readout
          label="Selected strength"
          value={diagnostics.selected_strength.toFixed(4)}
        />
        <Readout
          label={<>Target P<sub>0</sub></>}
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

        {diagnostics.metric_components && (
          <div className="mt-2 pt-2 border-t border-border/20 space-y-0.5">
            <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50 mb-1">
              Metric Breakdown
            </div>
            {Object.entries(diagnostics.metric_components.components).map(
              ([name, value]) => (
                <div key={name} className="flex justify-between text-[12px] py-px">
                  <span className="text-muted-foreground/70">{COMPONENT_LABELS[name] ?? name}</span>
                  <span className="font-mono text-foreground/80">{(value as number).toExponential(2)}</span>
                </div>
              )
            )}
            <div className="flex justify-between text-[12px] pt-1 border-t border-border/10">
              <span className="text-muted-foreground/50 italic">composite</span>
              <span className="font-mono text-muted-foreground/60">
                {diagnostics.metric_components.composite_score.toExponential(2)}
              </span>
            </div>
          </div>
        )}

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

      {/* Extended diagnostics */}
      {(diagnostics.input_ingress || diagnostics.resize_strategy_diagnostics ||
        diagnostics.chroma_guard || diagnostics.evaluator_result) && (
        <div className="space-y-2 border-t border-border/30 pt-2">
          {diagnostics.input_ingress && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">Ingress</div>
              <Readout label="Color space" value={diagnostics.input_ingress.declared_color_space} />
              {diagnostics.input_ingress.raw_value_min != null && (
                <Readout label="Raw range" value={`${diagnostics.input_ingress.raw_value_min.toFixed(3)} – ${diagnostics.input_ingress.raw_value_max?.toFixed(3) ?? "?"}`} />
              )}
              {diagnostics.input_ingress.normalization_scale != null && (
                <Readout label="Norm scale" value={diagnostics.input_ingress.normalization_scale.toFixed(4)} />
              )}
              {diagnostics.input_ingress.out_of_range_fraction != null && (
                <Readout label="Out of range" value={`${(diagnostics.input_ingress.out_of_range_fraction * 100).toFixed(2)}%`} />
              )}
            </div>
          )}

          {diagnostics.resize_strategy_diagnostics && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">Resize Strategy</div>
              <Readout label="Kernels used" value={diagnostics.resize_strategy_diagnostics.kernels_used.join(", ")} />
              {Object.entries(diagnostics.resize_strategy_diagnostics.per_kernel_pixel_count).map(
                ([kernel, count]) => (
                  <Readout key={kernel} label={kernel} value={String(count)} />
                )
              )}
            </div>
          )}

          {diagnostics.chroma_guard && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">Chroma Guard</div>
              <Readout label="Pixels clamped" value={`${(diagnostics.chroma_guard.pixels_clamped_fraction * 100).toFixed(2)}%`} />
              <Readout label="Mean shift" value={diagnostics.chroma_guard.mean_chroma_shift.toFixed(4)} />
              <Readout label="Max shift" value={diagnostics.chroma_guard.max_chroma_shift.toFixed(4)} />
            </div>
          )}

          {diagnostics.evaluator_result && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">Quality Evaluator</div>
              <Readout label="Quality score" value={diagnostics.evaluator_result.predicted_quality_score.toFixed(3)} />
              <Readout label="Confidence" value={diagnostics.evaluator_result.confidence.toFixed(3)} />
              {diagnostics.evaluator_result.suggested_strength != null && (
                <Readout label="Suggested s*" value={diagnostics.evaluator_result.suggested_strength.toFixed(4)} />
              )}
              <details className="mt-1">
                <summary className="text-[10px] font-mono text-muted-foreground/50 cursor-pointer hover:text-primary transition-colors">
                  Features
                </summary>
                <div className="pt-1 space-y-0.5">
                  <Readout label="Edge density" value={diagnostics.evaluator_result.features.edge_density.toExponential(2)} />
                  <Readout label="Mean gradient" value={diagnostics.evaluator_result.features.mean_gradient_magnitude.toExponential(2)} />
                  <Readout label="Gradient var" value={diagnostics.evaluator_result.features.gradient_variance.toExponential(2)} />
                  <Readout label="Mean local var" value={diagnostics.evaluator_result.features.mean_local_variance.toExponential(2)} />
                  <Readout label="Local var var" value={diagnostics.evaluator_result.features.local_variance_variance.toExponential(2)} />
                  <Readout label="Laplacian var" value={diagnostics.evaluator_result.features.laplacian_variance.toExponential(2)} />
                  <Readout label="Luma entropy" value={diagnostics.evaluator_result.features.luminance_histogram_entropy.toFixed(3)} />
                </div>
              </details>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
