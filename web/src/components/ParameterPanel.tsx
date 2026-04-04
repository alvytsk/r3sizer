import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ChevronDown, ArrowLeftRight } from "lucide-react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useTranslation } from "react-i18next";
import { useProcessorStore } from "@/stores/processor-store";
import { useThrottledUpdateParams, useDebouncedUpdateParams } from "@/hooks/useThrottledParams";
import type { MetricWeights } from "@/types/wasm-types";
import {
  DEFAULT_METRIC_WEIGHTS,
  DEFAULT_CONTENT_ADAPTIVE_STRATEGY,
  DEFAULT_CONTENT_ADAPTIVE_RESIZE_STRATEGY,
  DEFAULT_PARAMS,
  PIPELINE_PRESETS,
} from "@/types/wasm-types";

import { NumericInput } from "./params/NumericInput";
import { SectionLabel, ValueLabel, SelectedLabel } from "./params/helpers";
import { AdaptiveSettings } from "./params/AdaptiveSettings";
import {
  sliderValue,
  DIMENSION_PRESETS,
  ALL_PRESETS,
} from "./params/constants";

export function ParameterPanel() {
  const { t } = useTranslation();
  const params = useProcessorStore((s) => s.params);
  const updateParams = useProcessorStore((s) => s.updateParams);
  const throttledUpdate = useThrottledUpdateParams();
  const debouncedUpdate = useDebouncedUpdateParams();
  const preserveAspectRatio = useProcessorStore((s) => s.preserveAspectRatio);
  const setPreserveAspectRatio = useProcessorStore(
    (s) => s.setPreserveAspectRatio
  );
  const inputWidth = useProcessorStore((s) => s.inputWidth);
  const lockDimensions = useProcessorStore((s) => s.lockDimensions);
  const setLockDimensions = useProcessorStore((s) => s.setLockDimensions);

  const matchingPreset = ALL_PRESETS.find(
    (p) => p.w === params.target_width && p.h === params.target_height
  );
  const presetKey = matchingPreset ? `${matchingPreset.w}x${matchingPreset.h}` : "";

  const logRatio = Math.log10(params.target_artifact_ratio);
  const [activePreset, setActivePreset] = useState("photo");

  const SHARPEN_MODE: Record<string, string> = {
    lightness: t("params.lightness"),
    rgb: t("params.rgb"),
  };

  const METRIC_MODE: Record<string, string> = {
    relative_to_base: t("params.relative"),
    absolute_total: t("params.absolute"),
  };

  const ARTIFACT_METRIC: Record<string, string> = {
    channel_clipping_ratio: t("params.channelClipping"),
    pixel_out_of_gamut_ratio: t("params.pixelOog"),
  };

  const SELECTION_POLICY: Record<string, string> = {
    gamut_only: t("params.gamutOnly"),
    hybrid: t("params.hybrid"),
    composite_only: t("params.compositeOnly"),
  };

  const FIT_STRATEGY: Record<string, string> = {
    Cubic: t("params.cubic"),
    DirectSearch: t("params.directSearch"),
  };

  const CLAMP_POLICY: Record<string, string> = {
    Clamp: t("params.clamp"),
    Normalize: t("params.normalize"),
  };

  const SHARPEN_STRATEGY: Record<string, string> = {
    uniform: t("params.uniformStrategy"),
    content_adaptive: t("params.contentAdaptive"),
  };

  const RESIZE_KERNEL: Record<string, string> = {
    lanczos3: t("params.kernels.lanczos3"),
    mitchell_netravali: t("params.kernels.mitchellNetravali"),
    catmull_rom: t("params.kernels.catmullRom"),
    gaussian: t("params.kernels.gaussian"),
    content_adaptive: t("params.kernels.contentAdaptive"),
  };

  return (
    <TooltipProvider delay={100}>
    <div className="p-3 space-y-4">
      {/* Dimensions */}
      <div className="space-y-2">
        <SectionLabel>{t("params.dimensions")}</SectionLabel>
        <div>
          <ValueLabel>{t("params.preset")}</ValueLabel>
          <Select
            value={presetKey}
            onValueChange={(v) => {
              if (!v) return;
              const [w, h] = v.split("x").map(Number);
              if (w && h) updateParams({ target_width: w, target_height: h });
            }}
          >
            <SelectTrigger className="h-7 text-xs font-mono">
              <span className="flex flex-1 text-left truncate" data-slot="select-value">
                {matchingPreset
                  ? `${matchingPreset.label} — ${matchingPreset.detail}`
                  : <span className="text-muted-foreground">{t("params.selectPreset")}</span>
                }
              </span>
            </SelectTrigger>
            <SelectContent>
              {DIMENSION_PRESETS.map((group, gi) => (
                <SelectGroup key={group.group}>
                  {gi > 0 && <SelectSeparator />}
                  <SelectLabel>{t(`params.dimensionPresets.${group.group.toLowerCase()}` as const)}</SelectLabel>
                  {group.items.map((p) => (
                    <SelectItem key={`${p.w}x${p.h}`} value={`${p.w}x${p.h}`}>
                      <span className="flex items-center justify-between gap-3 w-full">
                        <span>{p.label}</span>
                        <span className="text-[11px] text-muted-foreground font-mono">{p.detail}</span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectGroup>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-end gap-1">
          <div className="flex-1 min-w-0">
            <ValueLabel>{t("params.width")}</ValueLabel>
            <NumericInput
              id="width"
              min={1}
              className="h-8 text-sm font-mono"
              value={params.target_width}
              onCommit={(v) => updateParams({ target_width: v })}
            />
          </div>
          <button
            type="button"
            className="shrink-0 h-8 w-8 flex items-center justify-center rounded-md border border-border/30 text-foreground/50 hover:text-primary hover:border-primary/30 hover:bg-primary/10 transition-colors"
            onClick={() =>
              updateParams({
                target_width: params.target_height,
                target_height: params.target_width,
              })
            }
            title={t("params.swapDimensions")}
          >
            <ArrowLeftRight className="h-3.5 w-3.5" />
          </button>
          <div className="flex-1 min-w-0">
            <ValueLabel>{t("params.height")}</ValueLabel>
            <NumericInput
              id="height"
              min={1}
              className="h-8 text-sm font-mono"
              value={params.target_height}
              onCommit={(v) => updateParams({ target_height: v })}
            />
          </div>
        </div>
        {inputWidth > 0 && (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              <Switch
                id="aspect"
                checked={preserveAspectRatio}
                onCheckedChange={setPreserveAspectRatio}
              />
              <Label htmlFor="aspect" className="text-[13px] text-muted-foreground">
                {t("params.lockRatio")}
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="pin-dims"
                checked={lockDimensions}
                onCheckedChange={setLockDimensions}
              />
              <Label htmlFor="pin-dims" className="text-[13px] text-foreground/70">
                {t("params.pinExact")}
              </Label>
            </div>
          </div>
        )}
      </div>

      {/* Pipeline preset */}
      <div className="space-y-2">
        <SectionLabel>{t("params.pipeline")}</SectionLabel>
        <div className="grid grid-cols-2 gap-1">
          {(["photo", "precision"] as const).map((key) => {
            const active = activePreset === key;
            const meta = key === "photo"
              ? { label: t("params.photo"), desc: t("params.photoDesc") }
              : { label: t("params.precision"), desc: t("params.precisionDesc") };
            return (
              <button
                key={key}
                type="button"
                className={[
                  "relative rounded-md px-2.5 py-2 text-left transition-all duration-150",
                  "border font-mono",
                  active
                    ? "border-primary/40 bg-primary/[0.08] ring-1 ring-primary/20"
                    : "border-border/30 bg-card/30 hover:border-border/50 hover:bg-card/60",
                ].join(" ")}
                onClick={() => {
                  const preset = PIPELINE_PRESETS[key];
                  if (preset) {
                    setActivePreset(key);
                    updateParams({
                      ...DEFAULT_PARAMS,
                      ...preset,
                      target_width: params.target_width,
                      target_height: params.target_height,
                      diagnostics_level: "full",
                    });
                  }
                }}
              >
                <span className={`text-xs font-semibold ${active ? "text-primary" : "text-foreground/70"}`}>
                  {meta.label}
                </span>
                <span className={`block text-[10px] mt-0.5 ${active ? "text-primary/70" : "text-muted-foreground/60"}`}>
                  {meta.desc}
                </span>
              </button>
            );
          })}
        </div>
        {/* Speed mode */}
        <div className="grid grid-cols-3 gap-1">
          {(["fast", "balanced", "quality"] as const).map((mode) => {
            const current = params.pipeline_mode ?? "balanced";
            const active = current === mode;
            const meta = {
              fast: { label: t("params.fast"), desc: t("params.fastDesc") },
              balanced: { label: t("params.balanced"), desc: t("params.balancedDesc") },
              quality: { label: t("params.quality"), desc: t("params.qualityDesc") },
            }[mode];
            return (
              <button
                key={mode}
                type="button"
                className={[
                  "rounded-md px-2 py-1.5 text-center transition-all duration-150",
                  "border font-mono",
                  active
                    ? "border-primary/40 bg-primary/[0.08] ring-1 ring-primary/20"
                    : "border-border/30 bg-card/30 hover:border-border/50 hover:bg-card/60",
                ].join(" ")}
                onClick={() => {
                  updateParams({
                    pipeline_mode: mode === "balanced" ? null : mode,
                  });
                }}
              >
                <span className={`text-[10px] font-semibold ${active ? "text-primary" : "text-foreground/70"}`}>
                  {meta.label}
                </span>
                <span className={`block text-[9px] ${active ? "text-primary/70" : "text-muted-foreground/60"}`}>
                  {meta.desc}
                </span>
              </button>
            );
          })}
        </div>
        {/* Active config summary */}
        <div className="rounded-md border border-border/20 bg-surface/50 px-2.5 py-2">
          <div className="flex items-baseline gap-2 mb-1.5">
            <span className="text-[10px] uppercase tracking-widest text-muted-foreground/60">P₀</span>
            <span className="text-base font-mono font-bold text-primary tabular-nums">
              {params.target_artifact_ratio.toExponential(0)}
            </span>
            <span className="text-[10px] text-muted-foreground/60">
              ({(params.target_artifact_ratio * 100).toFixed(1)}% {t("params.budget")})
            </span>
          </div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground/70 font-mono">
            <span>
              {"TwoPass" in params.probe_strengths
                ? `${params.probe_strengths.TwoPass.coarse_count}+${params.probe_strengths.TwoPass.dense_count} ${t("params.probes")}`
                : `${(params.probe_strengths as { Explicit: number[] }).Explicit.length} ${t("params.probes")}`}
            </span>
            <span className="text-border/60">|</span>
            <span>
              {params.sharpen_strategy.strategy === "content_adaptive" ? t("params.adaptive") : t("params.uniform")}
              {params.experimental_sharpen_mode ? ` + ${t("params.guard")}` : ""}
            </span>
            <span className="text-border/60">|</span>
            <span>{params.sharpen_mode}</span>
          </div>
        </div>
      </div>

      {/* ── Visual divider: setup ↑ / tuning ↓ ── */}
      <div className="h-px bg-gradient-to-r from-transparent via-border/40 to-transparent" />

      {/* Sharpening — only sigma and resize kernel at top level */}
      <div className="space-y-2">
        <SectionLabel>{t("params.sharpening")}</SectionLabel>
        <div>
          <div className="flex items-baseline justify-between">
            <ValueLabel tip={t("params.sigmaTip")}>{t("params.sigma")}</ValueLabel>
            <span className="text-xs font-mono text-primary">
              {params.sharpen_sigma.toFixed(1)}
            </span>
          </div>
          <Slider
            min={0.1}
            max={5.0}
            step={0.1}
            value={[params.sharpen_sigma]}
            onValueChange={(v) =>
              throttledUpdate({ sharpen_sigma: sliderValue(v) })
            }
          />
        </div>
        <div>
          <ValueLabel tip={t("params.resizeKernelTip")}>{t("params.resizeKernel")}</ValueLabel>
          <Select
            value={
              params.resize_strategy?.strategy === "content_adaptive"
                ? "content_adaptive"
                : params.resize_strategy?.strategy === "uniform"
                  ? (params.resize_strategy as { strategy: "uniform"; kernel: string }).kernel
                  : "lanczos3"
            }
            onValueChange={(v) => {
              if (v === "lanczos3") {
                updateParams({ resize_strategy: undefined });
              } else if (v === "content_adaptive") {
                updateParams({ resize_strategy: { ...DEFAULT_CONTENT_ADAPTIVE_RESIZE_STRATEGY } });
              } else {
                updateParams({ resize_strategy: { strategy: "uniform", kernel: v as "mitchell_netravali" | "catmull_rom" | "gaussian" } });
              }
            }}
          >
            <SelectTrigger className="h-8 text-sm font-mono">
              <SelectedLabel
                labels={RESIZE_KERNEL}
                value={
                  params.resize_strategy?.strategy === "content_adaptive"
                    ? "content_adaptive"
                    : params.resize_strategy?.strategy === "uniform"
                      ? (params.resize_strategy as { strategy: "uniform"; kernel: string }).kernel
                      : "lanczos3"
                }
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="lanczos3">{t("params.kernels.lanczos3")}</SelectItem>
              <SelectItem value="mitchell_netravali">{t("params.kernels.mitchellNetravali")}</SelectItem>
              <SelectItem value="catmull_rom">{t("params.kernels.catmullRom")}</SelectItem>
              <SelectItem value="gaussian">{t("params.kernels.gaussian")}</SelectItem>
              <SelectItem value="content_adaptive">{t("params.kernels.contentAdaptive")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Advanced — everything else collapsed */}
      <div className="space-y-2">
      <Collapsible>
        <CollapsibleTrigger className="group flex items-center gap-1.5 text-xs font-mono font-semibold uppercase tracking-[0.15em] text-muted-foreground/60 hover:text-primary transition-colors border-b border-border/20 pb-1 w-full">
          <div className="w-0.5 h-3 rounded-full bg-muted-foreground/20 group-hover:bg-primary/50 transition-colors" />
          {t("params.advanced")}
          <ChevronDown className="h-3 w-3 ml-auto transition-transform duration-200 group-data-[panel-open]:rotate-180" />
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-3 pt-3">

          {/* Target artifact ratio */}
          <div>
            <div className="flex items-baseline justify-between">
              <ValueLabel tip={t("params.targetPsTip")}>{t("params.targetPs")}</ValueLabel>
              <span className="text-xs font-mono text-primary">
                {params.target_artifact_ratio.toExponential(1)}
              </span>
            </div>
            <Slider
              min={-4}
              max={-1}
              step={0.1}
              value={[logRatio]}
              onValueChange={(v) =>
                throttledUpdate({ target_artifact_ratio: Math.pow(10, sliderValue(v)) })
              }
            />
          </div>

          {/* Sharpen mode & strategy */}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <ValueLabel tip={t("params.modeTip")}>{t("params.mode")}</ValueLabel>
              <Select
                value={params.sharpen_mode}
                onValueChange={(v) => {
                  if (!v) return;
                  updateParams({ sharpen_mode: v as typeof params.sharpen_mode });
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={SHARPEN_MODE} value={params.sharpen_mode} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="lightness">{t("params.lightness")}</SelectItem>
                  <SelectItem value="rgb">{t("params.rgb")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip={t("params.strategyTip")}>{t("params.strategy")}</ValueLabel>
              <Select
                value={params.sharpen_strategy.strategy}
                onValueChange={(v) => {
                  if (!v) return;
                  if (v === "uniform") {
                    updateParams({ sharpen_strategy: { strategy: "uniform" } });
                  } else {
                    updateParams({
                      sharpen_strategy: { ...DEFAULT_CONTENT_ADAPTIVE_STRATEGY },
                    });
                  }
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={SHARPEN_STRATEGY} value={params.sharpen_strategy.strategy} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="uniform">{t("params.uniformStrategy")}</SelectItem>
                  <SelectItem value="content_adaptive">{t("params.contentAdaptive")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {params.sharpen_strategy.strategy === "content_adaptive" && (
            <AdaptiveSettings
              strategy={params.sharpen_strategy}
              updateParams={throttledUpdate}
            />
          )}

          {/* Metric mode & artifact metric */}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <ValueLabel tip={t("params.metricModeTip")}>{t("params.metricMode")}</ValueLabel>
              <Select
                value={params.metric_mode}
                onValueChange={(v) => {
                  if (v) updateParams({ metric_mode: v as typeof params.metric_mode });
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={METRIC_MODE} value={params.metric_mode} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="relative_to_base">{t("params.relative")}</SelectItem>
                  <SelectItem value="absolute_total">{t("params.absolute")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip={t("params.artifactMetricTip")}>{t("params.artifactMetric")}</ValueLabel>
              <Select
                value={params.artifact_metric}
                onValueChange={(v) => {
                  if (v) updateParams({ artifact_metric: v as typeof params.artifact_metric });
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={ARTIFACT_METRIC} value={params.artifact_metric} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="channel_clipping_ratio">{t("params.channelClipping")}</SelectItem>
                  <SelectItem value="pixel_out_of_gamut_ratio">{t("params.pixelOog")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Selection policy */}
          <div>
            <ValueLabel tip={t("params.selectionPolicyTip")}>{t("params.selectionPolicy")}</ValueLabel>
            <Select
              value={params.selection_policy}
              onValueChange={(v) => {
                if (v) updateParams({ selection_policy: v as typeof params.selection_policy });
              }}
            >
              <SelectTrigger className="h-8 text-sm font-mono">
                <SelectedLabel labels={SELECTION_POLICY} value={params.selection_policy} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="gamut_only">{t("params.gamutOnly")}</SelectItem>
                <SelectItem value="hybrid">{t("params.hybrid")}</SelectItem>
                <SelectItem value="composite_only">{t("params.compositeOnly")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Fit strategy & clamp policy */}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <ValueLabel tip={t("params.fitStrategyTip")}>{t("params.fitStrategy")}</ValueLabel>
              <Select
                value={params.fit_strategy}
                onValueChange={(v) => {
                  if (v) updateParams({ fit_strategy: v as typeof params.fit_strategy });
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={FIT_STRATEGY} value={params.fit_strategy} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Cubic">{t("params.cubic")}</SelectItem>
                  <SelectItem value="DirectSearch">{t("params.directSearch")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip={t("params.clampPolicyTip")}>{t("params.clampPolicy")}</ValueLabel>
              <Select
                value={params.output_clamp}
                onValueChange={(v) => {
                  if (v) updateParams({ output_clamp: v as typeof params.output_clamp });
                }}
              >
                <SelectTrigger className="h-8 text-sm font-mono">
                  <SelectedLabel labels={CLAMP_POLICY} value={params.output_clamp} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="Clamp">{t("params.clamp")}</SelectItem>
                  <SelectItem value="Normalize">{t("params.normalize")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Toggles */}
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <Switch
                id="contrast"
                checked={params.enable_contrast_leveling}
                onCheckedChange={(v) =>
                  updateParams({ enable_contrast_leveling: v })
                }
              />
              <Label htmlFor="contrast" className="text-[13px] text-muted-foreground">
                {t("params.contrastLeveling")}
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="evaluator"
                checked={params.evaluator_config != null}
                onCheckedChange={(v) =>
                  updateParams({ evaluator_config: v ? "heuristic" : null })
                }
              />
              <Label htmlFor="evaluator" className="text-[13px] text-muted-foreground">
                {t("params.evaluator")}
              </Label>
            </div>
          </div>

          {/* Probe strengths override */}
          <div>
            <ValueLabel tip={t("params.probeStrengthsTip")}>{t("params.probeStrengths")}</ValueLabel>
            <Input
              className="h-8 text-sm font-mono"
              placeholder={t("params.probeStrengthsPlaceholder")}
              value={("Explicit" in params.probe_strengths ? params.probe_strengths.Explicit : []).join(", ")}
              onChange={(e) => {
                const vals = e.target.value
                  .split(",")
                  .map((s) => parseFloat(s.trim()))
                  .filter((n) => !isNaN(n) && n > 0);
                if (vals.length > 0) {
                  debouncedUpdate({
                    probe_strengths: { Explicit: vals },
                  });
                }
              }}
            />
          </div>

          {/* Metric weights */}
          <div className="space-y-1.5 pt-1">
            <div className="flex items-center justify-between">
              <ValueLabel>{t("params.metricWeights")}</ValueLabel>
              <button
                type="button"
                className="text-[10px] font-mono text-muted-foreground/60 hover:text-primary transition-colors"
                onClick={() => updateParams({ metric_weights: { ...DEFAULT_METRIC_WEIGHTS } })}
              >
                {t("params.reset")}
              </button>
            </div>
            {(
              [
                ["gamut_excursion", t("params.gamut")],
                ["halo_ringing", t("params.halo")],
                ["edge_overshoot", t("params.overshoot")],
                ["texture_flattening", t("params.texture")],
              ] as [keyof MetricWeights, string][]
            ).map(([key, label]) => (
              <div key={key}>
                <div className="flex items-baseline justify-between">
                  <span className="text-[11px] text-muted-foreground/70">{label}</span>
                  <span className="text-[10px] font-mono text-primary">
                    {params.metric_weights[key].toFixed(1)}
                  </span>
                </div>
                <Slider
                  min={0}
                  max={2.0}
                  step={0.1}
                  value={[params.metric_weights[key]]}
                  onValueChange={(v) => {
                    const w: MetricWeights = {
                      ...params.metric_weights,
                      [key]: sliderValue(v),
                    };
                    throttledUpdate({ metric_weights: w });
                  }}
                />
              </div>
            ))}
            <p className="text-[10px] text-muted-foreground/60 italic">
              {t("params.metricWeightsNote")}
            </p>
          </div>
        </CollapsibleContent>
      </Collapsible>
      </div>

    </div>
    </TooltipProvider>
  );
}
