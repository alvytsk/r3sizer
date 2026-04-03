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
  SHARPEN_MODE,
  METRIC_MODE,
  ARTIFACT_METRIC,
  SELECTION_POLICY,
  FIT_STRATEGY,
  CLAMP_POLICY,
  SHARPEN_STRATEGY,
  RESIZE_KERNEL,
  DIMENSION_PRESETS,
  ALL_PRESETS,
} from "./params/constants";

export function ParameterPanel() {
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

  return (
    <TooltipProvider delay={100}>
    <div className="p-3 space-y-4">
      {/* Dimensions */}
      <div className="space-y-2">
        <SectionLabel>Dimensions</SectionLabel>
        <div>
          <ValueLabel>Preset</ValueLabel>
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
                  : <span className="text-muted-foreground">Select preset…</span>
                }
              </span>
            </SelectTrigger>
            <SelectContent>
              {DIMENSION_PRESETS.map((group, gi) => (
                <SelectGroup key={group.group}>
                  {gi > 0 && <SelectSeparator />}
                  <SelectLabel>{group.group}</SelectLabel>
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
            <ValueLabel>Width</ValueLabel>
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
            title="Swap width and height"
          >
            <ArrowLeftRight className="h-3.5 w-3.5" />
          </button>
          <div className="flex-1 min-w-0">
            <ValueLabel>Height</ValueLabel>
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
                Lock ratio
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="pin-dims"
                checked={lockDimensions}
                onCheckedChange={setLockDimensions}
              />
              <Label htmlFor="pin-dims" className="text-[13px] text-foreground/70">
                Pin exact
              </Label>
            </div>
          </div>
        )}
      </div>

      {/* Pipeline preset */}
      <div className="space-y-2">
        <SectionLabel>Pipeline</SectionLabel>
        <div className="grid grid-cols-2 gap-1">
          {(["photo", "precision"] as const).map((key) => {
            const active = activePreset === key;
            const meta = key === "photo"
              ? { label: "Photo", desc: "natural images" }
              : { label: "Precision", desc: "text, UI, architecture" };
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
              fast: { label: "Fast", desc: "minimal probing" },
              balanced: { label: "Balanced", desc: "default" },
              quality: { label: "Quality", desc: "extended probing" },
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
              ({(params.target_artifact_ratio * 100).toFixed(1)}% budget)
            </span>
          </div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground/70 font-mono">
            <span>
              {"TwoPass" in params.probe_strengths
                ? `${params.probe_strengths.TwoPass.coarse_count}+${params.probe_strengths.TwoPass.dense_count} probes`
                : `${(params.probe_strengths as { Explicit: number[] }).Explicit.length} probes`}
            </span>
            <span className="text-border/60">|</span>
            <span>
              {params.sharpen_strategy.strategy === "content_adaptive" ? "adaptive" : "uniform"}
              {params.experimental_sharpen_mode ? " + guard" : ""}
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
        <SectionLabel>Sharpening</SectionLabel>
        <div>
          <div className="flex items-baseline justify-between">
            <ValueLabel tip="Gaussian blur radius for the unsharp mask. Higher values sharpen coarser details but risk halos around edges. Lower values sharpen fine detail with fewer artifacts.">Sigma</ValueLabel>
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
          <ValueLabel tip="Interpolation filter for downscaling. Lanczos3 is sharpest; Gaussian is smoothest; Catmull-Rom / Mitchell-Netravali are balanced cubic filters. Content Adaptive selects kernel per region (flat → Gaussian, edges → Lanczos3, etc.).">Resize Kernel</ValueLabel>
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
              <SelectItem value="lanczos3">Lanczos3</SelectItem>
              <SelectItem value="mitchell_netravali">Mitchell-Netravali</SelectItem>
              <SelectItem value="catmull_rom">Catmull-Rom</SelectItem>
              <SelectItem value="gaussian">Gaussian</SelectItem>
              <SelectItem value="content_adaptive">Content Adaptive</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Advanced — everything else collapsed */}
      <div className="space-y-2">
      <Collapsible>
        <CollapsibleTrigger className="group flex items-center gap-1.5 text-xs font-mono font-semibold uppercase tracking-[0.15em] text-muted-foreground/60 hover:text-primary transition-colors border-b border-border/20 pb-1 w-full">
          <div className="w-0.5 h-3 rounded-full bg-muted-foreground/20 group-hover:bg-primary/50 transition-colors" />
          Advanced
          <ChevronDown className="h-3 w-3 ml-auto transition-transform duration-200 group-data-[panel-open]:rotate-180" />
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-3 pt-3">

          {/* Target artifact ratio */}
          <div>
            <div className="flex items-baseline justify-between">
              <ValueLabel tip="Maximum allowed fraction of pixels with out-of-range values after sharpening. Lower = less artifacts but softer image. Higher = sharper but more clipping.">Target P(s)</ValueLabel>
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
              <ValueLabel tip="Lightness sharpens only luminance (less color fringing). RGB sharpens all channels independently (stronger but may shift colors).">Mode</ValueLabel>
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
                  <SelectItem value="lightness">Lightness</SelectItem>
                  <SelectItem value="rgb">RGB</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip="Uniform applies equal sharpening everywhere. Content Adaptive varies strength per region.">Strategy</ValueLabel>
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
                  <SelectItem value="uniform">Uniform</SelectItem>
                  <SelectItem value="content_adaptive">Content Adaptive</SelectItem>
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
              <ValueLabel tip="Relative subtracts the baseline artifact ratio so only sharpening-induced artifacts are measured.">Metric Mode</ValueLabel>
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
                  <SelectItem value="relative_to_base">Relative</SelectItem>
                  <SelectItem value="absolute_total">Absolute</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip="Channel Clipping counts individual R/G/B values outside [0,1]. Pixel Out-of-Gamut counts pixels where any channel is clipped.">Artifact Metric</ValueLabel>
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
                  <SelectItem value="channel_clipping_ratio">Channel Clipping</SelectItem>
                  <SelectItem value="pixel_out_of_gamut_ratio">Pixel OOG</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Selection policy */}
          <div>
            <ValueLabel tip="Gamut Only uses gamut excursion for both fitting and fallback ranking. Hybrid keeps gamut as the hard safety constraint but ranks fallback candidates by composite score (halo, overshoot, texture). Composite Only is experimental.">Selection Policy</ValueLabel>
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
                <SelectItem value="gamut_only">Gamut Only</SelectItem>
                <SelectItem value="hybrid">Hybrid</SelectItem>
                <SelectItem value="composite_only">Composite Only (exp)</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Fit strategy & clamp policy */}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <ValueLabel tip="Cubic fits a polynomial and solves analytically. Direct Search picks the best probe sample directly.">Fit Strategy</ValueLabel>
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
                  <SelectItem value="Cubic">Cubic</SelectItem>
                  <SelectItem value="DirectSearch">Direct Search</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <ValueLabel tip="Clamp clips values to [0,1]. Normalize rescales the entire image to fit.">Clamp Policy</ValueLabel>
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
                  <SelectItem value="Clamp">Clamp</SelectItem>
                  <SelectItem value="Normalize">Normalize</SelectItem>
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
                Contrast leveling
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
                Evaluator
              </Label>
            </div>
          </div>

          {/* Probe strengths override */}
          <div>
            <ValueLabel tip="Override the two-pass probe placement with explicit comma-separated strengths.">Probe strengths</ValueLabel>
            <Input
              className="h-8 text-sm font-mono"
              placeholder="auto (two-pass)"
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
              <ValueLabel>Metric Weights</ValueLabel>
              <button
                type="button"
                className="text-[10px] font-mono text-muted-foreground/60 hover:text-primary transition-colors"
                onClick={() => updateParams({ metric_weights: { ...DEFAULT_METRIC_WEIGHTS } })}
              >
                reset
              </button>
            </div>
            {(
              [
                ["gamut_excursion", "Gamut"],
                ["halo_ringing", "Halo"],
                ["edge_overshoot", "Overshoot"],
                ["texture_flattening", "Texture"],
              ] as const
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
              Diagnostic only — does not affect selection
            </p>
          </div>
        </CollapsibleContent>
      </Collapsible>
      </div>

    </div>
    </TooltipProvider>
  );
}
