import { afterEach, expect, it, vi } from "vitest";

vi.mock("@/shared/api", () => ({ preserveEncodedMetadata: vi.fn() }));

import { preserveEncodedMetadata } from "@/shared/api";
import type { ExportSnapshot } from "./export-image";
import { exportImage } from "./export-image";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.mocked(preserveEncodedMetadata).mockReset();
});

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

function stubImageData() {
  vi.stubGlobal("ImageData", FakeImageData);
}

function stubContext(
  overrides: Partial<CanvasRenderingContext2D> & {
    getContextAttributes?: (() => CanvasRenderingContext2DSettings) | undefined;
  } = {},
) {
  const ctx = {
    putImageData: vi.fn(),
    getContextAttributes: vi.fn(() => ({ colorSpace: "srgb" }) as CanvasRenderingContext2DSettings),
    ...overrides,
  } as unknown as CanvasRenderingContext2D;
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx);
  return ctx;
}

function stubToBlob(type: string) {
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => {
    callback(new Blob([new Uint8Array([1, 2, 3])], { type }));
  });
}

function snapshot(overrides: Partial<ExportSnapshot> = {}): ExportSnapshot {
  return {
    sourceFile: new File(["src"], "original.jpg"),
    rgba: new Uint8Array(4),
    width: 1,
    height: 1,
    format: "png",
    quality: 90,
    ...overrides,
  };
}

it("fails when the canvas cannot encode the output", async () => {
  stubContext();
  stubImageData();
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => callback(null));

  await expect(exportImage(snapshot())).rejects.toThrow("Image encoding failed");
  expect(preserveEncodedMetadata).not.toHaveBeenCalled();
});

it("fails when the canvas context cannot be created", async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  stubImageData();

  await expect(exportImage(snapshot())).rejects.toThrow("Image encoding failed");
  expect(preserveEncodedMetadata).not.toHaveBeenCalled();
});

it("fails when the encoded blob's MIME type does not match the requested format", async () => {
  stubContext();
  stubImageData();
  stubToBlob("image/png"); // requested jpeg, canvas fell back to png

  await expect(exportImage(snapshot({ format: "jpeg" }))).rejects.toThrow("Image encoding failed");
  expect(preserveEncodedMetadata).not.toHaveBeenCalled();
});

it("takes the filename stem and metadata source from the snapshot's source file", async () => {
  stubContext();
  stubImageData();
  stubToBlob("image/png");
  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([9])], { type: "image/png" }),
    report: { issues: [] },
  });

  const result = await exportImage(
    snapshot({ sourceFile: new File(["src"], "vacation-photo.jpg"), width: 800, height: 600 }),
  );

  expect(preserveEncodedMetadata).toHaveBeenCalledTimes(1);
  const [source] = vi.mocked(preserveEncodedMetadata).mock.calls[0];
  expect(source.name).toBe("vacation-photo.jpg");
  expect(result.filename).toBe("vacation-photo-800x600.png");
  expect(result.blob.type).toBe("image/png");
  expect(result.report).toEqual({ issues: [] });
});

it("maps quality 90 to 0.9 for jpeg and webp but omits it for png", async () => {
  stubContext();
  stubImageData();
  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([1])], { type: "image/jpeg" }),
    report: { issues: [] },
  });

  const toBlob = vi
    .spyOn(HTMLCanvasElement.prototype, "toBlob")
    .mockImplementation((callback, type) => callback(new Blob([new Uint8Array([1])], { type })));

  await exportImage(snapshot({ format: "jpeg", quality: 90 }));
  expect(toBlob.mock.calls[0][2]).toBe(0.9);

  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([1])], { type: "image/png" }),
    report: { issues: [] },
  });
  await exportImage(snapshot({ format: "png", quality: 90 }));
  expect(toBlob.mock.calls[1][2]).toBeUndefined();
});

it("calls metadata preservation with the encoded blob before resolving", async () => {
  stubContext();
  stubImageData();
  stubToBlob("image/webp");
  let calledBeforeResolve = false;
  vi.mocked(preserveEncodedMetadata).mockImplementation(async () => {
    calledBeforeResolve = true;
    return {
      blob: new Blob([new Uint8Array([2])], { type: "image/webp" }),
      report: { issues: [] },
    };
  });

  await exportImage(snapshot({ format: "webp" }));
  expect(calledBeforeResolve).toBe(true);
});

it("reports color as srgb when the context confirms an sRGB color space", async () => {
  stubContext({
    getContextAttributes: vi.fn(() => ({ colorSpace: "srgb" }) as CanvasRenderingContext2DSettings),
  });
  stubImageData();
  stubToBlob("image/png");
  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([1])], { type: "image/png" }),
    report: { issues: [] },
  });

  await exportImage(snapshot());
  const facts = vi.mocked(preserveEncodedMetadata).mock.calls[0][2];
  expect(facts).toEqual({ width: 1, height: 1, orientation: "normalize", color: "srgb" });
});

it("reports color as unverified when the context cannot confirm sRGB", async () => {
  stubContext({
    getContextAttributes: vi.fn(
      () => ({ colorSpace: "display-p3" }) as CanvasRenderingContext2DSettings,
    ),
  });
  stubImageData();
  stubToBlob("image/png");
  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([1])], { type: "image/png" }),
    report: { issues: [] },
  });

  await exportImage(snapshot());
  const facts = vi.mocked(preserveEncodedMetadata).mock.calls[0][2];
  expect(facts.color).toBe("unverified");
});

it("reports color as unverified when getContextAttributes is unavailable", async () => {
  stubContext({ getContextAttributes: undefined });
  stubImageData();
  stubToBlob("image/png");
  vi.mocked(preserveEncodedMetadata).mockResolvedValue({
    blob: new Blob([new Uint8Array([1])], { type: "image/png" }),
    report: { issues: [] },
  });

  await exportImage(snapshot());
  const facts = vi.mocked(preserveEncodedMetadata).mock.calls[0][2];
  expect(facts.color).toBe("unverified");
});
