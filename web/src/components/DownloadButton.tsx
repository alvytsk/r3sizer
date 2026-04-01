import { useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Download } from "lucide-react";
import { useProcessorStore, type ExportFormat } from "@/stores/processor-store";

const FORMAT_EXT: Record<ExportFormat, string> = {
  jpeg: "jpg",
  png: "png",
  webp: "webp",
};

const FORMAT_MIME: Record<ExportFormat, string> = {
  jpeg: "image/jpeg",
  png: "image/png",
  webp: "image/webp",
};

const QUALITY_PRESETS = [
  { label: "Max", value: 100 },
  { label: "High", value: 90 },
  { label: "Std", value: 80 },
  { label: "Low", value: 60 },
] as const;

export function DownloadButton() {
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const outputWidth = useProcessorStore((s) => s.outputWidth);
  const outputHeight = useProcessorStore((s) => s.outputHeight);
  const inputFile = useProcessorStore((s) => s.inputFile);
  const format = useProcessorStore((s) => s.exportFormat);
  const quality = useProcessorStore((s) => s.exportQuality);
  const setFormat = useProcessorStore((s) => s.setExportFormat);
  const setQuality = useProcessorStore((s) => s.setExportQuality);

  const handleDownload = useCallback(() => {
    if (!outputRgbaData) return;

    const canvas = document.createElement("canvas");
    canvas.width = outputWidth;
    canvas.height = outputHeight;
    const ctx = canvas.getContext("2d")!;
    const clamped = new Uint8ClampedArray(outputRgbaData.length);
    clamped.set(outputRgbaData);
    ctx.putImageData(new ImageData(clamped, outputWidth, outputHeight), 0, 0);

    const q = format === "png" ? undefined : quality / 100;
    canvas.toBlob(
      (blob) => {
        if (!blob) return;
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        const stem = inputFile?.name.replace(/\.[^.]+$/, "") ?? "r3sizer";
        a.download = `${stem}-${outputWidth}x${outputHeight}.${FORMAT_EXT[format]}`;
        a.click();
        URL.revokeObjectURL(url);
      },
      FORMAT_MIME[format],
      q
    );
  }, [outputRgbaData, outputWidth, outputHeight, format, quality, inputFile]);

  if (!outputRgbaData) return null;

  const isLossy = format !== "png";

  return (
    <div className="flex items-center gap-2">
      {/* Format selector — hidden below lg */}
      <div className="hidden lg:flex rounded-md border border-border/40 overflow-hidden">
        {(["jpeg", "png", "webp"] as const).map((fmt) => (
          <button
            key={fmt}
            onClick={() => setFormat(fmt)}
            className={`px-2.5 py-1 text-[11px] font-mono font-medium transition-colors ${
              format === fmt
                ? "bg-primary text-primary-foreground"
                : "bg-card text-foreground/60 hover:text-foreground hover:bg-accent"
            }`}
          >
            {fmt.toUpperCase()}
          </button>
        ))}
      </div>
      {/* Quality presets (lossy) / Lossless badge (PNG) — stable layout */}
      <div className="hidden xl:flex items-center">
        {isLossy ? (
          <div className="flex rounded-md border border-border/40 overflow-hidden">
            {QUALITY_PRESETS.map((preset) => (
              <button
                key={preset.label}
                onClick={() => setQuality(preset.value)}
                className={`px-2.5 py-1 text-[11px] font-mono font-medium transition-colors ${
                  quality === preset.value
                    ? "bg-primary/20 text-primary"
                    : "bg-card text-foreground/60 hover:text-foreground hover:bg-accent"
                }`}
              >
                {preset.label}
              </button>
            ))}
          </div>
        ) : (
          <span className="px-2 py-1 text-[11px] font-mono text-muted-foreground border border-border/40 rounded-md bg-card">
            Lossless
          </span>
        )}
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={handleDownload}
        className="font-mono text-[11px] dark:border-primary/30 dark:text-primary dark:hover:bg-primary/10 dark:hover:border-primary/50"
        title={`Save as ${format.toUpperCase()}${isLossy ? ` · Q${quality}` : " · Lossless"}`}
      >
        <Download className="h-3.5 w-3.5 mr-1" />
        {/* Below lg: show format in button since selector is hidden */}
        <span className="lg:hidden">{format.toUpperCase()}</span>
        <span className="hidden lg:inline">Save</span>
      </Button>
    </div>
  );
}
