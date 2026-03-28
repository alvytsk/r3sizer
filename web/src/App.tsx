import { useState, useRef, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Loader2, FolderOpen, ChevronLeft, ChevronRight, BarChart3, SlidersHorizontal } from "lucide-react";
import { DownloadButton } from "@/components/DownloadButton";
import { ImageUpload } from "@/components/ImageUpload";
import { ImagePreview } from "@/components/ImagePreview";
import { ParameterPanel } from "@/components/ParameterPanel";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { useProcessorStore } from "@/stores/processor-store";

const ACCEPTED = ".png,.jpg,.jpeg,.bmp,.webp,.gif,.tiff";

/** Inline crosshair logo mark derived from favicon.svg (no background rect). */
function LogoMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <rect x="6" y="6" width="20" height="20" rx="2" stroke="currentColor" strokeWidth="1.5" opacity="0.45" />
      <rect x="10" y="10" width="12" height="12" rx="1" stroke="currentColor" strokeWidth="1.5" />
      <line x1="16" y1="4" x2="16" y2="10" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="16" y1="22" x2="16" y2="28" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="4" y1="16" x2="10" y2="16" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="22" y1="16" x2="28" y2="16" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <circle cx="16" cy="16" r="1.5" fill="currentColor" />
    </svg>
  );
}

