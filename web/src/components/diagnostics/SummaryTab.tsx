import { useTranslation } from "react-i18next";
import type {
  AutoSharpDiagnostics,
  RegionCoverage,
  AdaptiveValidationOutcome,
} from "@/types/wasm-types";
import { StatusIndicators } from "../StatusIndicators";
import { ProbeChart } from "../ProbeChart";
import { Readout } from "./shared";
import { buildDiagnosis, severityStyle } from "./utils";

const REGION_KEYS: [keyof RegionCoverage, keyof RegionCoverage, string][] = [
  ["flat", "flat_fraction", "diagnostics.flat"],
  ["textured", "textured_fraction", "diagnostics.textured"],
  ["strong_edge", "strong_edge_fraction", "diagnostics.strongEdge"],
  ["microtexture", "microtexture_fraction", "diagnostics.microtexture"],
  ["risky_halo_zone", "risky_halo_zone_fraction", "diagnostics.riskyHalo"],
];

const REGION_COLORS = [
  "bg-chart-4",
  "bg-chart-2",
  "bg-chart-1",
  "bg-chart-3",
  "bg-chart-5",
];

const COMPONENT_LABELS_KEYS: Record<string, string> = {
  gamut_excursion: "diagnostics.components.gamutExcursion",
  halo_ringing: "diagnostics.components.haloRinging",
  edge_overshoot: "diagnostics.components.edgeOvershoot",
  texture_flattening: "diagnostics.components.textureFlattening",
};

