import { Button } from "@/components/ui/button";
import { Loader2, FolderOpen, SlidersHorizontal, BarChart3, Play, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { DownloadButton } from "@/components/DownloadButton";

export function Toolbar({
  isProcessing,
  processingStage,
  paramsChanged,
  hasOutput,
  onProcess,
  onOpenFile,
  onShowParams,
  onShowDiag,
}: {
  isProcessing: boolean;
  processingStage: string | null;
  paramsChanged: boolean;
  hasOutput: boolean;
  onProcess: () => void;
  onOpenFile: () => void;
  onShowParams: () => void;
  onShowDiag: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="px-4 py-2 border-b border-border/30 flex items-center gap-2 flex-shrink-0">
      {paramsChanged && (
        <div className="w-2 h-2 rounded-full bg-primary animate-pulse" title={t("toolbar.paramsChanged")} />
      )}
      <Button
        onClick={onProcess}
        disabled={isProcessing}
        className={paramsChanged ? "glow-amber border border-primary/40" : "glow-amber"}
        size="sm"
      >
        {isProcessing ? (
          <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
        ) : paramsChanged ? (
          <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
        ) : (
          <Play className="h-3.5 w-3.5 mr-1.5" />
        )}
        {isProcessing
          ? t("toolbar.processing")
          : paramsChanged
            ? t("toolbar.reprocess")
            : t("toolbar.process")}
      </Button>
      <span className={`text-[11px] font-mono hidden sm:inline ${paramsChanged ? "text-primary/80" : "text-muted-foreground"}`}>
        {isProcessing
          ? processingStage || t("toolbar.starting")
          : paramsChanged
            ? t("toolbar.paramsChanged")
            : t("toolbar.autoSharpness")}
      </span>
      <div className="flex items-center gap-1 ml-auto">
        {hasOutput && <DownloadButton />}
        <Button
          variant="ghost"
          size="sm"
          onClick={onOpenFile}
          className="text-muted-foreground hover:text-foreground"
          title={t("toolbar.openFile")}
        >
          <FolderOpen className="h-3.5 w-3.5" />
        </Button>
        <div className="w-px h-4 bg-border/30 lg:hidden" />
        <Button
          variant="ghost"
          size="sm"
          onClick={onShowParams}
          className="text-muted-foreground hover:text-primary lg:hidden"
          title={t("toolbar.parameters")}
        >
          <SlidersHorizontal className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={onShowDiag}
          className="text-muted-foreground hover:text-primary lg:hidden"
          title={t("toolbar.diagnostics")}
        >
          <BarChart3 className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}
