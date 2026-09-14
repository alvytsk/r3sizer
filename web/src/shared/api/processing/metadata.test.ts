import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { MetadataExportResponse, MetadataIssueReason, OutputFacts } from "@/shared/lib";

vi.mock("./wasm", () => ({ exportMetadata: vi.fn() }));
vi.mock("./probe-pool", () => ({
  initProbePool: vi.fn(async () => {}),
  isProbePoolReady: () => false,
  distributeBaseData: vi.fn(),
  resetBaseCache: vi.fn(),
  runProbesParallel: vi.fn(),
}));
vi.mock("./wasm-pkg/r3sizer_wasm", () => ({ initSync: vi.fn(), preserve_metadata: vi.fn() }));

import { preserveEncodedMetadata } from "./metadata";
import { exportMetadata } from "./wasm";
import { initSync, preserve_metadata } from "./wasm-pkg/r3sizer_wasm";

const facts: OutputFacts = { width: 1, height: 1, orientation: "normalize", color: "srgb" };
const issue = (reason: MetadataIssueReason) => ({ category: "unknown", reason, field: null });
const bytesOf = async (b: Blob) => [...new Uint8Array(await b.arrayBuffer())];

afterEach(() => vi.mocked(exportMetadata).mockReset());

it("returns the original encoded blob with an issue on worker failure", async () => {
  vi.mocked(exportMetadata).mockRejectedValueOnce(new Error("worker failed"));
  const encoded = new Blob([new Uint8Array([1, 2, 3])], { type: "image/png" });
  const result = await preserveEncodedMetadata(new File(["src"], "a.jpg"), encoded, {
    width: 1,
    height: 1,
    orientation: "normalize",
    color: "srgb",
  });
  expect(result.blob).toBe(encoded);
  expect(result.report.issues).toContainEqual({
    category: "unknown",
    reason: "merge_failed",
    field: null,
  });
});

it("rejects an oversized source without reading it", async () => {
  const source = new File(["src"], "a.jpg");
  Object.defineProperty(source, "size", { value: 256 * 1024 * 1024 + 1 });
  const read = vi.spyOn(source, "arrayBuffer");
  const encoded = new Blob([new Uint8Array([1])], { type: "image/png" });
  const result = await preserveEncodedMetadata(source, encoded, facts);
  expect(result.blob).toBe(encoded);
  expect(result.report).toEqual({ issues: [issue("limit_exceeded")] });
  expect(read).not.toHaveBeenCalled();
  expect(exportMetadata).not.toHaveBeenCalled();
});

it("reports an unreadable source as unverified", async () => {
  const source = new File(["src"], "a.jpg");
  vi.spyOn(source, "arrayBuffer").mockRejectedValueOnce(new Error("NotReadableError"));
  const encoded = new Blob([new Uint8Array([1])], { type: "image/png" });
  const result = await preserveEncodedMetadata(source, encoded, facts);
  expect(result.blob).toBe(encoded);
  expect(result.report).toEqual({ issues: [issue("unverified")] });
  expect(exportMetadata).not.toHaveBeenCalled();
});

