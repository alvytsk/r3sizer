import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/api", () => {
  class CancelledError extends Error {}
  return {
    CancelledError,
    processingClient: { process: vi.fn(), decode: vi.fn(), prewarmBase: vi.fn() },
  };
});

import { useImageStore } from "@/entities/images";
import { useOutputStore } from "@/entities/outputs";
import { CancelledError, type DecodedInput, type ProcessJob, processingClient } from "@/shared/api";
import type { ProcessResult } from "@/shared/lib";
import { useProcessingStore } from "./store";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** A job whose result the test controls. `cancel` does not settle it, so a
 * result that arrives after cancellation exercises the store's own guard. */
function fakeJob(sourceFile: File) {
  const d = deferred<ProcessResult>();
  const job: ProcessJob = {
    promise: d.promise,
    sourceFile,
    onProgress: () => {},
    cancel: vi.fn(),
  };
  return { job, resolve: d.resolve, reject: d.reject };
}

function result(tag: number): ProcessResult {
  return {
    imageData: new Uint8Array([tag]),
    outputWidth: tag,
    outputHeight: tag,
    diagnostics: {} as ProcessResult["diagnostics"],
  };
}

function decoded(): DecodedInput {
  return {
    width: 100,
    height: 50,
    striped: false,
    preview: { rgbaData: new Uint8Array(4), width: 1, height: 1 },
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("image-processing store lifecycle", () => {
  const fileA = new File(["a"], "a.jpg");
  const fileB = new File(["b"], "b.jpg");

  beforeEach(() => {
    vi.clearAllMocks();
    useImageStore.getState().resetImage();
    useOutputStore.getState().clearOutput();
    useProcessingStore.setState({ isProcessing: false, progress: null, error: null });
    useImageStore.setState({ inputFile: fileA, paramsVersion: 1 });
  });

  it("commits job A with source A even when the image entity changes to B", async () => {
    const a = fakeJob(fileA);
    vi.mocked(processingClient.process).mockReturnValueOnce(a.job);
    const run = useProcessingStore.getState().process();
    useImageStore.setState({ inputFile: fileB, paramsVersion: 7 });
    a.resolve(result(1));
    await run;
    const out = useOutputStore.getState();
    expect(out.outputSourceFile).toBe(fileA);
    expect(out.lastProcessedVersion).toBe(1);
    expect(useProcessingStore.getState().isProcessing).toBe(false);
  });

  it("never lets job A overwrite job B once B has started", async () => {
    const a = fakeJob(fileA);
    const b = fakeJob(fileB);
    vi.mocked(processingClient.process).mockReturnValueOnce(a.job).mockReturnValueOnce(b.job);
    const runA = useProcessingStore.getState().process();
    useImageStore.setState({ inputFile: fileB, paramsVersion: 2 });
    const runB = useProcessingStore.getState().process();

    a.resolve(result(1));
    await runA;
    expect(useOutputStore.getState().outputRgbaData).toBeNull();
    expect(useProcessingStore.getState().isProcessing).toBe(true);

    b.resolve(result(2));
    await runB;
    const out = useOutputStore.getState();
    expect(out.outputWidth).toBe(2);
    expect(out.outputSourceFile).toBe(fileB);
    expect(useProcessingStore.getState().isProcessing).toBe(false);
  });

  it("discards a result that arrives after reset", async () => {
    const a = fakeJob(fileA);
    vi.mocked(processingClient.process).mockReturnValueOnce(a.job);
    const run = useProcessingStore.getState().process();
    useProcessingStore.getState().resetProcessing();
    expect(a.job.cancel).toHaveBeenCalled();
    a.resolve(result(1));
    await run;
    expect(useOutputStore.getState().outputRgbaData).toBeNull();
    expect(useOutputStore.getState().outputSourceFile).toBeNull();
    expect(useProcessingStore.getState().isProcessing).toBe(false);
  });

  it("sets no error when the active job is cancelled by loading a new file", async () => {
    // The loading UI calls setInput without resetting processing, so job A is
    // still current when the client settles it as a cancellation.
    const a = fakeJob(fileA);
    vi.mocked(processingClient.process).mockReturnValueOnce(a.job);
    vi.mocked(processingClient.decode).mockResolvedValueOnce(decoded());
    const run = useProcessingStore.getState().process();
    await useImageStore.getState().setInput(fileB);
    a.reject(new CancelledError());
    await run;
    expect(useProcessingStore.getState()).toMatchObject({ error: null, isProcessing: false });
    expect(useOutputStore.getState().outputRgbaData).toBeNull();
  });

  it("clearOutput resets the output source file", async () => {
    const a = fakeJob(fileA);
    vi.mocked(processingClient.process).mockReturnValueOnce(a.job);
    const run = useProcessingStore.getState().process();
    a.resolve(result(1));
    await run;
    useOutputStore.getState().clearOutput();
    expect(useOutputStore.getState().outputSourceFile).toBeNull();
  });

  it("keeps the latest requested decode regardless of completion order", async () => {
    for (const order of ["b-first", "a-first"] as const) {
      const a = deferred<DecodedInput>();
      const b = deferred<DecodedInput>();
      vi.mocked(processingClient.decode)
        .mockReturnValueOnce(a.promise)
        .mockReturnValueOnce(b.promise);
      const setA = useImageStore.getState().setInput(fileA);
      const setB = useImageStore.getState().setInput(fileB);
      const [first, second] = order === "b-first" ? [b, a] : [a, b];
      first.resolve(decoded());
      await flush();
      second.resolve(decoded());
      await Promise.all([setA, setB]);
      expect(useImageStore.getState().inputFile).toBe(fileB);
    }
  });

  it("ignores a stale decode error", async () => {
    const a = deferred<DecodedInput>();
    vi.mocked(processingClient.decode)
      .mockReturnValueOnce(a.promise)
      .mockResolvedValueOnce(decoded());
    const setA = useImageStore.getState().setInput(fileA);
    await useImageStore.getState().setInput(fileB);
    a.reject(new Error("decode failed"));
    await setA;
    expect(useImageStore.getState().error).toBeNull();
    expect(useImageStore.getState().inputFile).toBe(fileB);
  });
});