function RegionCoverageBar({ coverage }: { coverage: RegionCoverage }) {
  const { t } = useTranslation();

  return (
    <div className="space-y-1.5">
      <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50">
        {t("diagnostics.regionCoverage")}
      </div>
      <div className="flex h-2 rounded-[2px] overflow-hidden bg-background border border-border/20">
        {REGION_KEYS.map(([, fracKey], i) => {
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
        {REGION_KEYS.map(([countKey, fracKey, labelKey], i) => {
          const frac = coverage[fracKey] as number;
          const count = coverage[countKey] as number;
          return (
            <div key={countKey} className="flex items-center gap-2 text-[11px]">
              <div className={`w-1.5 h-1.5 rounded-[1px] shrink-0 ${REGION_COLORS[i]}`} />
              <span className="text-muted-foreground flex-1">{t(labelKey)}</span>
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
  const { t } = useTranslation();
  const isPassed =
    outcome.outcome === "passed_direct" || outcome.outcome === "passed_after_backoff";
  const borderColor = isPassed ? "border-chart-3/25" : "border-destructive/30";
  const bgColor = isPassed ? "bg-chart-3/5" : "bg-destructive/5";
  const dotColor = isPassed ? "bg-chart-3" : "bg-destructive";
  const headlineColor = isPassed ? "text-chart-3" : "text-destructive";

  let headline: string;
  let detail: string;
  if (outcome.outcome === "passed_direct") {
    headline = t("diagnostics.adaptivePassedDirect");
    detail = t("diagnostics.noBackoff", { value: outcome.measured_metric.toExponential(3) });
  } else if (outcome.outcome === "passed_after_backoff") {
    headline = t("diagnostics.adaptivePassedBackoff", { count: outcome.iterations });
    detail = t("diagnostics.finalScale", { scale: outcome.final_scale.toFixed(3), value: outcome.measured_metric.toExponential(3) });
  } else {
    headline = t("diagnostics.adaptiveBudgetExceeded", { count: outcome.iterations });
    detail = t("diagnostics.bestScale", { scale: outcome.best_scale.toFixed(3), value: outcome.best_metric.toExponential(3) });
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
  const { t } = useTranslation();
  const entries = buildDiagnosis(diagnostics, t);
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
  const { t } = useTranslation();

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
          label={t("diagnostics.selectedStrength")}
          value={diagnostics.selected_strength.toFixed(4)}
        />
        <Readout
          label={<>{t("diagnostics.targetP0")}</>}
          value={diagnostics.target_artifact_ratio.toExponential(2)}
        />
        <Readout
          label={t("diagnostics.measuredP")}
          value={diagnostics.measured_artifact_ratio.toExponential(3)}
        />
        <Readout
          label={t("diagnostics.baselineP")}
          value={diagnostics.baseline_artifact_ratio.toExponential(3)}
        />

        {diagnostics.metric_components && (
          <div className="mt-2 pt-2 border-t border-border/20 space-y-0.5">
            <div className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/50 mb-1">
              {t("diagnostics.metricBreakdown")}
            </div>
            {Object.entries(diagnostics.metric_components.components).map(
              ([name, value]) => (
                <div key={name} className="flex justify-between text-[12px] py-px">
                  <span className="text-muted-foreground/70">{t(COMPONENT_LABELS_KEYS[name] ?? name)}</span>
                  <span className="font-mono text-foreground/80">{(value as number).toExponential(2)}</span>
                </div>
              )
            )}
            <div className="flex justify-between text-[12px] pt-1 border-t border-border/10">
              <span className="text-muted-foreground/50 italic">{t("diagnostics.composite")}</span>
              <span className="font-mono text-muted-foreground/60">
                {diagnostics.metric_components.composite_score.toExponential(2)}
              </span>
            </div>
          </div>
        )}

        <Readout
          label={t("diagnostics.input")}
          value={`${diagnostics.input_size.width}\u00d7${diagnostics.input_size.height}`}
        />
        <Readout
          label={t("diagnostics.output")}
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
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">{t("diagnostics.ingress")}</div>
              <Readout label={t("diagnostics.colorSpace")} value={diagnostics.input_ingress.declared_color_space} />
              {diagnostics.input_ingress.raw_value_min != null && (
                <Readout label={t("diagnostics.rawRange")} value={`${diagnostics.input_ingress.raw_value_min.toFixed(3)} – ${diagnostics.input_ingress.raw_value_max?.toFixed(3) ?? "?"}`} />
              )}
              {diagnostics.input_ingress.normalization_scale != null && (
                <Readout label={t("diagnostics.normScale")} value={diagnostics.input_ingress.normalization_scale.toFixed(4)} />
              )}
              {diagnostics.input_ingress.out_of_range_fraction != null && (
                <Readout label={t("diagnostics.outOfRange")} value={`${(diagnostics.input_ingress.out_of_range_fraction * 100).toFixed(2)}%`} />
              )}
            </div>
          )}

          {diagnostics.resize_strategy_diagnostics && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">{t("diagnostics.resizeStrategy")}</div>
              <Readout label={t("diagnostics.kernelsUsed")} value={diagnostics.resize_strategy_diagnostics.kernels_used.join(", ")} />
              {Object.entries(diagnostics.resize_strategy_diagnostics.per_kernel_pixel_count).map(
                ([kernel, count]) => (
                  <Readout key={kernel} label={kernel} value={String(count)} />
                )
              )}
            </div>
          )}

          {diagnostics.chroma_guard && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">{t("diagnostics.chromaGuard")}</div>
              <Readout label={t("diagnostics.pixelsClamped")} value={`${(diagnostics.chroma_guard.pixels_clamped_fraction * 100).toFixed(2)}%`} />
              <Readout label={t("diagnostics.meanShift")} value={diagnostics.chroma_guard.mean_chroma_shift.toFixed(4)} />
              <Readout label={t("diagnostics.maxShift")} value={diagnostics.chroma_guard.max_chroma_shift.toFixed(4)} />
            </div>
          )}

          {diagnostics.evaluator_result && (
            <div className="space-y-0.5 bg-muted/20 rounded-md p-2">
              <div className="text-[11px] font-mono font-semibold text-muted-foreground">{t("diagnostics.qualityEvaluator")}</div>
              <Readout label={t("diagnostics.qualityScore")} value={diagnostics.evaluator_result.predicted_quality_score.toFixed(3)} />
              <Readout label={t("diagnostics.confidence")} value={diagnostics.evaluator_result.confidence.toFixed(3)} />
              {diagnostics.evaluator_result.suggested_strength != null && (
                <Readout label={t("diagnostics.suggestedS")} value={diagnostics.evaluator_result.suggested_strength.toFixed(4)} />
              )}
              <details className="mt-1">
                <summary className="text-[10px] font-mono text-muted-foreground/50 cursor-pointer hover:text-primary transition-colors">
                  {t("diagnostics.features")}
                </summary>
                <div className="pt-1 space-y-0.5">
                  <Readout label={t("diagnostics.edgeDensity")} value={diagnostics.evaluator_result.features.edge_density.toExponential(2)} />
                  <Readout label={t("diagnostics.meanGradient")} value={diagnostics.evaluator_result.features.mean_gradient_magnitude.toExponential(2)} />
                  <Readout label={t("diagnostics.gradientVar")} value={diagnostics.evaluator_result.features.gradient_variance.toExponential(2)} />
                  <Readout label={t("diagnostics.meanLocalVar")} value={diagnostics.evaluator_result.features.mean_local_variance.toExponential(2)} />
                  <Readout label={t("diagnostics.localVarVar")} value={diagnostics.evaluator_result.features.local_variance_variance.toExponential(2)} />
                  <Readout label={t("diagnostics.laplacianVar")} value={diagnostics.evaluator_result.features.laplacian_variance.toExponential(2)} />
                  <Readout label={t("diagnostics.lumaEntropy")} value={diagnostics.evaluator_result.features.luminance_histogram_entropy.toFixed(3)} />
                </div>
              </details>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
