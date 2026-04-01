import { useEffect, useRef, useState, useCallback } from "react";
import { useProcessorStore } from "@/stores/processor-store";

function renderToCanvas(
  canvas: HTMLCanvasElement,
  rgbaData: Uint8Array,
  width: number,
  height: number
) {
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d")!;
  const clamped = new Uint8ClampedArray(rgbaData.length);
  clamped.set(rgbaData);
  ctx.putImageData(new ImageData(clamped, width, height), 0, 0);
}

/** Measures parent and fits content at the given aspect ratio. */
function useFittedDims(
  wrapperRef: React.RefObject<HTMLDivElement | null>,
  aspectW: number,
  aspectH: number
) {
  const [dims, setDims] = useState({ w: 0, h: 0 });

  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return;
    const ratio = aspectW / aspectH;
    const update = () => {
      const pw = wrapper.clientWidth;
      const ph = wrapper.clientHeight;
      let w = pw;
      let h = pw / ratio;
      if (h > ph) {
        h = ph;
        w = h * ratio;
      }
      setDims({ w: Math.floor(w), h: Math.floor(h) });
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(wrapper);
    return () => ro.disconnect();
  }, [wrapperRef, aspectW, aspectH]);

  return dims;
}

function FittedCanvas({
  rgbaData,
  width,
  height,
}: {
  rgbaData: Uint8Array;
  width: number;
  height: number;
}) {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dims = useFittedDims(wrapperRef, width, height);

  useEffect(() => {
    if (canvasRef.current)
      renderToCanvas(canvasRef.current, rgbaData, width, height);
  }, [rgbaData, width, height]);

  return (
    <div ref={wrapperRef} className="flex-1 flex items-center justify-center min-h-0">
      <canvas
        ref={canvasRef}
        className="rounded-sm bg-black/20"
        style={{ width: dims.w, height: dims.h, visibility: dims.w > 0 ? "visible" : "hidden" }}
      />
    </div>
  );
}

function ComparisonSlider({
  inputRgba,
  inputW,
  inputH,
  outputRgba,
  outputW,
  outputH,
}: {
  inputRgba: Uint8Array;
  inputW: number;
  inputH: number;
  outputRgba: Uint8Array;
  outputW: number;
  outputH: number;
}) {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputCanvasRef = useRef<HTMLCanvasElement>(null);
  const outputCanvasRef = useRef<HTMLCanvasElement>(null);
  const [sliderPos, setSliderPos] = useState(50);
  const dims = useFittedDims(wrapperRef, outputW, outputH);

  useEffect(() => {
    if (inputCanvasRef.current)
      renderToCanvas(inputCanvasRef.current, inputRgba, inputW, inputH);
  }, [inputRgba, inputW, inputH]);

  useEffect(() => {
    if (outputCanvasRef.current)
      renderToCanvas(outputCanvasRef.current, outputRgba, outputW, outputH);
  }, [outputRgba, outputW, outputH]);

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;

    const updatePos = (clientX: number) => {
      const rect = container.getBoundingClientRect();
      const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
      setSliderPos((x / rect.width) * 100);
    };
    updatePos(e.clientX);

    const onMove = (ev: PointerEvent) => updatePos(ev.clientX);
    const onUp = () => {
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
    };
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }, []);

  return (
    <div ref={wrapperRef} className="flex-1 flex items-center justify-center min-h-0">
      <div
        ref={containerRef}
        className="relative select-none cursor-ew-resize overflow-hidden rounded-sm bg-black/20 touch-none"
        style={{ width: dims.w, height: dims.h, visibility: dims.w > 0 ? "visible" : "hidden" }}
        onPointerDown={handlePointerDown}
      >
        <canvas ref={inputCanvasRef} className="absolute inset-0 w-full h-full" />
        <div className="absolute inset-0" style={{ clipPath: `inset(0 0 0 ${sliderPos}%)` }}>
          <canvas ref={outputCanvasRef} className="w-full h-full" />
        </div>

        <div
          className="absolute top-0 bottom-0 z-10 pointer-events-none"
          style={{ left: `${sliderPos}%`, transform: "translateX(-50%)" }}
        >
          <div className="w-px h-full bg-white/80" />
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-7 h-7 rounded-full bg-background/80 border border-white/50 flex items-center justify-center backdrop-blur-sm">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" className="text-foreground/60">
              <path d="M4 3L2 7L4 11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              <path d="M10 3L12 7L10 11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        </div>

        <span className="absolute bottom-2 left-2 z-10 pointer-events-none text-[10px] font-mono text-white/90 bg-black/60 px-1.5 py-0.5 rounded-sm backdrop-blur-sm">
          <span className="uppercase tracking-widest">Input</span> <span className="text-white/60">{inputW}&times;{inputH}</span>
        </span>
        <span className="absolute bottom-2 right-2 z-10 pointer-events-none text-[10px] font-mono text-white/90 bg-black/60 px-1.5 py-0.5 rounded-sm backdrop-blur-sm">
          <span className="uppercase tracking-widest">Output</span> <span className="text-white/60">{outputW}&times;{outputH}</span>
        </span>
      </div>
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

  if (outputRgbaData) {
    return (
      <div className="flex-1 flex flex-col min-h-0">
        <div className="flex items-baseline justify-between mb-1 flex-shrink-0">
          <span className="text-[11px] font-mono uppercase tracking-widest text-muted-foreground">
            Compare
          </span>
          <span className="text-[11px] font-mono text-muted-foreground/60">
            {inputWidth}&times;{inputHeight} {"\u2192"} {outputWidth}&times;{outputHeight}
          </span>
        </div>
        <ComparisonSlider
          inputRgba={inputRgbaData}
          inputW={inputWidth}
          inputH={inputHeight}
          outputRgba={outputRgbaData}
          outputW={outputWidth}
          outputH={outputHeight}
        />
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="flex items-baseline justify-between mb-1 flex-shrink-0">
        <span className="text-[11px] font-mono uppercase tracking-widest text-muted-foreground">
          Input
        </span>
        <span className="text-[11px] font-mono text-muted-foreground/60">
          {inputWidth}&times;{inputHeight}
        </span>
      </div>
      <FittedCanvas rgbaData={inputRgbaData} width={inputWidth} height={inputHeight} />
    </div>
  );
}