export default function App() {
  const inputFile = useProcessorStore((s) => s.inputFile);
  const isProcessing = useProcessorStore((s) => s.isProcessing);
  const error = useProcessorStore((s) => s.error);
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const params = useProcessorStore((s) => s.params);
  const lastProcessedParams = useProcessorStore((s) => s.lastProcessedParams);
  const setInput = useProcessorStore((s) => s.setInput);
  const process = useProcessorStore((s) => s.process);
  const reset = useProcessorStore((s) => s.reset);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => typeof window !== "undefined" && window.innerWidth >= 1024
  );
  const [diagOpen, setDiagOpen] = useState(
    () => typeof window !== "undefined" && window.innerWidth >= 1280
  );
  const fileInputRef = useRef<HTMLInputElement>(null);

  const paramsChanged = !!(
    lastProcessedParams &&
    outputRgbaData &&
    JSON.stringify(params) !== JSON.stringify(lastProcessedParams)
  );

  const handleOpenFile = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      const img = new Image();
      const url = URL.createObjectURL(file);
      img.onload = () => {
        const canvas = document.createElement("canvas");
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
        const ctx = canvas.getContext("2d")!;
        ctx.drawImage(img, 0, 0);
        const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
        setInput(
          file,
          new Uint8Array(imageData.data.buffer),
          canvas.width,
          canvas.height
        );
        URL.revokeObjectURL(url);
      };
      img.src = url;
      e.target.value = "";
    },
    [setInput]
  );

  return (
    <div className="h-screen flex flex-col relative grain overflow-hidden">
      {/* Hidden file input for "Open" */}
      <input
        ref={fileInputRef}
        type="file"
        accept={ACCEPTED}
        className="hidden"
        onChange={handleOpenFile}
      />

      {/* Top bar — instrument panel zones */}
      <header className="border-b border-border/60 px-4 py-2.5 flex items-center gap-3 backdrop-blur-sm bg-background/80 sticky top-0 z-20">
        {/* Zone 1: Brand — doubles as home button */}
        <button
          onClick={inputFile ? reset : undefined}
          className={`flex items-center gap-2 flex-shrink-0 ${inputFile ? "cursor-pointer group" : "cursor-default"}`}
          title={inputFile ? "Return to home" : undefined}
        >
          <LogoMark className="h-[22px] w-[22px] text-primary transition-[filter] duration-150 group-hover:drop-shadow-[0_0_6px_oklch(0.78_0.16_75_/_0.5)]" />
          <span className="font-mono text-sm font-bold tracking-tight text-primary">
            r3sizer
          </span>
        </button>

        <div className="h-4 w-px bg-border/40 flex-shrink-0 hidden sm:block" />

        {/* Zone 2: Context — filename or tagline */}
        {inputFile ? (
          <span className="text-xs font-mono text-muted-foreground truncate min-w-0 hidden md:block">
            {inputFile.name}
          </span>
        ) : (
          <span className="text-xs font-mono text-muted-foreground/50 hidden sm:block">
            precision downscaling
          </span>
        )}

        <div className="flex-1" />

        {/* Zone 3: Export controls */}
        {outputRgbaData && <DownloadButton />}

        {/* Zone 4: File actions */}
        {inputFile && (
          <>
            <div className="h-4 w-px bg-border/40 flex-shrink-0" />
            <Button
              variant="ghost"
              size="sm"
              onClick={() => fileInputRef.current?.click()}
              className="text-muted-foreground hover:text-foreground flex-shrink-0"
              title="Open another image"
            >
              <FolderOpen className="h-3.5 w-3.5" />
            </Button>
          </>
        )}
      </header>

      {/* Body: params sidebar | center | diagnostics sidebar */}
      <div className="flex-1 flex overflow-hidden">
        {/* Mobile backdrop — params */}
        {inputFile && sidebarOpen && (
          <div
            className="fixed inset-0 z-30 bg-black/50 lg:hidden"
            onClick={() => setSidebarOpen(false)}
          />
        )}
        {/* Mobile backdrop — diagnostics */}
        {inputFile && diagOpen && (
          <div
            className="fixed inset-0 z-30 bg-black/50 lg:hidden"
            onClick={() => setDiagOpen(false)}
          />
        )}

        {/* Parameters sidebar — left */}
        {inputFile && (
          <aside
            className={[
              "bg-card border-r border-border/40",
              /* Mobile: fixed overlay, slides from left */
              "fixed top-0 bottom-0 left-0 z-40 w-[min(340px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              sidebarOpen ? "translate-x-0" : "-translate-x-full",
              /* Desktop: inline sidebar, width transitions */
              "lg:static lg:z-auto lg:shadow-none lg:translate-x-0",
              "lg:flex-shrink-0 lg:overflow-hidden",
              sidebarOpen ? "lg:w-[340px]" : "lg:w-11",
            ].join(" ")}
          >
            <div className="w-full lg:w-[340px] h-full flex flex-col">
              <div className="sticky top-0 z-10 flex items-center gap-2 px-2.5 pt-3 pb-2 bg-card border-b border-border/30">
                <button
                  onClick={() => setSidebarOpen(!sidebarOpen)}
                  className="p-1 rounded-md text-muted-foreground hover:text-primary hover:bg-accent transition-colors flex-shrink-0"
                  title={sidebarOpen ? "Collapse panel" : "Show parameters"}
                >
                  {sidebarOpen ? <ChevronLeft className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                </button>
                <span className={`text-sm font-mono font-semibold text-foreground/80 tracking-tight whitespace-nowrap transition-opacity duration-150 ${sidebarOpen ? "opacity-100" : "lg:opacity-0"}`}>
                  Parameters
                </span>
              </div>
              <div className={`flex-1 overflow-y-auto transition-opacity duration-150 ${sidebarOpen ? "opacity-100" : "lg:opacity-0"}`}>
                <ParameterPanel />
              </div>
            </div>
          </aside>
        )}

        {/* Center column */}
        <div className="relative flex-1 flex flex-col min-w-0 overflow-hidden">
          {/* Processing overlay — covers entire center column */}
          {isProcessing && (
            <div className="absolute inset-0 z-20 flex items-center justify-center bg-background/60 backdrop-blur-[2px]">
              <div className="flex flex-col items-center gap-2.5">
                <Loader2 className="h-6 w-6 animate-spin text-primary" />
                <span className="text-xs font-mono text-primary/80 tracking-wide">
                  running pipeline...
                </span>
              </div>
            </div>
          )}
          {/* Toolbar — above image */}
          {inputFile && (
            <div className="px-4 py-2 border-b border-border/30 flex items-center gap-2 flex-shrink-0">
              {paramsChanged && (
                <div className="w-2 h-2 rounded-full bg-primary animate-pulse" title="Parameters changed" />
              )}
              <Button
                onClick={process}
                disabled={isProcessing}
                className={paramsChanged ? "glow-amber border border-primary/40" : "glow-amber"}
                size="sm"
              >
                {isProcessing && (
                  <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                )}
                {isProcessing
                  ? "Processing..."
                  : paramsChanged
                    ? "Reprocess"
                    : "Process"}
              </Button>
              <span className={`text-[11px] font-mono hidden sm:inline ${paramsChanged ? "text-primary/80" : "text-muted-foreground"}`}>
                {isProcessing
                  ? "running pipeline..."
                  : paramsChanged
                    ? "parameters changed"
                    : "auto-sharpness downscale"}
              </span>
              {/* Mobile panel toggles — hidden on desktop where inline strips exist */}
              <div className="flex items-center gap-1 ml-auto lg:hidden">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSidebarOpen(true)}
                  className="text-muted-foreground hover:text-primary"
                  title="Parameters"
                >
                  <SlidersHorizontal className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setDiagOpen(true)}
                  className="text-muted-foreground hover:text-primary"
                  title="Diagnostics"
                >
                  <BarChart3 className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          )}

          {/* Error banner */}
          {error && (
            <div className="px-4 py-2 bg-destructive/10 border-b border-destructive/20 flex-shrink-0">
              <span className="text-destructive text-xs font-mono">{error}</span>
            </div>
          )}

          {/* Image panel — full height */}
          <div className="flex-1 flex overflow-hidden">
            {!inputFile ? (
              <div className="flex-1 flex items-center justify-center px-6">
                <div className="flex flex-col items-center gap-8 max-w-lg w-full -mt-10">
                  {/* Hero title */}
                  <div className="flex flex-col items-center gap-2 text-center">
                    <h2 className="font-mono text-3xl font-bold tracking-tight text-primary glow-amber-text">
                      r3sizer
                    </h2>
                    <p className="text-sm text-muted-foreground max-w-xs">
                      Precision downscaling with automatic sharpness optimization.
                      Runs entirely in your browser.
                    </p>
                  </div>
                  {/* Upload zone with crop marks */}
                  <div className="relative w-full">
                    {/* Corner crop marks */}
                    <div className="absolute -top-2 -left-2 w-5 h-5 border-t-2 border-l-2 border-primary/40 rounded-tl-sm" />
                    <div className="absolute -top-2 -right-2 w-5 h-5 border-t-2 border-r-2 border-primary/40 rounded-tr-sm" />
                    <div className="absolute -bottom-2 -left-2 w-5 h-5 border-b-2 border-l-2 border-primary/40 rounded-bl-sm" />
                    <div className="absolute -bottom-2 -right-2 w-5 h-5 border-b-2 border-r-2 border-primary/40 rounded-br-sm" />
                    <ImageUpload />
                  </div>
                  {/* Pipeline hint — hidden on narrow screens */}
                  <div className="hidden sm:flex items-center gap-4 text-[11px] font-mono text-muted-foreground/50">
                    <span>linearize</span>
                    <span className="text-primary/30">&rarr;</span>
                    <span>downscale</span>
                    <span className="text-primary/30">&rarr;</span>
                    <span>sharpen</span>
                    <span className="text-primary/30">&rarr;</span>
                    <span>optimize</span>
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex-1 flex flex-col px-3 py-1 min-w-0 overflow-hidden">
                <ImagePreview />
              </div>
            )}
          </div>

        </div>

        {/* Diagnostics sidebar — right, always present when image loaded */}
        {inputFile && (
          <aside
            className={[
              "bg-card border-l border-border/40",
              /* Mobile: fixed overlay, slides from right */
              "fixed top-0 bottom-0 right-0 z-40 w-[min(380px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              diagOpen ? "translate-x-0" : "translate-x-full",
              /* Desktop: inline sidebar, width transitions */
              "lg:static lg:z-auto lg:shadow-none lg:translate-x-0",
              "lg:flex-shrink-0 lg:overflow-hidden",
              diagOpen ? "lg:w-[380px]" : "lg:w-11",
            ].join(" ")}
          >
            <div className="w-full lg:w-[380px] h-full flex flex-col">
              <div className="sticky top-0 z-10 flex items-center gap-2 px-2.5 pt-3 pb-2 bg-card border-b border-border/30">
                <button
                  onClick={() => setDiagOpen(!diagOpen)}
                  className="p-1 rounded-md text-muted-foreground hover:text-primary hover:bg-accent transition-colors flex-shrink-0"
                  title={diagOpen ? "Collapse panel" : "Show diagnostics"}
                >
                  {diagOpen ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
                </button>
                <span className={`text-sm font-mono font-semibold text-foreground/80 tracking-tight whitespace-nowrap transition-opacity duration-150 ${diagOpen ? "opacity-100" : "lg:opacity-0"}`}>
                  Diagnostics
                </span>
              </div>
              <div className={`flex-1 overflow-y-auto transition-opacity duration-150 ${diagOpen ? "opacity-100" : "lg:opacity-0"}`}>
                {diagnostics ? (
                  <DiagnosticsPanel />
                ) : (
                  <div className="flex flex-col items-center justify-center gap-3 px-6 py-16 text-center">
                    <div className="rounded-full p-3 bg-surface text-muted-foreground/30">
                      <BarChart3 className="h-5 w-5" />
                    </div>
                    <div className="space-y-1">
                      <p className="text-sm text-muted-foreground/50">No diagnostics yet</p>
                      <p className="text-[11px] font-mono text-muted-foreground/30">
                        Process an image to see pipeline results
                      </p>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </aside>
        )}
      </div>

      {/* Status bar — always visible */}
      <footer className="border-t border-border/60 px-5 py-2.5 flex items-center gap-5 text-xs font-mono text-muted-foreground bg-background/80 backdrop-blur-sm">
        {diagnostics ? (
          <>
            <span>
              s* = <span className="text-foreground">{diagnostics.selected_strength.toFixed(4)}</span>
            </span>
            <span>
              P = <span className="text-foreground">{diagnostics.measured_artifact_ratio.toExponential(2)}</span>
            </span>
            <span>
              {diagnostics.output_size.width}&times;{diagnostics.output_size.height}
            </span>
            <span className="ml-auto">
              {(diagnostics.timing.total_us / 1000).toFixed(0)}ms
            </span>
          </>
        ) : (
          <span className="text-muted-foreground/40">ready</span>
        )}
      </footer>
    </div>
  );
}
