import { useState, useCallback, useRef } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ChevronDown } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";
import type { MetricWeights } from "@/types/wasm-types";
import { DEFAULT_METRIC_WEIGHTS } from "@/types/wasm-types";

function sliderValue(v: number | readonly number[]): number {
  return Array.isArray(v) ? v[0] : (v as number);
}

/** Text input with local draft state + custom stepper arrows. */
function NumericInput({
  value,
  min,
  step = 1,
  onCommit,
  id,
  className,
}: {
  value: number;
  min?: number;
  step?: number;
  onCommit: (v: number) => void;
  id?: string;
  className?: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const editing = draft !== null;
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const valueRef = useRef(value);
  valueRef.current = value;

  const clamp = useCallback(
    (n: number) => Math.max(min ?? -Infinity, n),
    [min]
  );

  const commit = useCallback(() => {
    if (draft === null) return;
    const n = parseInt(draft, 10);
    if (!isNaN(n)) onCommit(clamp(n));
    setDraft(null);
  }, [draft, clamp, onCommit]);

  const nudge = useCallback(
    (dir: 1 | -1) => onCommit(clamp(value + step * dir)),
    [value, step, clamp, onCommit]
  );

  const stopRepeat = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    document.removeEventListener("pointerup", stopRepeat);
    document.removeEventListener("pointercancel", stopRepeat);
  }, []);

  const startRepeat = useCallback(
    (dir: 1 | -1) => {
      nudge(dir);
      // Initial delay before repeat kicks in (like key-repeat)
      let count = 0;
      const tick = () => {
        count++;
        const s = count > 6 ? step * 10 : step;
        onCommit(clamp(valueRef.current + s * dir));
      };
      const timeout = setTimeout(() => {
        tick();
        intervalRef.current = setInterval(tick, 100);
      }, 400);
      intervalRef.current = timeout as unknown as ReturnType<typeof setInterval>;
      document.addEventListener("pointerup", stopRepeat);
      document.addEventListener("pointercancel", stopRepeat);
    },
    [nudge, step, clamp, onCommit, stopRepeat]
  );

  const chevron = (
    <svg width="8" height="5" viewBox="0 0 8 5" fill="none">
      <path
        d="M1 1.5L4 3.5L7 1.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );

  return (
    <div className="relative group">
      <Input
        id={id}
        inputMode="numeric"
        className={`${className ?? ""} pr-6`}
        value={editing ? draft : String(value)}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => {
          setDraft(String(value));
          e.target.select();
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp") {
            e.preventDefault();
            nudge(1);
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            nudge(-1);
          } else if (e.key === "Enter") {
            commit();
            (e.target as HTMLInputElement).blur();
          } else if (e.key === "Escape") {
            setDraft(null);
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
      {/* Custom stepper buttons */}
      <div className="absolute right-px top-px bottom-px w-5 flex flex-col rounded-r-lg overflow-hidden opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity">
        <button
          type="button"
          tabIndex={-1}
          className="flex-1 flex items-center justify-center text-muted-foreground/50 hover:text-primary hover:bg-primary/10 transition-colors rotate-180"
          onPointerDown={(e) => {
            e.preventDefault();
            startRepeat(1);
          }}
        >
          {chevron}
        </button>
        <div className="h-px bg-border/40" />
        <button
          type="button"
          tabIndex={-1}
          className="flex-1 flex items-center justify-center text-muted-foreground/50 hover:text-primary hover:bg-primary/10 transition-colors"
          onPointerDown={(e) => {
            e.preventDefault();
            startRepeat(-1);
          }}
        >
          {chevron}
        </button>
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-1.5 text-xs font-mono font-semibold uppercase tracking-[0.15em] text-primary border-b border-border/30 pb-1">
      <div className="w-0.5 h-3 rounded-full bg-primary" />
      {children}
    </div>
  );
}

function ValueLabel({ children }: { children: React.ReactNode }) {
  return <Label className="text-[13px] text-muted-foreground">{children}</Label>;
}

const SHARPEN_MODE: Record<string, string> = {
  lightness: "Lightness",
  rgb: "RGB",
};

const SHARPEN_MODEL: Record<string, string> = {
  practical_usm: "Practical USM",
  paper_lightness_approx: "Paper Lightness",
};

const METRIC_MODE: Record<string, string> = {
  relative_to_base: "Relative to Baseline",
  absolute_total: "Absolute Total",
};

const ARTIFACT_METRIC: Record<string, string> = {
  channel_clipping_ratio: "Channel Clipping Ratio",
  pixel_out_of_gamut_ratio: "Pixel Out-of-Gamut Ratio",
};

const FIT_STRATEGY: Record<string, string> = {
  Cubic: "Cubic",
  DirectSearch: "Direct Search",
};

const CLAMP_POLICY: Record<string, string> = {
  Clamp: "Clamp",
  Normalize: "Normalize",
};

function SelectedLabel({ labels, value }: { labels: Record<string, string>; value: string }) {
  return (
    <span className="flex flex-1 text-left truncate" data-slot="select-value">
      {labels[value] ?? value}
    </span>
  );
}

export function ParameterPanel() {
  const params = useProcessorStore((s) => s.params);
  const updateParams = useProcessorStore((s) => s.updateParams);
  const preserveAspectRatio = useProcessorStore((s) => s.preserveAspectRatio);
  const setPreserveAspectRatio = useProcessorStore(
    (s) => s.setPreserveAspectRatio
  );
  const inputWidth = useProcessorStore((s) => s.inputWidth);

  const logRatio = Math.log10(params.target_artifact_ratio);

  return (
    <div className="p-3 space-y-4">
      {/* Dimensions */}
      <div className="space-y-2">
        <SectionLabel>Dimensions</SectionLabel>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <ValueLabel>Width</ValueLabel>
            <NumericInput
              id="width"
              min={1}
              className="h-8 text-sm font-mono"
              value={params.target_width}
              onCommit={(v) => updateParams({ target_width: v })}
            />
          </div>
          <div>
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
          <div className="flex items-center gap-2">
            <Switch
              id="aspect"
              checked={preserveAspectRatio}
              onCheckedChange={setPreserveAspectRatio}
            />
            <Label htmlFor="aspect" className="text-[13px] text-muted-foreground">
              Lock aspect ratio
            </Label>
          </div>
        )}
      </div>

      {/* Sharpening */}
      <div className="space-y-2">
        <SectionLabel>Sharpening</SectionLabel>
        <div>
          <div className="flex items-baseline justify-between">
            <ValueLabel>Sigma</ValueLabel>
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
              updateParams({ sharpen_sigma: sliderValue(v) })
            }
          />
        </div>
        <div className="grid grid-cols-2 gap-2">
          <div>
            <ValueLabel>Mode</ValueLabel>
            <Select
              value={params.sharpen_mode}
              onValueChange={(v) => {
                if (!v) return;
                const update: Partial<typeof params> = { sharpen_mode: v as typeof params.sharpen_mode };
                if (v === "rgb" && params.sharpen_model === "paper_lightness_approx") {
                  update.sharpen_model = "practical_usm";
                }
                updateParams(update);
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
            <ValueLabel>Model</ValueLabel>
            <Select
              value={params.sharpen_model}
              onValueChange={(v) => {
                if (!v) return;
                const update: Partial<typeof params> = { sharpen_model: v as typeof params.sharpen_model };
                if (v === "paper_lightness_approx" && params.sharpen_mode !== "lightness") {
                  update.sharpen_mode = "lightness";
                }
                updateParams(update);
              }}
            >
              <SelectTrigger className="h-8 text-sm font-mono">
                <SelectedLabel labels={SHARPEN_MODEL} value={params.sharpen_model} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="practical_usm">Practical USM</SelectItem>
                <SelectItem value="paper_lightness_approx">Paper Lightness</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </div>

      {/* Metric */}
      <div className="space-y-2">
        <SectionLabel>Metric</SectionLabel>
        <div>
          <div className="flex items-baseline justify-between">
            <ValueLabel>Target P(s)</ValueLabel>
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
              updateParams({ target_artifact_ratio: Math.pow(10, sliderValue(v)) })
            }
          />
        </div>
        <div>
          <ValueLabel>Metric Mode</ValueLabel>
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
              <SelectItem value="relative_to_base">Relative to Baseline</SelectItem>
              <SelectItem value="absolute_total">Absolute Total</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div>
          <ValueLabel>Artifact Metric</ValueLabel>
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
              <SelectItem value="channel_clipping_ratio">Channel Clipping Ratio</SelectItem>
              <SelectItem value="pixel_out_of_gamut_ratio">Pixel Out-of-Gamut Ratio</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Advanced */}
      <Collapsible>
        <CollapsibleTrigger className="flex items-center gap-1 text-xs font-mono font-semibold uppercase tracking-[0.15em] text-muted-foreground hover:text-primary transition-colors">
          <ChevronDown className="h-3 w-3" />
          Advanced
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-2 pt-2">
          <div className="grid grid-cols-2 gap-2">
            <div>
              <ValueLabel>Fit Strategy</ValueLabel>
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
              <ValueLabel>Clamp Policy</ValueLabel>
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
          <div>
            <ValueLabel>Probe strengths</ValueLabel>
            <Input
              className="h-8 text-sm font-mono"
              placeholder="comma-separated"
              value={("Explicit" in params.probe_strengths ? params.probe_strengths.Explicit : []).join(", ")}
              onChange={(e) => {
                const vals = e.target.value
                  .split(",")
                  .map((s) => parseFloat(s.trim()))
                  .filter((n) => !isNaN(n) && n > 0);
                if (vals.length > 0) {
                  updateParams({
                    probe_strengths: { Explicit: vals },
                  });
                }
              }}
            />
          </div>

          {/* Metric weights (v0.2 composite) */}
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
                    updateParams({ metric_weights: w });
                  }}
                />
              </div>
            ))}
            <p className="text-[10px] text-muted-foreground/40 italic">
              Diagnostic only — does not affect selection
            </p>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
