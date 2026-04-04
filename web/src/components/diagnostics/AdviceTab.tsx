import { useTranslation } from "react-i18next";
import type {
  AutoSharpDiagnostics,
  Recommendation,
  RecommendationKind,
  Severity as RecSeverity,
} from "@/types/wasm-types";
import { useProcessorStore } from "@/stores/processor-store";
import type { TFunction } from "i18next";

interface Advice {
  icon: string;
  title: string;
  body: string;
  kind: "success" | "tip" | "warning";
}

function buildAdvice(d: AutoSharpDiagnostics, t: TFunction): Advice[] {
  const advice: Advice[] = [];
  const ratio = d.measured_artifact_ratio;
  const target = d.target_artifact_ratio;
  const strength = d.selected_strength;

  const recKinds = new Set<RecommendationKind>(
    (d.recommendations ?? []).map((r) => r.kind)
  );

  if (d.selection_mode === "polynomial_root" && ratio <= target * 1.1) {
    advice.push({
      icon: "\u2713",
      title: t("advice.optimalResult"),
      body: t("advice.optimalResultBody"),
      kind: "success",
    });
  }

  if (d.selection_mode === "budget_unreachable") {
    advice.push({
      icon: "!",
      title: t("advice.budgetUnreachable"),
      body: t("advice.budgetUnreachableBody"),
      kind: "warning",
    });
  } else if (ratio > target * 2) {
    advice.push({
      icon: "!",
      title: t("advice.artifactsExceedTarget"),
      body: t("advice.artifactsExceedTargetBody", { ratio: (ratio / target).toFixed(1) }),
      kind: "warning",
    });
  }

  if (strength < 0.02 && d.selection_mode !== "budget_unreachable") {
    advice.push({
      icon: "\u2193",
      title: t("advice.veryLowSharpening"),
      body: t("advice.veryLowSharpeningBody"),
      kind: "tip",
    });
  }

  const probeMax = d.probe_samples.length > 0
    ? Math.max(...d.probe_samples.map((p) => p.strength))
    : 0;
  if (strength > 0 && probeMax > 0 && strength >= probeMax * 0.95) {
    advice.push({
      icon: "\u2191",
      title: t("advice.nearProbeLimit"),
      body: t("advice.nearProbeLimitBody"),
      kind: "tip",
    });
  }

  if (d.baseline_artifact_ratio > target * 0.5 && d.baseline_artifact_ratio > 0) {
    advice.push({
      icon: "\u26A0",
      title: t("advice.highBaseline"),
      body: t("advice.highBaselineBody", { value: (d.baseline_artifact_ratio * 100).toFixed(2) }),
      kind: "warning",
    });
  }

  if (d.sharpen_mode === "rgb" && !recKinds.has("switch_to_lightness")) {
    advice.push({
      icon: "\u2192",
      title: t("advice.considerLightness"),
      body: t("advice.considerLightnessBody"),
      kind: "tip",
    });
  }

  if (d.fit_quality && d.fit_quality.r_squared < 0.85 && !recKinds.has("widen_probe_range")) {
    advice.push({
      icon: "\u223C",
      title: t("advice.poorFit"),
      body: t("advice.poorFitBody", { value: d.fit_quality.r_squared.toFixed(3) }),
      kind: "warning",
    });
  }

  if (d.robustness && !d.robustness.loo_stable) {
    advice.push({
      icon: "\u2248",
      title: t("advice.noiseSensitive"),
      body: t("advice.noiseSensitiveBody"),
      kind: "tip",
    });
  }

  if (d.region_coverage) {
    const rc = d.region_coverage;
    if (rc.risky_halo_zone_fraction > 0.15
      && !recKinds.has("switch_to_content_adaptive")
      && !recKinds.has("lower_strong_edge_gain")) {
      advice.push({
        icon: "\u25CB",
        title: t("advice.highHaloRisk"),
        body: t("advice.highHaloRiskBody", { value: (rc.risky_halo_zone_fraction * 100).toFixed(0) }),
        kind: "tip",
      });
    }
    if (rc.flat_fraction > 0.7) {
      advice.push({
        icon: "\u2014",
        title: t("advice.mostlyFlat"),
        body: t("advice.mostlyFlatBody"),
        kind: "tip",
      });
    }
  }

  if (advice.length === 0 && (!d.recommendations || d.recommendations.length === 0)) {
    advice.push({
      icon: "\u2713",
      title: t("advice.lookingGood"),
      body: t("advice.lookingGoodBody"),
      kind: "success",
    });
  }

  return advice;
}

