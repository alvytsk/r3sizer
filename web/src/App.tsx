import { useState, useRef, useCallback } from "react";
import { Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { Loader2, FolderOpen, ChevronLeft, ChevronRight, BarChart3, SlidersHorizontal, Lock, BookOpen } from "lucide-react";
import { DownloadButton } from "@/components/DownloadButton";
import { ImageUpload } from "@/components/ImageUpload";
import { ImagePreview } from "@/components/ImagePreview";
import { ParameterPanel } from "@/components/ParameterPanel";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { useProcessorStore } from "@/stores/processor-store";
import { motion, AnimatePresence } from "motion/react";
import BlurText from "@/components/BlurText";
import ShinyText from "@/components/ShinyText";

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
  const processingStage = useProcessorStore((s) => s.processingStage);
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

      {/* Top bar */}
      <header className="flex flex-col sticky top-0 z-20">
        <div className="px-4 py-2.5 flex items-center gap-3 backdrop-blur-sm bg-background/80">
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

          <div className="h-4 w-px bg-border/40 flex-shrink-0" />

          <Link
            to="/algorithm"
            className="inline-flex items-center gap-1.5 text-xs font-mono text-muted-foreground/60 hover:text-primary transition-colors"
          >
            <BookOpen className="h-3.5 w-3.5" />
            <span className="hidden sm:inline">Algorithm</span>
          </Link>

          <a
            href="https://github.com/alvytsk/r3sizer"
            target="_blank"
            rel="noopener noreferrer"
            className="ml-auto text-muted-foreground/60 hover:text-primary transition-colors"
            title="View on GitHub"
          >
            <svg viewBox="0 0 16 16" fill="currentColor" className="h-4 w-4">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
            </svg>
          </a>
        </div>
        {/* Accent line — amber gradient spatial anchor */}
        <div className="h-px accent-line" />
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
              "bg-background border-r border-border/40",
              /* Mobile: fixed overlay, slides from left */
              "fixed top-0 bottom-0 left-0 z-40 w-[min(300px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              sidebarOpen ? "translate-x-0" : "-translate-x-full",
              /* Desktop: inline sidebar, width transitions */
              "lg:static lg:z-auto lg:shadow-none lg:translate-x-0",
              "lg:flex-shrink-0 lg:overflow-hidden",
              sidebarOpen ? "lg:w-[300px]" : "lg:w-11",
            ].join(" ")}
          >
            <div className="w-full lg:w-[300px] h-full flex flex-col">
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
                <AnimatePresence mode="popLayout">
                  <motion.span
                    key={processingStage}
                    initial={{ opacity: 0, filter: "blur(4px)", y: 4 }}
                    animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
                    exit={{ opacity: 0, filter: "blur(4px)", y: -4 }}
                    transition={{ duration: 0.15, ease: "easeOut" }}
                    className="text-sm font-mono text-primary/80 tracking-wide"
                  >
                    {processingStage || "starting..."}
                  </motion.span>
                </AnimatePresence>
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
                  ? processingStage || "starting..."
                  : paramsChanged
                    ? "parameters changed"
                    : "auto-sharpness downscale"}
              </span>
              <div className="flex items-center gap-1 ml-auto">
                {outputRgbaData && <DownloadButton />}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => fileInputRef.current?.click()}
                  className="text-muted-foreground hover:text-foreground"
                  title="Open another image"
                >
                  <FolderOpen className="h-3.5 w-3.5" />
                </Button>
                <div className="w-px h-4 bg-border/30 lg:hidden" />
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSidebarOpen(true)}
                  className="text-muted-foreground hover:text-primary lg:hidden"
                  title="Parameters"
                >
                  <SlidersHorizontal className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setDiagOpen(true)}
                  className="text-muted-foreground hover:text-primary lg:hidden"
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
                  {/* Hero logo + title */}
                  <div className="flex flex-col items-center gap-4 text-center">
                    <div className="animate-fade-up glow-amber rounded-2xl p-2">
                      <LogoMark className="h-16 w-16 text-primary" />
                    </div>
                    <div className="flex flex-col items-center gap-2.5 animate-fade-up delay-100">
                      <h2 className="font-mono text-3xl font-bold tracking-tight text-primary glow-amber-text">
                        r3sizer
                      </h2>
                      <BlurText
                        text="Precision downscaling with automatic sharpness optimization."
                        className="text-sm text-muted-foreground max-w-xs justify-center"
                        animateBy="words"
                        direction="bottom"
                        delay={80}
                        stepDuration={0.4}
                        animationFrom={{ filter: 'blur(12px)', opacity: 0, y: 12 }}
                        animationTo={[
                          { filter: 'blur(4px)', opacity: 0.4, y: -2 },
                          { filter: 'blur(0px)', opacity: 1, y: 0 }
                        ]}
                        easing={[0.16, 1, 0.3, 1]}
                      />
                      <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full border border-primary/25 bg-primary/[0.06] text-xs font-mono tracking-wide">
                        <Lock className="h-3 w-3 text-primary/80" />
                        <ShinyText
                          text="Runs entirely in your browser"
                          speed={3.5}
                          delay={1}
                          color="oklch(0.64 0.015 80)"
                          shineColor="oklch(0.88 0.14 80)"
                          className="text-xs font-mono tracking-wide"
                        />
                      </span>
                    </div>
                  </div>
                  {/* Upload zone with crop marks */}
                  <div className="relative w-full animate-fade-up delay-300">
                    {/* Corner crop marks */}
                    <div className="absolute -top-2 -left-2 w-5 h-5 border-t-2 border-l-2 border-primary/40 rounded-tl-sm" />
                    <div className="absolute -top-2 -right-2 w-5 h-5 border-t-2 border-r-2 border-primary/40 rounded-tr-sm" />
                    <div className="absolute -bottom-2 -left-2 w-5 h-5 border-b-2 border-l-2 border-primary/40 rounded-bl-sm" />
                    <div className="absolute -bottom-2 -right-2 w-5 h-5 border-b-2 border-r-2 border-primary/40 rounded-br-sm" />
                    <ImageUpload />
                  </div>
                  {/* Pipeline hint — hidden on narrow screens */}
                  <div className="hidden sm:flex items-center gap-4 text-[11px] font-mono text-muted-foreground/50 animate-fade-up delay-400">
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
              <div className="flex-1 flex flex-col px-3 py-2 min-w-0 overflow-hidden">
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
              "fixed top-0 bottom-0 right-0 z-40 w-[min(420px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              diagOpen ? "translate-x-0" : "translate-x-full",
              /* Desktop: inline sidebar, width transitions */
              "lg:static lg:z-auto lg:shadow-none lg:translate-x-0",
              "lg:flex-shrink-0 lg:overflow-hidden",
              diagOpen ? "lg:w-[420px]" : "lg:w-10",
            ].join(" ")}
          >
            <div className="w-full lg:w-[420px] h-full flex flex-col">
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

      {/* Status bar — instrument telemetry strip */}
      <footer className="footer-separator border-t border-border/40 px-5 flex items-center gap-5 bg-background/90 backdrop-blur-sm flex-shrink-0 h-9">
        {diagnostics ? (
          <>
            <span className="flex items-center gap-1.5">
              <span className={`led ${diagnostics.selection_mode === "polynomial_root" ? "led-green" : "led-amber"}`} />
              <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">S*</span>
              <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.selected_strength.toFixed(4)}</span>
            </span>
            <span className="w-px h-3 bg-border/30 flex-shrink-0" />
            <span className="flex items-center gap-1.5">
              <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">P</span>
              <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.measured_artifact_ratio.toExponential(2)}</span>
            </span>
            <span className="w-px h-3 bg-border/30 flex-shrink-0" />
            <span className="flex items-center gap-1.5">
              <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Out</span>
              <span className="text-[11px] font-mono text-foreground tabular-nums">{diagnostics.output_size.width}&times;{diagnostics.output_size.height}</span>
            </span>
            <span className="ml-auto flex items-center gap-1.5">
              <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/50">Total</span>
              <span className="text-[11px] font-mono text-primary tabular-nums">{(diagnostics.timing.total_us / 1000).toFixed(0)}ms</span>
            </span>
          </>
        ) : (
          <span className="flex items-center gap-2">
            <span className="led led-dim" />
            <span className="text-[9px] font-mono tracking-[0.15em] uppercase text-muted-foreground/30">ready</span>
          </span>
        )}
      </footer>
    </div>
  );
}
