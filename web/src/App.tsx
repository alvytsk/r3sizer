import { useState, useRef, useCallback } from "react";
import { ChevronLeft, ChevronRight, BarChart3, AlertTriangle } from "lucide-react";
import { ImagePreview } from "@/components/ImagePreview";
import { ParameterPanel } from "@/components/ParameterPanel";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { AppHeader } from "@/components/AppHeader";
import { Toolbar } from "@/components/Toolbar";
import { ProcessingOverlay } from "@/components/ProcessingOverlay";
import { WelcomeHero } from "@/components/WelcomeHero";
import { StatusBar } from "@/components/StatusBar";
import { useProcessorStore } from "@/stores/processor-store";
import { loadImageAsRgba } from "@/lib/image-loader";

const ACCEPTED = ".png,.jpg,.jpeg,.bmp,.webp,.gif,.tiff";

export default function App() {
  const inputFile = useProcessorStore((s) => s.inputFile);
  const isProcessing = useProcessorStore((s) => s.isProcessing);
  const processingStage = useProcessorStore((s) => s.processingStage);
  const error = useProcessorStore((s) => s.error);
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  const outputRgbaData = useProcessorStore((s) => s.outputRgbaData);
  const paramsVersion = useProcessorStore((s) => s.paramsVersion);
  const lastProcessedVersion = useProcessorStore((s) => s.lastProcessedVersion);
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

  const paramsChanged = !!(outputRgbaData && paramsVersion !== lastProcessedVersion);

  const handleOpenFile = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      loadImageAsRgba(file).then(({ data, width, height }) => {
        setInput(file, data, width, height);
      });
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

      <AppHeader
        onLogoClick={reset}
        showLogoAction={!!inputFile}
      />

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
              "fixed top-0 bottom-0 left-0 z-40 w-[min(300px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              sidebarOpen ? "translate-x-0" : "-translate-x-full",
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
                <ErrorBoundary panel="Parameters">
                  <ParameterPanel />
                </ErrorBoundary>
              </div>
            </div>
          </aside>
        )}

        {/* Center column */}
        <div className="relative flex-1 flex flex-col min-w-0 overflow-hidden">
          {isProcessing && <ProcessingOverlay stage={processingStage} />}

          {inputFile && (
            <Toolbar
              isProcessing={isProcessing}
              processingStage={processingStage}
              paramsChanged={paramsChanged}
              hasOutput={!!outputRgbaData}
              onProcess={process}
              onOpenFile={() => fileInputRef.current?.click()}
              onShowParams={() => setSidebarOpen(true)}
              onShowDiag={() => setDiagOpen(true)}
            />
          )}

          {/* Error banner */}
          {error && (
            <div className="px-4 py-2 bg-destructive/10 border-b border-destructive/20 flex-shrink-0 flex items-center gap-2">
              <AlertTriangle className="h-3.5 w-3.5 text-destructive flex-shrink-0" />
              <span className="text-destructive text-xs font-mono">{error}</span>
            </div>
          )}

          {/* Image panel — full height */}
          <div className="flex-1 flex overflow-hidden">
            {!inputFile ? (
              <WelcomeHero />
            ) : (
              <div className="flex-1 flex flex-col px-3 py-2 min-w-0 overflow-hidden">
                <ImagePreview />
              </div>
            )}
          </div>
        </div>

        {/* Diagnostics sidebar — right */}
        {inputFile && (
          <aside
            className={[
              "bg-card border-l border-border/40",
              "fixed top-0 bottom-0 right-0 z-40 w-[min(420px,85vw)] shadow-2xl",
              "transition-[transform,width] duration-200 ease-in-out",
              diagOpen ? "translate-x-0" : "translate-x-full",
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
                  <ErrorBoundary panel="Diagnostics">
                    <DiagnosticsPanel />
                  </ErrorBoundary>
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

      <StatusBar diagnostics={diagnostics} />
    </div>
  );
}