describe("worker transport (real wasm.ts, fake Worker)", () => {
  type Posted = { data: { type: string; id?: number; [k: string]: unknown }; transfer: unknown[] };
  let posted: Posted[] = [];
  let respond: ((req: Posted["data"], w: FakeWorker) => void) | null = null;
  let actual: typeof import("./wasm");

  class FakeWorker {
    onmessage: ((e: { data: unknown }) => void) | null = null;
    onerror: unknown = null;
    postMessage(msg: unknown, transfer: Transferable[] = []) {
      // Structured clone as the browser would: transferred buffers detach.
      const data = structuredClone(msg, { transfer }) as Posted["data"];
      posted.push({ data, transfer });
      if (data.type === "init") {
        queueMicrotask(() => this.onmessage?.({ data: { type: "ready" } }));
      } else if (data.type === "metadata_export") {
        respond?.(data, this);
      }
    }
    terminate() {}
  }

  const sentRequest = () => posted.find((p) => p.data.type === "metadata_export");

  beforeAll(async () => {
    vi.stubGlobal("Worker", FakeWorker);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({})),
    );
    vi.spyOn(WebAssembly, "compileStreaming").mockResolvedValue({} as WebAssembly.Module);
    actual = await vi.importActual<typeof import("./wasm")>("./wasm");
  });
  afterEach(() => {
    actual.resetWorker();
    vi.useRealTimers();
    posted = [];
    respond = null;
  });
  afterAll(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("transfers fresh request buffers and wraps response bytes in a Blob", async () => {
    vi.mocked(exportMetadata).mockImplementation(actual.exportMetadata);
    respond = (req, w) => {
      const metadataResponse: MetadataExportResponse = {
        bytes: new Uint8Array([9, 8, 7]),
        report: { issues: [] },
      };
      queueMicrotask(() =>
        w.onmessage?.({ data: { type: "metadata_exported", id: req.id, metadataResponse } }),
      );
    };
    const source = new File([new Uint8Array([1, 2])], "a.jpg");
    const encoded = new Blob([new Uint8Array([3, 4, 5])], { type: "image/jpeg" });

    const result = await preserveEncodedMetadata(source, encoded, facts);

    const sent = sentRequest()!;
    const req = sent.data.metadataRequest as { source: Uint8Array; encoded: Uint8Array };
    expect([...req.source]).toEqual([1, 2]);
    expect([...req.encoded]).toEqual([3, 4, 5]);
    expect(sent.data.metadataRequest).toMatchObject({ facts });
    expect(sent.transfer).toHaveLength(2);
    for (const buf of sent.transfer) expect((buf as ArrayBuffer).byteLength).toBe(0);
    // Caller-owned data is untouched by the transfer.
    expect(await bytesOf(encoded)).toEqual([3, 4, 5]);
    expect(await bytesOf(source)).toEqual([1, 2]);

    expect(result.blob).not.toBe(encoded);
    expect(result.blob.type).toBe("image/jpeg");
    expect(await bytesOf(result.blob)).toEqual([9, 8, 7]);
    expect(result.report).toEqual({ issues: [] });
  });

  it("falls back with merge_failed on a worker error response", async () => {
    vi.mocked(exportMetadata).mockImplementation(actual.exportMetadata);
    respond = (req, w) =>
      queueMicrotask(() =>
        w.onmessage?.({ data: { type: "metadata_exported", id: req.id, error: "boom" } }),
      );
    const encoded = new Blob([new Uint8Array([1])], { type: "image/png" });
    const result = await preserveEncodedMetadata(new File(["s"], "a.jpg"), encoded, facts);
    expect(result.blob).toBe(encoded);
    expect(result.report).toEqual({ issues: [issue("merge_failed")] });
  });

  it("falls back with merge_failed when the worker times out", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.mocked(exportMetadata).mockImplementation(actual.exportMetadata);
    const encoded = new Blob([new Uint8Array([1])], { type: "image/png" });
    const pending = preserveEncodedMetadata(new File(["s"], "a.jpg"), encoded, facts);
    await vi.waitFor(() => expect(sentRequest()).toBeDefined());
    await vi.advanceTimersByTimeAsync(30_000);
    const result = await pending;
    expect(result.blob).toBe(encoded);
    expect(result.report).toEqual({ issues: [issue("merge_failed")] });
  });

  it("falls back with merge_failed when the worker is reset", async () => {
    vi.mocked(exportMetadata).mockImplementation(actual.exportMetadata);
    const encoded = new Blob([new Uint8Array([1])], { type: "image/png" });
    const pending = preserveEncodedMetadata(new File(["s"], "a.jpg"), encoded, facts);
    await vi.waitFor(() => expect(sentRequest()).toBeDefined());
    actual.resetWorker();
    const result = await pending;
    expect(result.blob).toBe(encoded);
    expect(result.report).toEqual({ issues: [issue("merge_failed")] });
  });
});

describe("worker metadata_export branch", () => {
  const post = vi.fn();

  beforeAll(async () => {
    vi.spyOn(self, "postMessage").mockImplementation(post);
    await import("./wasm-worker");
    self.onmessage?.({ data: { type: "init", module: {} } } as MessageEvent);
    expect(initSync).toHaveBeenCalled();
  });
  afterEach(() => post.mockClear());
  afterAll(() => vi.restoreAllMocks());

  it("posts the response and transfers its bytes back", () => {
    const response = { bytes: new Uint8Array([7]), report: { issues: [] } };
    vi.mocked(preserve_metadata).mockReturnValueOnce(response);
    const metadataRequest = { source: new Uint8Array(1), encoded: new Uint8Array(1), facts };
    self.onmessage?.({ data: { type: "metadata_export", id: 4, metadataRequest } } as MessageEvent);
    expect(preserve_metadata).toHaveBeenCalledWith(metadataRequest);
    expect(post).toHaveBeenCalledWith(
      { type: "metadata_exported", id: 4, metadataResponse: response },
      [response.bytes.buffer],
    );
  });

  it("posts an error when the export throws", () => {
    vi.mocked(preserve_metadata).mockImplementationOnce(() => {
      throw new Error("bad request");
    });
    self.onmessage?.({ data: { type: "metadata_export", id: 5 } } as MessageEvent);
    expect(post).toHaveBeenCalledWith({ type: "metadata_exported", id: 5, error: "bad request" });
  });
});
