import { useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Download } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";

type ExportFormat = "jpeg" | "png" | "webp";

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

export function DownloadButton() {
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const outputWidth = useProcessorStore((s) => s.outputWidth);
  const outputHeight = useProcessorStore((s) => s.outputHeight);
  const inputFile = useProcessorStore((s) => s.inputFile);
  const [format, setFormat] = useState<ExportFormat>("jpeg");
  const [quality, setQuality] = useState(92);

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
  }, [outputRgbaData, outputWidth, outputHeight, format, quality]);

  if (!outputRgbaData) return null;

  return (
    <div className="flex items-center gap-2">
      {/* Format selector — hidden below lg */}
      <div className="hidden lg:flex rounded-md border border-border/40 overflow-hidden">
        {(["jpeg", "png", "webp"] as const).map((fmt) => (
          <button
            key={fmt}
            onClick={() => setFormat(fmt)}
            className={`px-2 py-1 text-[11px] font-mono transition-colors ${
              format === fmt
                ? "bg-primary text-primary-foreground"
                : "bg-card text-muted-foreground hover:text-foreground hover:bg-accent"
            }`}
          >
            {fmt.toUpperCase()}
          </button>
        ))}
      </div>
      {/* Quality slider — hidden below xl or when PNG */}
      {format !== "png" && (
        <div className="hidden xl:flex items-center gap-1.5">
          <input
            type="range"
            min={10}
            max={100}
            value={quality}
            onChange={(e) => setQuality(Number(e.target.value))}
            className="w-20 h-1 rounded-full appearance-none bg-border cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-2.5 [&::-webkit-slider-thumb]:h-2.5 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-primary"
          />
          <span className="text-[11px] font-mono text-muted-foreground w-7 text-right">{quality}</span>
        </div>
      )}
      <Button
        variant="outline"
        size="sm"
        onClick={handleDownload}
        className="font-mono text-[11px]"
        title={`Save as ${format.toUpperCase()}`}
      >
        <Download className="h-3.5 w-3.5 mr-1" />
        {/* Below lg: show format in button since selector is hidden */}
        <span className="lg:hidden">{format.toUpperCase()}</span>
        <span className="hidden lg:inline">Save</span>
      </Button>
    </div>
  );
}
