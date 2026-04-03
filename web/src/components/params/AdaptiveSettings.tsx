import { Slider } from "@/components/ui/slider";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ChevronDown } from "lucide-react";
import { ValueLabel } from "./helpers";
import { sliderValue } from "./constants";
import type {
  AutoSharpParams,
  GainTable,
  ClassificationParams,
  ContentAdaptiveStrategy,
} from "@/types/wasm-types";
import {
  DEFAULT_GAIN_TABLE,
  DEFAULT_CLASSIFICATION_PARAMS,
} from "@/types/wasm-types";

const GAIN_TABLE_ENTRIES: [keyof GainTable, string][] = [
  ["flat", "Flat"],
  ["textured", "Textured"],
  ["strong_edge", "Strong Edge"],
  ["microtexture", "Microtexture"],
  ["risky_halo_zone", "Risky Halo"],
];

const CLASSIFICATION_ENTRIES: [keyof Omit<ClassificationParams, "variance_window">, string, number, number, number][] = [
  ["gradient_low_threshold", "Grad Low", 0, 1, 0.01],
  ["gradient_high_threshold", "Grad High", 0, 2, 0.01],
  ["variance_low_threshold", "Var Low", 0, 0.1, 0.001],
  ["variance_high_threshold", "Var High", 0, 0.1, 0.001],
];

export function AdaptiveSettings({
  strategy,
  updateParams,
}: {
  strategy: ContentAdaptiveStrategy;
  updateParams: (partial: Partial<AutoSharpParams>) => void;
}) {
  function updateStrategy(patch: Partial<ContentAdaptiveStrategy>): void {
    updateParams({ sharpen_strategy: { ...strategy, ...patch } });
  }

  return (
    <Collapsible>
      <CollapsibleTrigger className="group flex items-center gap-1 text-xs font-mono font-semibold uppercase tracking-[0.15em] text-muted-foreground hover:text-primary transition-colors">
        <ChevronDown className="h-3 w-3 transition-transform duration-200 group-data-[panel-open]:rotate-180" />
        Adaptive Settings
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-2">
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <ValueLabel>Gain Table</ValueLabel>
            <button
              type="button"
              className="text-[10px] font-mono text-muted-foreground/60 hover:text-primary transition-colors"
              onClick={() => updateStrategy({ gain_table: { ...DEFAULT_GAIN_TABLE } })}
            >
              reset
            </button>
          </div>
          {GAIN_TABLE_ENTRIES.map(([key, label]) => (
            <div key={key}>
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-muted-foreground/70">{label}</span>
                <span className="text-[10px] font-mono text-primary">
                  {strategy.gain_table[key].toFixed(2)}
                </span>
              </div>
              <Slider
                min={0.25}
                max={2.0}
                step={0.05}
                value={[strategy.gain_table[key]]}
                onValueChange={(v) =>
                  updateStrategy({
                    gain_table: { ...strategy.gain_table, [key]: sliderValue(v) },
                  })
                }
              />
            </div>
          ))}
        </div>

        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <ValueLabel>Classification</ValueLabel>
            <button
              type="button"
              className="text-[10px] font-mono text-muted-foreground/60 hover:text-primary transition-colors"
              onClick={() => updateStrategy({ classification: { ...DEFAULT_CLASSIFICATION_PARAMS } })}
            >
              reset
            </button>
          </div>
          {CLASSIFICATION_ENTRIES.map(([key, label, min, max, step]) => (
            <div key={key}>
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-muted-foreground/70">{label}</span>
                <span className="text-[10px] font-mono text-primary">
                  {strategy.classification[key].toFixed(3)}
                </span>
              </div>
              <Slider
                min={min}
                max={max}
                step={step}
                value={[strategy.classification[key]]}
                onValueChange={(v) =>
                  updateStrategy({
                    classification: { ...strategy.classification, [key]: sliderValue(v) },
                  })
                }
              />
            </div>
          ))}
          <div>
            <div className="flex items-baseline justify-between">
              <span className="text-[11px] text-muted-foreground/70">Var Window</span>
              <span className="text-[10px] font-mono text-primary">
                {strategy.classification.variance_window}
              </span>
            </div>
            <Slider
              min={3}
              max={11}
              step={2}
              value={[strategy.classification.variance_window]}
              onValueChange={(v) =>
                updateStrategy({
                  classification: { ...strategy.classification, variance_window: sliderValue(v) },
                })
              }
            />
          </div>
        </div>

        <div className="space-y-1.5">
          <ValueLabel>Backoff</ValueLabel>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <span className="text-[11px] text-muted-foreground/70">Max Iterations</span>
              <Slider
                min={0}
                max={10}
                step={1}
                value={[strategy.max_backoff_iterations]}
                onValueChange={(v) =>
                  updateStrategy({ max_backoff_iterations: sliderValue(v) })
                }
              />
              <span className="text-[10px] font-mono text-primary">
                {strategy.max_backoff_iterations}
              </span>
            </div>
            <div>
              <span className="text-[11px] text-muted-foreground/70">Scale Factor</span>
              <Slider
                min={0.1}
                max={0.95}
                step={0.05}
                value={[strategy.backoff_scale_factor]}
                onValueChange={(v) =>
                  updateStrategy({ backoff_scale_factor: sliderValue(v) })
                }
              />
              <span className="text-[10px] font-mono text-primary">
                {strategy.backoff_scale_factor.toFixed(2)}
              </span>
            </div>
          </div>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
