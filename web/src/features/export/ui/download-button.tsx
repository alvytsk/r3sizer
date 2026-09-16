import { Download } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useExportPrefsStore } from "@/entities/export-preferences";
import { useOutputStore } from "@/entities/outputs";
import { Button } from "@/shared/ui/button";
import { type ExportSnapshot, exportImage } from "../model/export-image";
import { type ExportMessageState, MetadataMessage } from "./metadata-message";

export function DownloadButton() {
  const { t } = useTranslation();
  const outputRgbaData = useOutputStore((s) => s.outputRgbaData);
  const outputWidth = useOutputStore((s) => s.outputWidth);
  const outputHeight = useOutputStore((s) => s.outputHeight);
  const outputSourceFile = useOutputStore((s) => s.outputSourceFile);
  const format = useExportPrefsStore((s) => s.exportFormat);
  const quality = useExportPrefsStore((s) => s.exportQuality);
  const setFormat = useExportPrefsStore((s) => s.setExportFormat);
  const setQuality = useExportPrefsStore((s) => s.setExportQuality);

  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<ExportMessageState | null>(null);
  // Synchronous duplicate-click guard (state updates are not synchronous enough).
  const pendingRef = useRef(false);
  // Bumped whenever the output this button would export changes, so a
  // still-running export from before the change can detect it's stale.
  const generationRef = useRef(0);
  const outputRef = useRef({ outputSourceFile, outputRgbaData, outputWidth, outputHeight });

  useEffect(() => {
    const prev = outputRef.current;
    if (
      prev.outputSourceFile !== outputSourceFile ||
      prev.outputRgbaData !== outputRgbaData ||
      prev.outputWidth !== outputWidth ||
      prev.outputHeight !== outputHeight
    ) {
      outputRef.current = { outputSourceFile, outputRgbaData, outputWidth, outputHeight };
      generationRef.current += 1;
      pendingRef.current = false;
      setPending(false);
      setMessage(null);
    }
  }, [outputSourceFile, outputRgbaData, outputWidth, outputHeight]);

  // The output-change effect above only runs while this component stays
  // mounted. The toolbar unmounts it entirely (`hasOutput && <DownloadButton
  // />`) as soon as the output is cleared — e.g. opening a new file, which
  // calls clearOutput() before this effect ever gets a chance to react. A
  // still-running export from before that point must not resume calling
  // setState (or downloading) on a dead instance, so bump the generation on
  // unmount too.
  useEffect(() => {
    return () => {
      generationRef.current += 1;
    };
  }, []);

  const handleDownload = useCallback(() => {
    if (!outputRgbaData || !outputSourceFile || pendingRef.current) return;

    pendingRef.current = true;
    setPending(true);
    setMessage({ kind: "pending" });
    const generation = generationRef.current;
    const requestSourceFile = outputSourceFile;
    const requestRgba = outputRgbaData;

    const snapshot: ExportSnapshot = {
      sourceFile: outputSourceFile,
      rgba: outputRgbaData,
      width: outputWidth,
      height: outputHeight,
      format,
      quality,
    };

    // Independent of this component's lifetime: even if the button has
    // since unmounted (generationRef frozen at whatever it last was), the
    // output store itself is the ground truth for whether this request's
    // result still corresponds to what's on screen.
    const isStale = () => {
      if (generationRef.current !== generation) return true;
      const current = useOutputStore.getState();
      return (
        current.outputSourceFile !== requestSourceFile || current.outputRgbaData !== requestRgba
      );
    };

    exportImage(snapshot)
      .then(({ blob, report, filename }) => {
        if (isStale()) return; // output changed (or this button unmounted) mid-export; discard
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        a.click();
        // Deferred so the browser has dispatched the download before the URL dies.
        setTimeout(() => URL.revokeObjectURL(url), 0);
        setMessage(report.issues.length > 0 ? { kind: "report", report } : null);
      })
      .catch(() => {
        if (isStale()) return;
        setMessage({ kind: "error" });
      })
      .finally(() => {
        if (isStale()) return;
        pendingRef.current = false;
        setPending(false);
      });
  }, [outputRgbaData, outputSourceFile, outputWidth, outputHeight, format, quality]);

  if (!outputRgbaData) return null;

  const isLossy = format !== "png";
  const qualityPresets = [
    { label: t("download.max"), value: 100 },
    { label: t("download.high"), value: 90 },
    { label: t("download.std"), value: 80 },
    { label: t("download.low"), value: 60 },
  ] as const;

  return (
    <div className="flex flex-col gap-1">
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
              {qualityPresets.map((preset) => (
                <button
                  key={preset.value}
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
              {t("download.lossless")}
            </span>
          )}
        </div>
        <Button
          size="sm"
          onClick={handleDownload}
          disabled={pending}
          title={
            isLossy
              ? t("download.saveAsQuality", { format: format.toUpperCase(), quality })
              : t("download.saveAsLossless", { format: format.toUpperCase() })
          }
        >
          <Download className="h-3.5 w-3.5 mr-1" />
          {/* Below lg: show format in button since selector is hidden */}
          <span className="lg:hidden">{format.toUpperCase()}</span>
          <span className="hidden lg:inline">{t("download.save")}</span>
        </Button>
      </div>
      <MetadataMessage state={message} />
    </div>
  );
}
