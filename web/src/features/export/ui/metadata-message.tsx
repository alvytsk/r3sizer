import { useTranslation } from "react-i18next";
import type { MetadataIssue, MetadataReport } from "@/shared/lib";

/**
 * What the export message region shows right now. `report` covers the
 * post-encode state (empty issues render nothing — a file with verified
 * absence of metadata needs no warning).
 */
export type ExportMessageState =
  | { kind: "pending" }
  | { kind: "error" }
  | { kind: "report"; report: MetadataReport };

function dedupeIssues(issues: MetadataIssue[]): MetadataIssue[] {
  const seen = new Set<string>();
  const result: MetadataIssue[] = [];
  for (const issue of issues) {
    const key = `${issue.category}|${issue.reason}|${issue.field ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(issue);
  }
  return result;
}

function IssueLine({ issue }: { issue: MetadataIssue }) {
  const { t } = useTranslation();
  const category = t(`download.metadataCategory.${issue.category}`);
  const reason = t(`download.metadataReason.${issue.reason}`);
  // field is a tag/keyword identifier only (never a value); rendered as
  // plain React text, never HTML.
  return (
    <>
      {issue.field
        ? t("download.metadataIssueField", { category, reason, field: issue.field })
        : t("download.metadataIssue", { category, reason })}
    </>
  );
}

/**
 * Always-visible, accessible export status message shown next to the
 * download button (never hidden behind a `lg:`/`xl:` breakpoint). Keeps
 * metadata values out of the copy — only category/reason labels and field
 * identifiers are ever rendered.
 */
export function MetadataMessage({ state }: { state: ExportMessageState | null }) {
  const { t } = useTranslation();

  if (!state) return null;

  if (state.kind === "pending") {
    return (
      <p role="status" aria-live="polite" className="text-[11px] font-mono text-muted-foreground">
        {t("download.preparing")}
      </p>
    );
  }

  if (state.kind === "error") {
    return (
      <p role="status" aria-live="polite" className="text-[11px] font-mono text-destructive">
        {t("download.exportFailed")}
      </p>
    );
  }

  const issues = dedupeIssues(state.report.issues);
  if (issues.length === 0) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      className="text-[11px] font-mono text-amber-600 dark:text-amber-400"
    >
      {issues.length > 1 ? (
        <details>
          <summary className="cursor-pointer">{t("download.metadataPartial")}</summary>
          <ul className="mt-1 space-y-0.5 pl-3">
            {issues.map((issue, i) => (
              <li key={`${issue.category}-${issue.reason}-${issue.field ?? i}`}>
                <IssueLine issue={issue} />
              </li>
            ))}
          </ul>
        </details>
      ) : (
        <p>
          {t("download.metadataPartial")} <IssueLine issue={issues[0]} />
        </p>
      )}
    </div>
  );
}
