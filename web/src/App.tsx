import { useState, useRef, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Loader2, RotateCcw, FolderOpen, ChevronLeft, ChevronRight } from "lucide-react";
import { DownloadButton } from "@/components/DownloadButton";
import { ImageUpload } from "@/components/ImageUpload";
import { ImagePreview } from "@/components/ImagePreview";
import { ParameterPanel } from "@/components/ParameterPanel";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { useProcessorStore } from "@/stores/processor-store";

const ACCEPTED = ".png,.jpg,.jpeg,.bmp,.webp,.gif,.tiff";

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
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [diagOpen, setDiagOpen] = useState(true);
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
    <div className="min-h-screen flex flex-col relative grain">
      {/* Hidden file input for "Open" */}
      <input
        ref={fileInputRef}
        type="file"
        accept={ACCEPTED}
        className="hidden"
        onChange={handleOpenFile}
      />

      {/* Top bar */}
      <header className="border-b border-border/60 px-5 py-2.5 flex items-center justify-between backdrop-blur-sm bg-background/80 sticky top-0 z-20">
        <div className="flex items-center gap-3">
          <h1 className="font-mono text-base font-bold tracking-tight text-primary">
            r3sizer
          </h1>
          <span className="hidden sm:block text-[11px] font-mono text-muted-foreground/70 border-l border-border pl-3">
            precision downscaling
          </span>
        </div>
      </header>

      {/* Body: params sidebar | center | diagnostics sidebar */}
      <div className="flex-1 flex overflow-hidden">
        {/* Parameters sidebar — left */}
        {inputFile && (
          <aside className="border-r border-border/40 bg-card flex-shrink-0">
            {sidebarOpen ? (
              <div className="w-[340px] h-full overflow-y-auto">
                <div className="sticky top-0 z-10 flex items-center justify-between px-3 pt-3 pb-2 bg-card border-b border-border/30">
                  <span className="text-sm font-mono font-semibold text-foreground/80 tracking-tight">
                    Parameters
                  </span>
                  <button
                    onClick={() => setSidebarOpen(false)}
                    className="p-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                    title="Collapse panel"
                  >
                    <ChevronLeft className="h-4 w-4" />
                  </button>
                </div>
                <ParameterPanel />
              </div>
            ) : (
              <div className="w-10 h-full flex flex-col items-center pt-3">
                <button
                  onClick={() => setSidebarOpen(true)}
                  className="p-1.5 rounded-md text-muted-foreground hover:text-primary hover:bg-accent transition-colors"
                  title="Show parameters"
                >
                  <ChevronRight className="h-4 w-4" />
                </button>
              </div>
            )}
          </aside>
        )}

        {/* Center column */}
        <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
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
              <span className={`text-[11px] font-mono ${paramsChanged ? "text-primary/80" : "text-muted-foreground"}`}>
                {isProcessing
                  ? "running pipeline..."
                  : paramsChanged
                    ? "parameters changed"
                    : "auto-sharpness downscale"}
              </span>
              <div className="ml-auto flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => fileInputRef.current?.click()}
                  className="text-muted-foreground hover:text-foreground"
                >
                  <FolderOpen className="h-3.5 w-3.5 mr-1" />
                  Open
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={reset}
                  className="text-muted-foreground/50 hover:text-foreground"
                  title="Reset all"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
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
              <div className="flex-1 flex items-start justify-center pt-20 px-4">
                <div className="max-w-md w-full">
                  <ImageUpload />
                </div>
              </div>
            ) : (
              <div className="flex-1 flex flex-col px-3 py-1 min-w-0 overflow-hidden">
                <ImagePreview />
              </div>
            )}
          </div>

          {/* Save bar — bottom of center column */}
          {outputRgbaData && (
            <div className="px-4 py-2 border-t border-border/30 flex-shrink-0">
              <DownloadButton />
            </div>
          )}
        </div>

        {/* Diagnostics sidebar — right */}
        {diagnostics && (
          <aside className="border-l border-border/40 bg-card flex-shrink-0">
            {diagOpen ? (
              <div className="w-[380px] h-full overflow-y-auto">
                <div className="sticky top-0 z-10 flex items-center justify-between px-3 pt-3 pb-2 bg-card border-b border-border/30">
                  <span className="text-sm font-mono font-semibold text-foreground/80 tracking-tight">
                    Diagnostics
                  </span>
                  <button
                    onClick={() => setDiagOpen(false)}
                    className="p-1 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                    title="Collapse panel"
                  >
                    <ChevronRight className="h-4 w-4" />
                  </button>
                </div>
                <DiagnosticsPanel />
              </div>
            ) : (
              <div className="w-10 h-full flex flex-col items-center pt-3">
                <button
                  onClick={() => setDiagOpen(true)}
                  className="p-1.5 rounded-md text-muted-foreground hover:text-primary hover:bg-accent transition-colors"
                  title="Show diagnostics"
                >
                  <ChevronLeft className="h-4 w-4" />
                </button>
              </div>
            )}
          </aside>
        )}
      </div>

      {/* Status bar */}
      {diagnostics && (
        <footer className="border-t border-border/60 px-5 py-1.5 flex items-center gap-4 text-[11px] font-mono text-muted-foreground bg-background/80 backdrop-blur-sm">
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
        </footer>
      )}
    </div>
  );
}
