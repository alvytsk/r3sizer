import i18next from "i18next";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, expect, it, vi } from "vitest";

vi.mock("@/shared/api", () => ({ preserveEncodedMetadata: vi.fn() }));

import { useExportPrefsStore } from "@/entities/export-preferences";
import { useOutputStore } from "@/entities/outputs";
import { preserveEncodedMetadata } from "@/shared/api";
import type { AutoSharpDiagnostics, AutoSharpParams, MetadataReport } from "@/shared/lib";
import { DEFAULT_PARAMS } from "@/shared/lib";
import en from "../../../../public/locales/en.json";
import ru from "../../../../public/locales/ru.json";
import { DownloadButton } from "./download-button";

// Mirrors how the real toolbar mounts this button: `{hasOutput &&
// <DownloadButton />}`. Unlike the button's own internal `if
// (!outputRgbaData) return null`, this actually unmounts the component
// (tears down its hooks/effects) once the output is cleared.
function ConditionalDownloadButton() {
  const hasOutput = useOutputStore((s) => !!s.outputRgbaData);
  return hasOutput ? createElement(DownloadButton) : null;
}

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function makeI18n(lng: "en" | "ru" = "en") {
  const instance = i18next.createInstance();
  instance.use(initReactI18next).init({
    lng,
    fallbackLng: "en",
    resources: { en: { translation: en }, ru: { translation: ru } },
  });
  return instance;
}

const flush = () => new Promise((r) => setTimeout(r, 0));

class FakeImageData {
  data: Uint8ClampedArray;
  width: number;
  height: number;
  constructor(data: Uint8ClampedArray, width: number, height: number) {
    this.data = data;
    this.width = width;
    this.height = height;
  }
}

function setOutput(overrides: {
  sourceFile?: File;
  rgba?: Uint8Array;
  width?: number;
  height?: number;
}) {
  useOutputStore.getState().setResult({
    imageData: overrides.rgba ?? new Uint8Array(4 * 2 * 2),
    outputWidth: overrides.width ?? 2,
    outputHeight: overrides.height ?? 2,
    diagnostics: {} as unknown as AutoSharpDiagnostics,
    params: DEFAULT_PARAMS as AutoSharpParams,
    paramsVersion: 1,
    sourceFile: overrides.sourceFile ?? new File(["a"], "a.jpg"),
  });
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

function renderButton(lng: "en" | "ru" = "en") {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root?.render(
      createElement(I18nextProvider, { i18n: makeI18n(lng) }, createElement(DownloadButton)),
    );
  });
  return container;
}

function renderConditional(lng: "en" | "ru" = "en") {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root?.render(
      createElement(
        I18nextProvider,
        { i18n: makeI18n(lng) },
        createElement(ConditionalDownloadButton),
      ),
    );
  });
  return container;
}

function saveButton(el: HTMLElement) {
  const button = el.querySelector('button[title^="Save as"]');
  if (!button) throw new Error("save button not found");
  return button as HTMLButtonElement;
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
  useOutputStore.getState().clearOutput();
  useExportPrefsStore.getState().setExportFormat("jpeg");
  useExportPrefsStore.getState().setExportQuality(90);
  vi.restoreAllMocks();
  vi.mocked(preserveEncodedMetadata).mockReset();
});

function stubCanvas(mimeOf: (requested: string) => string = (m) => m) {
  vi.stubGlobal("ImageData", FakeImageData);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    putImageData: vi.fn(),
    getContextAttributes: () => ({ colorSpace: "srgb" }),
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback, type) => {
    callback(new Blob([new Uint8Array([1, 2, 3])], { type: mimeOf(type as string) }));
  });
}

function stubDownloadLink() {
  const clickSpy = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  const createUrl = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:mock");
  const revokeUrl = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  return { clickSpy, createUrl, revokeUrl };
}

function deferredMetadata() {
  let resolveFn!: (v: { blob: Blob; report: MetadataReport }) => void;
  const promise = new Promise<{ blob: Blob; report: MetadataReport }>((resolve) => {
    resolveFn = resolve;
  });
  vi.mocked(preserveEncodedMetadata).mockReturnValue(promise);
  return (v: { blob: Blob; report: MetadataReport }) => resolveFn(v);
}

it("disables the button while an export is pending, then re-enables it", async () => {
  setOutput({});
  stubCanvas();
  stubDownloadLink();
  const resolve = deferredMetadata();
  const el = renderButton();

  act(() => saveButton(el).click());
  expect(saveButton(el).disabled).toBe(true);

  await act(async () => {
    resolve({
      blob: new Blob([new Uint8Array([9])], { type: "image/jpeg" }),
      report: { issues: [] },
    });
    await flush();
    await flush();
  });

  expect(saveButton(el).disabled).toBe(false);
});

it("guards against duplicate clicks: one download for two rapid clicks", async () => {
  setOutput({});
  stubCanvas();
  const { createUrl } = stubDownloadLink();
  const resolve = deferredMetadata();
  const el = renderButton();

  await act(async () => {
    saveButton(el).click();
    saveButton(el).click();
    await flush();
  });
  expect(preserveEncodedMetadata).toHaveBeenCalledTimes(1);

  await act(async () => {
    resolve({
      blob: new Blob([new Uint8Array([9])], { type: "image/jpeg" }),
      report: { issues: [] },
    });
    await flush();
    await flush();
  });

  expect(createUrl).toHaveBeenCalledTimes(1);
});

