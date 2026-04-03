import type { ChipVariant } from "./utils";

const CHIP_STYLES: Record<ChipVariant, { border: string; bg: string; text: string }> = {
  ok:      { border: "border-chart-3/25",     bg: "bg-chart-3/5",      text: "text-chart-3"          },
  warn:    { border: "border-primary/25",      bg: "bg-primary/5",      text: "text-primary"          },
  error:   { border: "border-destructive/30",  bg: "bg-destructive/8",  text: "text-destructive"      },
  neutral: { border: "border-border/25",       bg: "bg-background",     text: "text-muted-foreground" },
};

export function Readout({ label, value }: { label: React.ReactNode; value: string | number }) {
  return (
    <div className="flex justify-between text-[13px] py-0.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-mono text-foreground/90">{value}</span>
    </div>
  );
}

export function StatusChip({
  heading,
  value,
  variant,
}: {
  heading: string;
  value: string;
  variant: ChipVariant;
}) {
  const s = CHIP_STYLES[variant];
  return (
    <div className={`flex-1 rounded-sm border px-2.5 py-2 ${s.border} ${s.bg}`}>
      <div className="text-[9px] font-mono uppercase tracking-[0.2em] text-muted-foreground/50 mb-1">
        {heading}
      </div>
      <div className={`text-[12px] font-mono font-medium leading-none ${s.text}`}>
        {value.replace(/_/g, " ")}
      </div>
    </div>
  );
}
