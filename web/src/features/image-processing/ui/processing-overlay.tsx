import { Loader2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useTranslation } from "react-i18next";

export function ProcessingOverlay({
  stage,
  overall,
  onCancel,
}: {
  stage: string | null;
  overall: number;
  onCancel: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-background/60 backdrop-blur-[2px]">
      <div className="flex w-56 flex-col items-center gap-2.5">
        <Loader2 className="h-6 w-6 animate-spin text-primary" />
        <AnimatePresence mode="popLayout">
          <motion.span
            key={stage}
            initial={{ opacity: 0, filter: "blur(4px)", y: 4 }}
            animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
            exit={{ opacity: 0, filter: "blur(4px)", y: -4 }}
            transition={{ duration: 0.15, ease: "easeOut" }}
            className="text-sm font-mono text-primary/80 tracking-wide"
          >
            {stage ? t(`processing.stages.${stage}`) : t("processing.starting")}
          </motion.span>
        </AnimatePresence>
        <div className="h-1 w-full overflow-hidden rounded-full bg-border/40">
          <div
            className="h-full bg-primary transition-[width] duration-200"
            style={{ width: `${Math.round(overall * 100)}%` }}
          />
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="mt-1 text-[11px] font-mono uppercase tracking-widest text-muted-foreground/60 transition-colors hover:text-foreground"
        >
          {t("processing.cancel")}
        </button>
      </div>
    </div>
  );
}
