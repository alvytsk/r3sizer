import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Loader2, RotateCcw } from "lucide-react";
import { ImageUpload } from "@/components/ImageUpload";
import { ImagePreview } from "@/components/ImagePreview";
import { ParameterPanel } from "@/components/ParameterPanel";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { DownloadButton } from "@/components/DownloadButton";
import { useProcessorStore } from "@/stores/processor-store";

export default function App() {
  const inputFile = useProcessorStore((s) => s.inputFile);
  const isProcessing = useProcessorStore((s) => s.isProcessing);
  const error = useProcessorStore((s) => s.error);
  const diagnostics = useProcessorStore((s) => s.diagnostics);
  const process = useProcessorStore((s) => s.process);
  const reset = useProcessorStore((s) => s.reset);

  return (
    <div className="min-h-screen flex flex-col">
      <header className="border-b px-6 py-3 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">r3sizer</h1>
          <p className="text-xs text-muted-foreground">
            Automatic sharpness-adjusted image downscaling
          </p>
        </div>
        <div className="flex items-center gap-2">
          <DownloadButton />
          {inputFile && (
            <Button variant="ghost" size="sm" onClick={reset}>
              <RotateCcw className="h-4 w-4 mr-1" />
              Reset
            </Button>
          )}
        </div>
      </header>

      <main className="flex-1 p-6">
        {!inputFile ? (
          <div className="max-w-lg mx-auto mt-16">
            <ImageUpload />
          </div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-[1fr_380px] gap-6">
            <div className="space-y-4">
              <ImagePreview />

              {error && (
                <div className="bg-destructive/10 text-destructive text-sm p-3 rounded border border-destructive/20">
                  {error}
                </div>
              )}

              <div className="flex items-center gap-2">
                <Button
                  onClick={process}
                  disabled={isProcessing}
                  className="w-full sm:w-auto"
                >
                  {isProcessing && (
                    <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                  )}
                  {isProcessing ? "Processing..." : "Process Image"}
                </Button>
                <span className="text-xs text-muted-foreground">
                  {isProcessing
                    ? "Running WASM pipeline..."
                    : "Click to run the auto-sharpness downscale pipeline"}
                </span>
              </div>

              {diagnostics && (
                <>
                  <Separator />
                  <DiagnosticsPanel />
                </>
              )}
            </div>

            <div className="space-y-4">
              <ParameterPanel />
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
