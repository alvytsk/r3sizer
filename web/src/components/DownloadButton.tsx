import { useCallback, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Download } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";

export function DownloadButton() {
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const outputWidth = useProcessorStore((s) => s.outputWidth);
  const outputHeight = useProcessorStore((s) => s.outputHeight);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const handleDownload = useCallback(() => {
    if (!outputRgbaData) return;

    const canvas = document.createElement("canvas");
    canvas.width = outputWidth;
    canvas.height = outputHeight;
    const ctx = canvas.getContext("2d")!;
    const clamped = new Uint8ClampedArray(outputRgbaData.length);
    clamped.set(outputRgbaData);
    const imgData = new ImageData(clamped, outputWidth, outputHeight);
    ctx.putImageData(imgData, 0, 0);

    canvas.toBlob((blob) => {
      if (!blob) return;
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `r3sizer-${outputWidth}x${outputHeight}.png`;
      a.click();
      URL.revokeObjectURL(url);
    }, "image/png");
  }, [outputRgbaData, outputWidth, outputHeight]);

  if (!outputRgbaData) return null;

  return (
    <>
      <canvas ref={canvasRef} className="hidden" />
      <Button variant="outline" size="sm" onClick={handleDownload}>
        <Download className="h-4 w-4 mr-1" />
        Download PNG
      </Button>
    </>
  );
}
