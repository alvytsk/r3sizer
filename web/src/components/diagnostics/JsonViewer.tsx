import { useState, useCallback } from "react";

/** Simple JSON syntax highlighter — no external deps. */
function highlightJson(json: string): string {
  return json.replace(
    /("(?:\\.|[^"\\])*")\s*:/g,
    '<span class="text-sky-400">$1</span>:'
  ).replace(
    /:\s*("(?:\\.|[^"\\])*")/g,
    ': <span class="text-amber-300">$1</span>'
  ).replace(
    /:\s*(-?\d+\.?\d*(?:e[+-]?\d+)?)/gi,
    ': <span class="text-emerald-400">$1</span>'
  ).replace(
    /:\s*(true|false)/g,
    ': <span class="text-violet-400">$1</span>'
  ).replace(
    /:\s*(null)/g,
    ': <span class="text-rose-400/60">$1</span>'
  );
}

export function JsonViewer({ data }: { data: unknown }) {
  const [copied, setCopied] = useState(false);
  const json = JSON.stringify(data, null, 2);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(json).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [json]);

  const handleDownload = useCallback(() => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `r3sizer-diag-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [json]);

  return (
    <div className="space-y-2">
      <div className="flex gap-1.5">
        <button
          onClick={handleCopy}
          className="text-[11px] font-mono px-2 py-0.5 rounded border border-border/40 bg-muted hover:bg-muted/80 text-muted-foreground transition-colors"
        >
          {copied ? "Copied!" : "Copy"}
        </button>
        <button
          onClick={handleDownload}
          className="text-[11px] font-mono px-2 py-0.5 rounded border border-border/40 bg-muted hover:bg-muted/80 text-muted-foreground transition-colors"
        >
          Download
        </button>
      </div>
      <pre
        className="text-[11px] leading-[1.6] font-mono bg-[#0d1117] text-[#c9d1d9] p-3 rounded-md border border-border/20 overflow-auto max-h-[450px] selection:bg-sky-500/20"
        dangerouslySetInnerHTML={{ __html: highlightJson(json) }}
      />
    </div>
  );
}
