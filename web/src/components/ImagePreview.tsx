import { useEffect, useRef } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useProcessorStore } from "@/stores/processor-store";

function CanvasPreview({
  rgbaData,
  width,
  height,
  label,
}: {
  rgbaData: Uint8Array;
  width: number;
  height: number;
  label: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d")!;
    const clamped = new Uint8ClampedArray(rgbaData.length);
    clamped.set(rgbaData);
    const imgData = new ImageData(clamped, width, height);
    ctx.putImageData(imgData, 0, 0);
  }, [rgbaData, width, height]);

  return (
    <div className="flex-1 min-w-0">
      <p className="text-xs font-medium text-muted-foreground mb-1">
        {label} ({width}&times;{height})
      </p>
      <canvas
        ref={canvasRef}
        className="w-full h-auto rounded border bg-[url('data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2216%22%20height%3D%2216%22%3E%3Crect%20width%3D%228%22%20height%3D%228%22%20fill%3D%22%23ccc%22/%3E%3Crect%20x%3D%228%22%20y%3D%228%22%20width%3D%228%22%20height%3D%228%22%20fill%3D%22%23ccc%22/%3E%3C/svg%3E')]"
      />
    </div>
  );
}

export function ImagePreview() {
  const inputRgbaData = useProcessorStore((s) => s.inputRgbaData);
  const inputWidth = useProcessorStore((s) => s.inputWidth);
  const inputHeight = useProcessorStore((s) => s.inputHeight);
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const outputWidth = useProcessorStore((s) => s.outputWidth);
  const outputHeight = useProcessorStore((s) => s.outputHeight);

  if (!inputRgbaData) return null;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Preview</CardTitle>
      </CardHeader>
      <CardContent className="flex gap-4">
        <CanvasPreview
          rgbaData={inputRgbaData}
          width={inputWidth}
          height={inputHeight}
          label="Input"
        />
        {outputRgbaData ? (
          <CanvasPreview
            rgbaData={outputRgbaData}
            width={outputWidth}
            height={outputHeight}
            label="Output"
          />
        ) : (
          <div className="flex-1 min-w-0 flex items-center justify-center border rounded bg-muted/50 min-h-[100px]">
            <p className="text-xs text-muted-foreground">
              Output will appear here
            </p>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
