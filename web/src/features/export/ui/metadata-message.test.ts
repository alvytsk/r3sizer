import i18next from "i18next";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, expect, it } from "vitest";
import type { MetadataCategory, MetadataIssueReason, MetadataReport } from "@/shared/lib";
import en from "../../../../public/locales/en.json";
import ru from "../../../../public/locales/ru.json";
import { MetadataMessage } from "./metadata-message";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Every category/reason the generated types can produce. Kept in sync with
// generated.ts by hand (unions are erased at runtime) — a drift here means a
// missing locale label, which the completeness test below would also catch.
const ALL_CATEGORIES = [
  "exif",
  "xmp",
  "iptc",
  "icc",
  "text",
  "thumbnail",
  "maker_note",
  "density",
  "unknown",
] as const satisfies readonly MetadataCategory[];

const ALL_REASONS = [
  "unsupported",
  "malformed",
  "removed_stale",
  "unverified",
  "limit_exceeded",
  "merge_failed",
] as const satisfies readonly MetadataIssueReason[];

function makeI18n(lng: "en" | "ru") {
  const instance = i18next.createInstance();
  instance.use(initReactI18next).init({
    lng,
    fallbackLng: "en",
    resources: { en: { translation: en }, ru: { translation: ru } },
  });
  return instance;
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

function render(state: Parameters<typeof MetadataMessage>[0]["state"], lng: "en" | "ru" = "en") {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root?.render(
      createElement(
        I18nextProvider,
        { i18n: makeI18n(lng) },
        createElement(MetadataMessage, { state }),
      ),
    );
  });
  return container;
}

afterEach(() => {
  if (root) {
    act(() => root?.unmount());
    root = null;
  }
  if (container) {
    document.body.removeChild(container);
    container = null;
  }
});

it("shows a live region with the partial-metadata summary and the issue line", () => {
  const report: MetadataReport = {
    issues: [{ category: "exif", reason: "malformed", field: null }],
  };
  const el = render({ kind: "report", report });

  const status = el.querySelector('[role="status"]');
  expect(status).not.toBeNull();
  expect(status?.getAttribute("aria-live")).toBe("polite");
  expect(status?.textContent).toContain(
    "Download prepared. Some original metadata could not be retained.",
  );
  expect(status?.textContent).toContain("EXIF: Invalid metadata");
});

it("renders nothing for an empty report", () => {
  const el = render({ kind: "report", report: { issues: [] } });
  expect(el.querySelector('[role="status"]')).toBeNull();
  expect(el.textContent).toBe("");
});

it("renders nothing when there is no message state", () => {
  const el = render(null);
  expect(el.textContent).toBe("");
});

it("shows expandable details for multiple issue categories", () => {
  const report: MetadataReport = {
    issues: [
      { category: "exif", reason: "malformed", field: null },
      { category: "icc", reason: "unverified", field: null },
    ],
  };
  const el = render({ kind: "report", report });

  const details = el.querySelector("details");
  expect(details).not.toBeNull();
  const items = el.querySelectorAll("li");
  expect(items).toHaveLength(2);
  expect(items[0]?.textContent).toContain("EXIF: Invalid metadata");
  expect(items[1]?.textContent).toContain("Color profile: Preservation could not be verified");
});

it("deduplicates issues sharing the same category, reason, and field", () => {
  const report: MetadataReport = {
    issues: [
      { category: "xmp", reason: "unsupported", field: "foo" },
      { category: "xmp", reason: "unsupported", field: "foo" },
    ],
  };
  const el = render({ kind: "report", report });
  expect(el.querySelector("details")).toBeNull();
  expect(el.textContent).toContain("XMP (foo): Unsupported");
});

it("renders a field identifier as escaped plain text, never as markup", () => {
  const report: MetadataReport = {
    issues: [{ category: "text", reason: "removed_stale", field: "<b>evil</b>" }],
  };
  const el = render({ kind: "report", report });
  // No element was created from the field value — whether i18next also
  // HTML-entity-escapes the interpolated text is an implementation detail,
  // but it must never become a real DOM node.
  expect(el.querySelector("b")).toBeNull();
  expect(el.textContent).toContain("evil");
});

it("shows the pending message while an export is in flight", () => {
  const el = render({ kind: "pending" });
  const status = el.querySelector('[role="status"]');
  expect(status?.textContent).toBe("Preparing download…");
});

it("shows the export-failed message", () => {
  const el = render({ kind: "error" });
  const status = el.querySelector('[role="status"]');
  expect(status?.textContent).toBe("Could not prepare the image download.");
});

it("translates reason labels in Russian", () => {
  const report: MetadataReport = {
    issues: [{ category: "icc", reason: "limit_exceeded", field: null }],
  };
  const el = render({ kind: "report", report }, "ru");
  expect(el.textContent).toContain("Цветовой профиль: Превышен лимит размера метаданных");
});

it("has an English and a Russian label for every generated metadata category and reason", () => {
  for (const lng of ["en", "ru"] as const) {
    const resources = lng === "en" ? en : ru;
    for (const category of ALL_CATEGORIES) {
      const label = resources.download.metadataCategory[category];
      expect(label, `${lng}.download.metadataCategory.${category}`).toBeTruthy();
      expect(label).not.toBe(category);
    }
    for (const reason of ALL_REASONS) {
      const label = resources.download.metadataReason[reason];
      expect(label, `${lng}.download.metadataReason.${reason}`).toBeTruthy();
      expect(label).not.toBe(reason);
    }
  }
});
