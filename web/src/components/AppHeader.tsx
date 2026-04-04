import { Link } from "@tanstack/react-router";
import { BookOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LogoMark } from "./LogoMark";

export function AppHeader({
  onLogoClick,
  showLogoAction,
}: {
  onLogoClick: (() => void) | undefined;
  showLogoAction: boolean;
}) {
  const { t, i18n } = useTranslation();

  return (
    <header className="flex flex-col sticky top-0 z-20">
      <div className="px-4 py-2.5 flex items-center gap-3 backdrop-blur-sm bg-background/80">
        <button
          onClick={showLogoAction ? onLogoClick : undefined}
          className={`flex items-center gap-2 flex-shrink-0 ${showLogoAction ? "cursor-pointer group" : "cursor-default"}`}
          title={showLogoAction ? t("app.returnHome") : undefined}
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
          <span className="hidden sm:inline">{t("app.algorithm")}</span>
        </Link>

        <div className="flex items-center gap-2 ml-auto">
          <button
            type="button"
            aria-label={i18n.language === "ru" ? "Switch to English" : "Переключить на русский"}
            className="rounded-md border border-border/40 bg-card px-2 py-1 font-mono text-[11px] font-medium text-muted-foreground/60 transition-colors hover:text-primary hover:border-primary/30"
            onClick={() => i18n.changeLanguage(i18n.language === "ru" ? "en" : "ru")}
          >
            {i18n.language === "ru" ? "RU" : "EN"}
          </button>

          <a
            href="https://github.com/alvytsk/r3sizer"
            target="_blank"
            rel="noopener noreferrer"
            className="text-muted-foreground/60 hover:text-primary transition-colors"
            title={t("app.viewOnGithub")}
          >
            <svg viewBox="0 0 16 16" fill="currentColor" className="h-4 w-4">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
            </svg>
          </a>
        </div>
      </div>
      {/* Accent line — amber gradient spatial anchor */}
      <div className="h-px accent-line" />
    </header>
  );
}
