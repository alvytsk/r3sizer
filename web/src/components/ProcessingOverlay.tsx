import { Loader2 } from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { useTranslation } from "react-i18next";

export function ProcessingOverlay({ stage }: { stage: string | null }) {
  const { t } = useTranslation();

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-background/60 backdrop-blur-[2px]">
      <div className="flex flex-col items-center gap-2.5">
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
            {stage || t("processing.starting")}
          </motion.span>
        </AnimatePresence>
      </div>
    </div>
  );
}
