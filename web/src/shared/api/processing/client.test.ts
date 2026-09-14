import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoSharpParams } from "@/shared/lib";
import { DEFAULT_PARAMS } from "@/shared/lib";

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
  return { width, height, close: vi.fn() } as unknown as ImageBitmap;
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
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

  it("falls back to the monolithic path when a large image has a modest downscale", async () => {
    // 8000x6000 (48MP) is classified striped at decode, but target 4000x3000
    // is only a 2x shrink — below the 3.0 striped-ingest threshold, so the
    // striped ingest would reject it. The job must run monolithic instead.
    await client.decode(new File([], "big.jpg"));
    await client.process({ ...params(), target_width: 4000, target_height: 3000 }).promise;
    expect(wasm.ingestBegin).not.toHaveBeenCalled();
    expect(wasm.processImageParallel).toHaveBeenCalledWith(
      expect.objectContaining({ length: 8000 * 6000 * 4 }),
      8000,
      6000,
      expect.any(String),
      expect.anything(),
    );
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

  it("captures the source file in the processing job", async () => {
    const file = new File(["source A"], "a.jpg");
    await client.decode(file);
    const job = client.process(params());
    expect(job.sourceFile).toBe(file);
    await job.promise;
  });

  it("keeps the latest requested decode and closes discarded bitmaps", async () => {
    for (const order of ["b-first", "a-first"] as const) {
      const a = deferred<ImageBitmap>();
      const b = deferred<ImageBitmap>();
      const bitmapA = fakeBitmap(40, 30);
      const bitmapB = fakeBitmap(40, 30);
      vi.mocked(ingest.decodeToBitmap)
        .mockReturnValueOnce(a.promise)
        .mockReturnValueOnce(b.promise);
      const fileA = new File(["a"], "a.jpg");
      const fileB = new File(["b"], "b.jpg");
      const decodeA = client.decode(fileA);
      const decodeB = client.decode(fileB);
      if (order === "b-first") {
        b.resolve(bitmapB);
        await decodeB;
        a.resolve(bitmapA);
        await expect(decodeA).rejects.toBeInstanceOf(CancelledError);
      } else {
        a.resolve(bitmapA);
        await expect(decodeA).rejects.toBeInstanceOf(CancelledError);
        b.resolve(bitmapB);
        await decodeB;
      }
      expect(bitmapA.close).toHaveBeenCalled();
      expect(bitmapB.close).not.toHaveBeenCalled();
      const job = client.process(params());
      expect(job.sourceFile).toBe(fileB);
      await job.promise;
    }
  });

  it("ignores a stale decode error", async () => {
    const a = deferred<ImageBitmap>();
    vi.mocked(ingest.decodeToBitmap)
      .mockReturnValueOnce(a.promise)
      .mockResolvedValueOnce(fakeBitmap(40, 30));
    const decodeA = client.decode(new File(["a"], "a.jpg"));
    const fileB = new File(["b"], "b.jpg");
    await client.decode(fileB);
    a.reject(new Error("decode failed"));
    await expect(decodeA).rejects.toThrow();
    expect(client.process(params()).sourceFile).toBe(fileB);
  });

  it("cancels the active job before replacing the decoded input", async () => {
    const pending = deferred<never>();
    vi.mocked(wasm.processImageParallel).mockReturnValueOnce(pending.promise);
    const oldBitmap = fakeBitmap(40, 30);
    vi.mocked(ingest.decodeToBitmap).mockResolvedValueOnce(oldBitmap);
    await client.decode(new File(["a"], "a.jpg"));
    const job = client.process(params());
    const cancel = vi.spyOn(job, "cancel");
    const next = client.decode(new File(["b"], "b.jpg"));
    expect(cancel).toHaveBeenCalled();
    expect(cancel.mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(oldBitmap.close).mock.invocationCallOrder[0],
    );
    await next;
  });

  it("reset terminates workers and the probe pool", async () => {
    await client.reset();
    expect(wasm.resetWorker).toHaveBeenCalled();
  });
});
