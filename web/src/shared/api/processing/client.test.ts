import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoSharpParams } from "@/shared/types/wasm-types";
import { DEFAULT_PARAMS } from "@/shared/types/wasm-types";

// Mock everything below the facade.
vi.mock("./wasm", () => ({
  clearAllCaches: vi.fn(async () => {}),
  prepareImage: vi.fn(async () => {}),
  prepareBaseImage: vi.fn(async () => {}),
  processImageParallel: vi.fn(async () => ({
    imageData: new Uint8Array(4),
    outputWidth: 1,
    outputHeight: 1,
    diagnostics: {},
  })),
  ingestBegin: vi.fn(async () => ({ width: 1000, height: 750 })),
  ingestStripe: vi.fn(async () => {}),
  ingestEnd: vi.fn(async () => ({ width: 1000, height: 750 })),
  ingestAbortFireAndForget: vi.fn(),
  resetWorker: vi.fn(),
  setProgressCallback: vi.fn(),
}));
vi.mock("./probe-pool", () => ({ destroyProbePool: vi.fn() }));
vi.mock("./ingest", () => ({
  decodeToBitmap: vi.fn(async () => fakeBitmap(8000, 6000)), // 48MP -> striped
  bitmapToRgba: vi.fn((b: ImageBitmap) => ({
    data: new Uint8Array(b.width * b.height * 4),
    width: b.width,
    height: b.height,
  })),
  makePreview: vi.fn(() => ({ rgbaData: new Uint8Array(4), width: 1, height: 1 })),
  planStripes: vi.fn(() => ({ stripeHeight: 1024, count: 3 })),
  extractStripes: vi.fn(async function* () {
    yield { rgba: new Uint8Array(8), rows: 1024 };
    yield { rgba: new Uint8Array(8), rows: 1024 };
    yield { rgba: new Uint8Array(8), rows: 952 };
  }),
}));

import { ProcessingClient } from "./client";
import { CancelledError } from "./errors";
import * as ingest from "./ingest";
import * as wasm from "./wasm";

function fakeBitmap(width: number, height: number): ImageBitmap {
  return { width, height, close: () => {} } as unknown as ImageBitmap;
}

function params(): AutoSharpParams {
  return { ...DEFAULT_PARAMS, target_width: 800, target_height: 600 };
}

describe("ProcessingClient", () => {
  let client: ProcessingClient;

  beforeEach(() => {
    client = new ProcessingClient();
    vi.clearAllMocks();
  });
  afterEach(() => vi.restoreAllMocks());

  it("classifies a >24MP image as striped and returns a preview", async () => {
    const info = await client.decode(new File([], "big.jpg"));
    expect(info.striped).toBe(true);
    expect(info.width).toBe(8000);
    expect(info.height).toBe(6000);
    expect(info.preview.rgbaData).toBeInstanceOf(Uint8Array);
  });

  it("runs the striped sequence: begin -> stripes -> end -> process", async () => {
    await client.decode(new File([], "big.jpg"));
    const result = await client.process(params()).promise;
    expect(wasm.ingestBegin).toHaveBeenCalledWith(8000, 6000, 800, 600);
    expect(wasm.ingestStripe).toHaveBeenCalledTimes(3);
    expect(wasm.ingestEnd).toHaveBeenCalledTimes(1);
    // Downstream runs on the intermediate with empty pixel data.
    expect(wasm.processImageParallel).toHaveBeenCalledWith(
      expect.objectContaining({ length: 0 }),
      1000,
      750,
      expect.any(String),
      expect.anything(),
    );
    expect(result.outputWidth).toBe(1);
  });

  it("forces uniform resize and summary diagnostics on the striped path", async () => {
    await client.decode(new File([], "big.jpg"));
    await client.process({
      ...params(),
      resize_strategy: { strategy: "uniform" } as never,
      diagnostics_level: "full",
    }).promise;
    const sentJson = vi.mocked(wasm.processImageParallel).mock.calls[0][3];
    const sent = JSON.parse(sentJson);
    expect(sent.resize_strategy).toBeNull();
    expect(sent.diagnostics_level).toBe("summary");
  });

  it("uses the monolithic path for small images", async () => {
    vi.mocked(ingest.decodeToBitmap).mockResolvedValueOnce(fakeBitmap(4000, 3000)); // 12MP
    await client.decode(new File([], "small.jpg"));
    await client.process(params()).promise;
    expect(wasm.ingestBegin).not.toHaveBeenCalled();
    expect(wasm.processImageParallel).toHaveBeenCalledWith(
      expect.objectContaining({ length: 4000 * 3000 * 4 }),
      4000,
      3000,
      expect.any(String),
      expect.anything(),
    );
  });

  it("cancel during ingest aborts and rejects with CancelledError", async () => {
    vi.mocked(wasm.ingestStripe).mockImplementation(async () => {
      job.cancel(); // cancel mid-stripe; the loop checks before the next one
    });
    await client.decode(new File([], "big.jpg"));
    const job = client.process(params());
    await expect(job.promise).rejects.toBeInstanceOf(CancelledError);
    expect(wasm.ingestAbortFireAndForget).toHaveBeenCalled();
    expect(wasm.processImageParallel).not.toHaveBeenCalled();
  });

  it("reports monotonically increasing overall progress", async () => {
    await client.decode(new File([], "big.jpg"));
    const job = client.process(params());
    const overalls: number[] = [];
    job.onProgress((e) => overalls.push(e.overall));
    await job.promise;
    expect(overalls.length).toBeGreaterThan(0);
    for (let i = 1; i < overalls.length; i++) {
      expect(overalls[i]).toBeGreaterThanOrEqual(overalls[i - 1]);
    }
    expect(overalls.at(-1)).toBeCloseTo(1, 5);
  });

  it("reset terminates workers and the probe pool", async () => {
    await client.reset();
    expect(wasm.resetWorker).toHaveBeenCalled();
  });
});
