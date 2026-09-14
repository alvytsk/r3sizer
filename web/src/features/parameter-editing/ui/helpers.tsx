import { Info } from "lucide-react";
import { Label } from "@/shared/ui/label";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

export function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-xs font-semibold text-foreground/80 border-b border-border/30 pb-1">
      {children}
    </div>
  );
}

export function ValueLabel({ children, tip }: { children: React.ReactNode; tip?: string }) {
  if (!tip) return <Label className="text-[13px] text-muted-foreground">{children}</Label>;
  return (
    <span className="flex items-center gap-1">
      <Label className="text-[13px] text-muted-foreground">{children}</Label>
      <Tooltip>
        <TooltipTrigger
          render={<span />}
          className="inline-flex text-muted-foreground/40 hover:text-primary transition-colors"
        >
          <Info className="h-3 w-3" />
        </TooltipTrigger>
        <TooltipContent side="right">{tip}</TooltipContent>
      </Tooltip>
    </span>
  );
}

export function SelectedLabel({
  labels,
  value,
}: {
  labels: Record<string, string>;
  value: string;
}) {
  return (
    <span className="flex flex-1 text-left truncate" data-slot="select-value">
      {labels[value] ?? value}
    </span>
  );
}
