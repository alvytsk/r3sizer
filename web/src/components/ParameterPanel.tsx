import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ChevronDown } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";

function sliderValue(v: number | readonly number[]): number {
  return Array.isArray(v) ? v[0] : (v as number);
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
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Parameters</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Dimensions */}
        <div className="space-y-2">
          <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
            Dimensions
          </Label>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <Label htmlFor="width" className="text-xs">
                Width
              </Label>
              <Input
                id="width"
                type="number"
                min={1}
                value={params.target_width}
                onChange={(e) =>
                  updateParams({ target_width: Number(e.target.value) || 1 })
                }
              />
            </div>
            <div>
              <Label htmlFor="height" className="text-xs">
                Height
              </Label>
              <Input
                id="height"
                type="number"
                min={1}
                value={params.target_height}
                onChange={(e) =>
                  updateParams({ target_height: Number(e.target.value) || 1 })
                }
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
              <Label htmlFor="aspect" className="text-xs">
                Lock aspect ratio
              </Label>
            </div>
          )}
        </div>

        {/* Sharpening */}
        <div className="space-y-2">
          <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
            Sharpening
          </Label>
          <div>
            <Label className="text-xs">
              Sigma: {params.sharpen_sigma.toFixed(1)}
            </Label>
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
              <Label className="text-xs">Mode</Label>
              <Select
                value={params.sharpen_mode}
                onValueChange={(v) => {
                  if (v) updateParams({ sharpen_mode: v as typeof params.sharpen_mode });
                }}
              >
                <SelectTrigger className="h-8 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="lightness">Lightness</SelectItem>
                  <SelectItem value="rgb">RGB</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label className="text-xs">Model</Label>
              <Select
                value={params.sharpen_model}
                onValueChange={(v) => {
                  if (v) updateParams({ sharpen_model: v as typeof params.sharpen_model });
                }}
              >
                <SelectTrigger className="h-8 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="practical_usm">Practical USM</SelectItem>
                  <SelectItem value="paper_lightness_approx">
                    Paper Lightness
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>

        {/* Metric */}
        <div className="space-y-2">
          <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
            Metric
          </Label>
          <div>
            <Label className="text-xs">
              Target artifact ratio: {params.target_artifact_ratio.toExponential(1)}
            </Label>
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
          <div className="grid grid-cols-2 gap-2">
            <div>
              <Label className="text-xs">Mode</Label>
              <Select
                value={params.metric_mode}
                onValueChange={(v) => {
                  if (v) updateParams({ metric_mode: v as typeof params.metric_mode });
                }}
              >
                <SelectTrigger className="h-8 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="relative_to_base">Relative</SelectItem>
                  <SelectItem value="absolute_total">Absolute</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label className="text-xs">Metric</Label>
              <Select
                value={params.artifact_metric}
                onValueChange={(v) => {
                  if (v) updateParams({ artifact_metric: v as typeof params.artifact_metric });
                }}
              >
                <SelectTrigger className="h-8 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="channel_clipping_ratio">
                    Channel Clipping
                  </SelectItem>
                  <SelectItem value="pixel_out_of_gamut_ratio">
                    Pixel OOG
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>

        {/* Advanced */}
        <Collapsible>
          <CollapsibleTrigger className="flex items-center gap-1 text-xs font-semibold text-muted-foreground uppercase tracking-wider hover:text-foreground transition-colors">
            <ChevronDown className="h-3 w-3" />
            Advanced
          </CollapsibleTrigger>
          <CollapsibleContent className="space-y-2 pt-2">
            <div className="grid grid-cols-2 gap-2">
              <div>
                <Label className="text-xs">Fit Strategy</Label>
                <Select
                  value={params.fit_strategy}
                  onValueChange={(v) => {
                    if (v) updateParams({ fit_strategy: v as typeof params.fit_strategy });
                  }}
                >
                  <SelectTrigger className="h-8 text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="Cubic">Cubic</SelectItem>
                    <SelectItem value="DirectSearch">Direct Search</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div>
                <Label className="text-xs">Clamp Policy</Label>
                <Select
                  value={params.output_clamp}
                  onValueChange={(v) => {
                    if (v) updateParams({ output_clamp: v as typeof params.output_clamp });
                  }}
                >
                  <SelectTrigger className="h-8 text-xs">
                    <SelectValue />
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
              <Label htmlFor="contrast" className="text-xs">
                Enable contrast leveling
              </Label>
            </div>
            <div>
              <Label className="text-xs">Probe strengths (comma-separated)</Label>
              <Input
                className="h-8 text-xs font-mono"
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
          </CollapsibleContent>
        </Collapsible>
      </CardContent>
    </Card>
  );
}
