import { useState, useCallback, useRef } from "react";
import { Input } from "@/components/ui/input";

export function NumericInput({
  value,
  min,
  step = 1,
  onCommit,
  id,
  className,
}: {
  value: number;
  min?: number;
  step?: number;
  onCommit: (v: number) => void;
  id?: string;
  className?: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const editing = draft !== null;
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const valueRef = useRef(value);
  valueRef.current = value;

  const clamp = useCallback(
    (n: number) => Math.max(min ?? -Infinity, n),
    [min]
  );

  const commit = useCallback(() => {
    if (draft === null) return;
    const n = parseInt(draft, 10);
    if (!isNaN(n)) onCommit(clamp(n));
    setDraft(null);
  }, [draft, clamp, onCommit]);

  const nudge = useCallback(
    (dir: 1 | -1) => onCommit(clamp(value + step * dir)),
    [value, step, clamp, onCommit]
  );

  const stopRepeat = useCallback(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    document.removeEventListener("pointerup", stopRepeat);
    document.removeEventListener("pointercancel", stopRepeat);
  }, []);

  const startRepeat = useCallback(
    (dir: 1 | -1) => {
      nudge(dir);
      let count = 0;
      const tick = () => {
        count++;
        const s = count > 6 ? step * 10 : step;
        onCommit(clamp(valueRef.current + s * dir));
      };
      const timeout = setTimeout(() => {
        tick();
        intervalRef.current = setInterval(tick, 100);
      }, 400);
      intervalRef.current = timeout as unknown as ReturnType<typeof setInterval>;
      document.addEventListener("pointerup", stopRepeat);
      document.addEventListener("pointercancel", stopRepeat);
    },
    [nudge, step, clamp, onCommit, stopRepeat]
  );

  const chevron = (
    <svg width="8" height="5" viewBox="0 0 8 5" fill="none">
      <path
        d="M1 1.5L4 3.5L7 1.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );

  return (
    <div className="relative group">
      <Input
        id={id}
        inputMode="numeric"
        className={`${className ?? ""} pr-6`}
        value={editing ? draft : String(value)}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => {
          setDraft(String(value));
          e.target.select();
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp") {
            e.preventDefault();
            nudge(1);
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            nudge(-1);
          } else if (e.key === "Enter") {
            commit();
            (e.target as HTMLInputElement).blur();
          } else if (e.key === "Escape") {
            setDraft(null);
            (e.target as HTMLInputElement).blur();
          }
        }}
      />
      <div className="absolute right-px top-px bottom-px w-5 flex flex-col rounded-r-lg overflow-hidden opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity">
        <button
          type="button"
          tabIndex={-1}
          className="flex-1 flex items-center justify-center text-muted-foreground/50 hover:text-primary hover:bg-primary/10 transition-colors rotate-180"
          onPointerDown={(e) => {
            e.preventDefault();
            startRepeat(1);
          }}
        >
          {chevron}
        </button>
        <div className="h-px bg-border/40" />
        <button
          type="button"
          tabIndex={-1}
          className="flex-1 flex items-center justify-center text-muted-foreground/50 hover:text-primary hover:bg-primary/10 transition-colors"
          onPointerDown={(e) => {
            e.preventDefault();
            startRepeat(-1);
          }}
        >
          {chevron}
        </button>
      </div>
    </div>
  );
}
