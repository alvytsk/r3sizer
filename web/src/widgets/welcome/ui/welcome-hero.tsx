import { ImageIcon, Lock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useImageStore } from "@/entities/images";
import { ImageUpload } from "@/features/image-upload";

export function WelcomeHero() {
  const { t } = useTranslation();
  const setInput = useImageStore((s) => s.setInput);

  const loadSample = async () => {
    const blob = await (await fetch(`${import.meta.env.BASE_URL}sample.jpg`)).blob();
    void setInput(new File([blob], "sample.jpg", { type: "image/jpeg" }));
  };

  return (
    <div className="flex-1 flex items-center justify-center px-6">
      <div className="flex flex-col items-center gap-8 max-w-lg w-full -mt-10 animate-fade-up">
        <div className="flex flex-col items-center gap-3 text-center">
          <h1 className="font-heading text-3xl font-bold tracking-tight text-foreground text-balance">
            {t("app.headline")}
          </h1>
          <p className="text-[15px] leading-relaxed text-muted-foreground max-w-md">
            {t("app.subtitle")}
          </p>
        </div>
        {/* Upload zone with crop marks */}
        <div className="relative w-full">
          <div className="absolute -top-2 -left-2 w-5 h-5 border-t-2 border-l-2 border-primary/40 rounded-tl-sm" />
          <div className="absolute -top-2 -right-2 w-5 h-5 border-t-2 border-r-2 border-primary/40 rounded-tr-sm" />
          <div className="absolute -bottom-2 -left-2 w-5 h-5 border-b-2 border-l-2 border-primary/40 rounded-bl-sm" />
          <div className="absolute -bottom-2 -right-2 w-5 h-5 border-b-2 border-r-2 border-primary/40 rounded-br-sm" />
          <ImageUpload />
        </div>
        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-2 text-xs text-muted-foreground">
          <button
            type="button"
            onClick={loadSample}
            className="inline-flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-foreground/90 transition-colors hover:border-primary/50 hover:text-primary"
          >
            <ImageIcon className="h-3.5 w-3.5" />
            {t("upload.trySample")}
          </button>
          <span className="inline-flex items-center gap-1.5">
            <Lock className="h-3 w-3 text-chart-3" />
            {t("app.badge")}
          </span>
        </div>
      </div>
    </div>
  );
}