it("discards a stale export when the source is replaced mid-export, without downloading or messaging", async () => {
  setOutput({ sourceFile: new File(["a"], "first.jpg") });
  stubCanvas();
  const { createUrl } = stubDownloadLink();
  const resolve = deferredMetadata();
  const el = renderButton();

  await act(async () => {
    saveButton(el).click();
    await flush();
  });
  expect(preserveEncodedMetadata).toHaveBeenCalledTimes(1);

  // Output changes while the first export is still awaiting metadata.
  await act(async () => {
    setOutput({ sourceFile: new File(["b"], "second.jpg"), width: 4, height: 4 });
    await flush();
  });

  // The button is usable again immediately — the stale request no longer blocks it.
  expect(saveButton(el).disabled).toBe(false);
  expect(el.querySelector('[role="status"]')).toBeNull();

  await act(async () => {
    resolve({
      blob: new Blob([new Uint8Array([9])], { type: "image/jpeg" }),
      report: { issues: [{ category: "exif", reason: "malformed", field: null }] },
    });
    await flush();
    await flush();
  });

  expect(createUrl).not.toHaveBeenCalled();
  expect(el.querySelector('[role="status"]')).toBeNull();
});

it("clears an old warning message when a new export starts", async () => {
  setOutput({});
  stubCanvas();
  stubDownloadLink();
  const el = renderButton();

  vi.mocked(preserveEncodedMetadata).mockResolvedValueOnce({
    blob: new Blob([new Uint8Array([1])], { type: "image/jpeg" }),
    report: { issues: [{ category: "exif", reason: "malformed", field: null }] },
  });
  await act(async () => {
    saveButton(el).click();
    await flush();
    await flush();
  });
  expect(el.querySelector('[role="status"]')).not.toBeNull();

  const resolve = deferredMetadata();
  act(() => saveButton(el).click());
  // The pending message replaces the old warning immediately, synchronously with the click.
  expect(el.querySelector('[role="status"]')?.textContent).toBe("Preparing download…");

  await act(async () => {
    resolve({
      blob: new Blob([new Uint8Array([1])], { type: "image/jpeg" }),
      report: { issues: [] },
    });
    await flush();
    await flush();
  });
});

it("worker failure still produces exactly one download, with a warning", async () => {
  setOutput({});
  stubCanvas();
  const { createUrl } = stubDownloadLink();
  const el = renderButton();

  // preserveEncodedMetadata never rejects; on worker failure it resolves
  // with the original blob and a fallback issue.
  const encoded = new Blob([new Uint8Array([1, 2, 3])], { type: "image/jpeg" });
  vi.mocked(preserveEncodedMetadata).mockResolvedValueOnce({
    blob: encoded,
    report: { issues: [{ category: "unknown", reason: "merge_failed", field: null }] },
  });

  await act(async () => {
    saveButton(el).click();
    await flush();
    await flush();
  });

  expect(createUrl).toHaveBeenCalledTimes(1);
  expect(el.querySelector('[role="status"]')?.textContent).toContain(
    "Download prepared. Some original metadata could not be retained.",
  );
});

it("a null canvas blob produces no download and an export-error message", async () => {
  setOutput({});
  vi.stubGlobal("ImageData", FakeImageData);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    putImageData: vi.fn(),
    getContextAttributes: () => ({ colorSpace: "srgb" }),
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => callback(null));
  const { createUrl } = stubDownloadLink();
  const el = renderButton();

  await act(async () => {
    saveButton(el).click();
    await flush();
    await flush();
  });

  expect(preserveEncodedMetadata).not.toHaveBeenCalled();
  expect(createUrl).not.toHaveBeenCalled();
  expect(el.querySelector('[role="status"]')?.textContent).toBe(
    "Could not prepare the image download.",
  );
  expect(saveButton(el).disabled).toBe(false);
});

it("discards a stale export when the button is unmounted mid-export (toolbar clearing output)", async () => {
  setOutput({ sourceFile: new File(["a"], "first.jpg") });
  stubCanvas();
  const { createUrl, clickSpy } = stubDownloadLink();
  const resolve = deferredMetadata();
  const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  const el = renderConditional();

  await act(async () => {
    saveButton(el).click();
    await flush();
  });
  expect(preserveEncodedMetadata).toHaveBeenCalledTimes(1);

  // Opening a new file (or resetting processing) clears the output before
  // anything else — this unmounts DownloadButton entirely, same as the
  // toolbar's `hasOutput && <DownloadButton />}`.
  act(() => {
    useOutputStore.getState().clearOutput();
  });
  expect(el.querySelector('[role="status"]')).toBeNull();

  await act(async () => {
    resolve({
      blob: new Blob([new Uint8Array([9])], { type: "image/jpeg" }),
      report: { issues: [{ category: "exif", reason: "malformed", field: null }] },
    });
    await flush();
    await flush();
  });

  expect(createUrl).not.toHaveBeenCalled();
  expect(clickSpy).not.toHaveBeenCalled();
  expect(el.querySelector('[role="status"]')).toBeNull();
  for (const call of errorSpy.mock.calls) {
    expect(call.join(" ")).not.toMatch(/unmounted component/i);
  }
});