const ADVICE_STYLES: Record<Advice["kind"], { border: string; bg: string; icon: string; title: string }> = {
  success: {
    border: "border-chart-3/25",
    bg: "bg-chart-3/5",
    icon: "text-chart-3",
    title: "text-chart-3",
  },
  tip: {
    border: "border-chart-2/25",
    bg: "bg-chart-2/5",
    icon: "text-chart-2",
    title: "text-chart-2",
  },
  warning: {
    border: "border-primary/25",
    bg: "bg-primary/5",
    icon: "text-primary",
    title: "text-primary",
  },
};

const REC_SEVERITY_STYLES: Record<RecSeverity, { border: string; bg: string; title: string }> = {
  warning: {
    border: "border-primary/25",
    bg: "bg-primary/5",
    title: "text-primary",
  },
  suggestion: {
    border: "border-chart-2/25",
    bg: "bg-chart-2/5",
    title: "text-chart-2",
  },
  info: {
    border: "border-muted-foreground/15",
    bg: "bg-muted/5",
    title: "text-muted-foreground",
  },
};

const REC_KIND_KEYS: Record<RecommendationKind, string> = {
  switch_to_content_adaptive: "recommendations.switchToContentAdaptive",
  lower_strong_edge_gain: "recommendations.lowerStrongEdgeGain",
  raise_artifact_budget: "recommendations.raiseArtifactBudget",
  switch_to_lightness: "recommendations.switchToLightness",
  widen_probe_range: "recommendations.widenProbeRange",
  lower_sigma: "recommendations.lowerSigma",
  switch_to_hybrid: "recommendations.switchToHybrid",
};

function RecommendationCards({ recommendations }: { recommendations: Recommendation[] }) {
  const { t } = useTranslation();
  const updateParams = useProcessorStore((s) => s.updateParams);

  if (recommendations.length === 0) return null;

  const applyPatch = (rec: Recommendation) => {
    const p = rec.patch;
    updateParams({
      ...(p.sharpen_strategy != null && { sharpen_strategy: p.sharpen_strategy }),
      ...(p.target_artifact_ratio != null && { target_artifact_ratio: p.target_artifact_ratio }),
      ...(p.sharpen_mode != null && { sharpen_mode: p.sharpen_mode }),
      ...(p.probe_strengths != null && { probe_strengths: p.probe_strengths }),
      ...(p.sharpen_sigma != null && { sharpen_sigma: p.sharpen_sigma }),
    });
  };

  const applyAll = () => {
    for (const rec of recommendations) {
      applyPatch(rec);
    }
  };

  return (
    <>
      {recommendations.map((rec, i) => {
        const s = REC_SEVERITY_STYLES[rec.severity];
        return (
          <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2.5`}>
            <div className="flex items-start gap-2">
              <span className={`text-[14px] font-mono leading-none mt-0.5 shrink-0 ${s.title}`}>
                {"\u2606"}
              </span>
              <div className="space-y-1 min-w-0 flex-1">
                <div className={`text-[12px] font-mono font-medium ${s.title}`}>
                  {t(REC_KIND_KEYS[rec.kind] ?? rec.kind)}
                </div>
                <p className="text-[12px] text-muted-foreground leading-relaxed">
                  {rec.reason}
                </p>
                <button
                  type="button"
                  className="text-[11px] font-mono font-medium text-primary hover:text-primary/80 transition-colors mt-0.5"
                  onClick={() => applyPatch(rec)}
                >
                  {t("advice.apply")}
                </button>
              </div>
            </div>
          </div>
        );
      })}
      {recommendations.length > 1 && (
        <button
          type="button"
          className="w-full text-[11px] font-mono font-medium text-primary/70 hover:text-primary border border-primary/20 hover:border-primary/40 hover:bg-primary/5 rounded-md transition-colors text-center py-1.5"
          onClick={applyAll}
        >
          {t("advice.applyAll")}
        </button>
      )}
    </>
  );
}

export function AdviceTab({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2 mt-3">
      {buildAdvice(diagnostics, t).map((item, i) => {
        const s = ADVICE_STYLES[item.kind];
        return (
          <div key={i} className={`rounded-sm border ${s.border} ${s.bg} px-3 py-2.5`}>
            <div className="flex items-start gap-2">
              <span className={`text-[14px] font-mono leading-none mt-0.5 shrink-0 ${s.icon}`}>
                {item.icon}
              </span>
              <div className="space-y-1 min-w-0">
                <div className={`text-[12px] font-mono font-medium ${s.title}`}>
                  {item.title}
                </div>
                <p className="text-[12px] text-muted-foreground leading-relaxed">
                  {item.body}
                </p>
              </div>
            </div>
          </div>
        );
      })}
      <RecommendationCards recommendations={diagnostics.recommendations ?? []} />
    </div>
  );
}
