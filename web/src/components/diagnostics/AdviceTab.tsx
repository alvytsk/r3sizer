import type {
  AutoSharpDiagnostics,
  Recommendation,
  RecommendationKind,
  Severity as RecSeverity,
} from "@/types/wasm-types";
import { useProcessorStore } from "@/stores/processor-store";

interface Advice {
  icon: string;
  title: string;
  body: string;
  kind: "success" | "tip" | "warning";
}

function buildAdvice(d: AutoSharpDiagnostics): Advice[] {
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
      title: "Optimal result",
      body: "The polynomial solve found an analytical root. Sharpening strength is well-calibrated for this image.",
      kind: "success",
    });
  }

  if (d.selection_mode === "budget_unreachable") {
    advice.push({
      icon: "!",
      title: "Budget unreachable",
      body: "All probe strengths exceeded the artifact budget. Try increasing Target P(s) (e.g. from 1e-3 to 1e-2), reducing output resolution, or switching to Lightness sharpening mode.",
      kind: "warning",
    });
  } else if (ratio > target * 2) {
    advice.push({
      icon: "!",
      title: "Measured artifacts exceed target",
      body: `Measured P is ${(ratio / target).toFixed(1)}x the target. Consider raising Target P(s) or lowering sigma to reduce sharpening intensity.`,
      kind: "warning",
    });
  }

  if (strength < 0.02 && d.selection_mode !== "budget_unreachable") {
    advice.push({
      icon: "\u2193",
      title: "Very low sharpening applied",
      body: "Strength is below 0.02 — the image may appear soft. You can raise Target P(s) to allow more sharpening, or this image may simply not need much.",
      kind: "tip",
    });
  }

  const probeMax = d.probe_samples.length > 0
    ? Math.max(...d.probe_samples.map((p) => p.strength))
    : 0;
  if (strength > 0 && probeMax > 0 && strength >= probeMax * 0.95) {
    advice.push({
      icon: "\u2191",
      title: "Strength near probe limit",
      body: "Selected strength is at the upper edge of probe range. Consider adding higher probe values (e.g. 0.7, 1.0) so the solver has more room to find an optimal point.",
      kind: "tip",
    });
  }

  if (d.baseline_artifact_ratio > target * 0.5 && d.baseline_artifact_ratio > 0) {
    advice.push({
      icon: "\u26A0",
      title: "High baseline artifacts",
      body: `The resize step alone produces ${(d.baseline_artifact_ratio * 100).toFixed(2)}% artifacts before any sharpening. This content may be too detailed for the target resolution. Try a larger output size or a smoother resize kernel (Gaussian).`,
      kind: "warning",
    });
  }

  if (d.sharpen_mode === "rgb" && !recKinds.has("switch_to_lightness")) {
    advice.push({
      icon: "\u2192",
      title: "Consider Lightness mode",
      body: "RGB mode sharpens all color channels independently, which can amplify color fringing. Lightness mode only sharpens luminance — it typically produces fewer color artifacts.",
      kind: "tip",
    });
  }

  if (d.fit_quality && d.fit_quality.r_squared < 0.85 && !recKinds.has("widen_probe_range")) {
    advice.push({
      icon: "\u223C",
      title: "Poor polynomial fit",
      body: `R\u00b2 = ${d.fit_quality.r_squared.toFixed(3)} is below 0.85. The cubic model doesn't closely match probe data. Try adding more probe points or widening the probe range for a better fit.`,
      kind: "warning",
    });
  }

  if (d.robustness && !d.robustness.loo_stable) {
    advice.push({
      icon: "\u2248",
      title: "Result is noise-sensitive",
      body: "Leave-one-out analysis shows the selected strength shifts significantly when any single probe is removed. Adding more probe samples will stabilize the result.",
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
        title: "High halo-risk content",
        body: `${(rc.risky_halo_zone_fraction * 100).toFixed(0)}% of the image is in the halo risk zone (strong edges next to flat areas). Consider Content Adaptive strategy with reduced strong_edge gain, or lower sigma.`,
        kind: "tip",
      });
    }
    if (rc.flat_fraction > 0.7) {
      advice.push({
        icon: "\u2014",
        title: "Mostly flat image",
        body: "Over 70% of the image is flat regions. Sharpening has little to enhance — the result should be clean. If you see noise amplification, reduce sigma.",
        kind: "tip",
      });
    }
  }

  if (advice.length === 0 && (!d.recommendations || d.recommendations.length === 0)) {
    advice.push({
      icon: "\u2713",
      title: "Looking good",
      body: "No issues detected. The current settings appear well-suited for this image.",
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

const REC_KIND_LABELS: Record<RecommendationKind, string> = {
  switch_to_content_adaptive: "Content-adaptive sharpening recommended",
  lower_strong_edge_gain: "Reduce strong-edge gain",
  raise_artifact_budget: "Raise artifact budget",
  switch_to_lightness: "Switch to lightness mode",
  widen_probe_range: "Widen probe range",
  lower_sigma: "Lower blur sigma",
  switch_to_hybrid: "Switch to hybrid selection policy",
};

function RecommendationCards({ recommendations }: { recommendations: Recommendation[] }) {
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
                  {REC_KIND_LABELS[rec.kind] ?? rec.kind}
                </div>
                <p className="text-[12px] text-muted-foreground leading-relaxed">
                  {rec.reason}
                </p>
                <button
                  type="button"
                  className="text-[11px] font-mono font-medium text-primary hover:text-primary/80 transition-colors mt-0.5"
                  onClick={() => applyPatch(rec)}
                >
                  Apply
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
          Apply all recommendations
        </button>
      )}
    </>
  );
}

export function AdviceTab({ diagnostics }: { diagnostics: AutoSharpDiagnostics }) {
  return (
    <div className="space-y-2 mt-3">
      {buildAdvice(diagnostics).map((item, i) => {
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
