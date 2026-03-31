import type { WorkerRequest, WorkerResponse } from "./wasm-worker";
import type { ProcessResult } from "@/types/wasm-types";
import type { BaseData } from "./probe-pool";
import { initProbePool, isProbePoolReady, runProbesParallel, distributeBaseData } from "./probe-pool";
import wasmUrl from "./wasm-pkg/r3sizer_wasm_bg.wasm?url";

let worker: Worker | null = null;
let workerReadyPromise: Promise<void> | null = null;
let nextId = 0;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const pending = new Map<number, { resolve: (r: any) => void; reject: (e: Error) => void }>();

// ---------------------------------------------------------------------------
// Progress callback — set by the store to receive pipeline stage updates.
// ---------------------------------------------------------------------------

let progressCallback: ((stage: string) => void) | null = null;

export function setProgressCallback(cb: ((stage: string) => void) | null) {
  progressCallback = cb;
}

// ---------------------------------------------------------------------------
// Worker lifecycle
// ---------------------------------------------------------------------------

function ensureWorker(): Promise<void> {
  if (workerReadyPromise) return workerReadyPromise;

  workerReadyPromise = new Promise<void>((resolveReady, rejectReady) => {
    WebAssembly.compileStreaming(fetch(wasmUrl))
      .then((wasmModule) => {
        const w = new Worker(
          new URL("./wasm-worker.ts", import.meta.url),
          { type: "module" }
        );

        w.onerror = (ev) => {
          const msg = ev.message || "Worker failed to load";
          rejectReady(new Error(msg));
          // Reject all pending calls
          for (const [id, cb] of pending) {
            cb.reject(new Error(msg));
            pending.delete(id);
          }
        };

        w.onmessage = (e: MessageEvent<WorkerResponse>) => {
          const data = e.data;

          if (data.type === "ready") {
            resolveReady();
            return;
          }

          // Pipeline progress — forward to store callback.
          if (data.type === "progress" && data.stage) {
            progressCallback?.(data.stage);
            return;
          }

          // Ignore fire-and-forget acknowledgements (prepare_image only).
          if (data.type === "prepared") return;

          // Resolve pending calls.
          if (data.id != null) {
            const cb = pending.get(data.id);
            if (!cb) return;
            pending.delete(data.id);
            if (data.error) {
              cb.reject(new Error(data.error));
            } else if (data.type === "result") {
              cb.resolve(data.result as ProcessResult);
            } else if (data.type === "base_data") {
              cb.resolve(data.baseData ?? null);
            } else if (data.type === "strengths") {
              cb.resolve(data.strengthsJson ?? "[]");
            } else if (data.type === "dense_result") {
              cb.resolve(data.denseResult ?? null);
            } else if (data.type === "base_prepared") {
              cb.resolve(undefined);
            }
          }
        };

        worker = w;
        w.postMessage({ type: "init", module: wasmModule } as WorkerRequest);

        // Eagerly initialize probe pool in background (non-blocking).
        initProbePool(wasmModule).catch(() => {
          // Pool unavailable — will fall back to single-worker probing.
        });
      })
      .catch((err) => {
        workerReadyPromise = null; // allow retry
        rejectReady(err);
      });
  });

  return workerReadyPromise;
}

// ---------------------------------------------------------------------------
// Eager WASM initialization — starts compiling immediately on import.
// ---------------------------------------------------------------------------

ensureWorker().catch(() => {
  // Silently ignore — will retry on first processImageAsync call.
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export async function processImageAsync(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string
): Promise<ProcessResult> {
  await ensureWorker();

  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });

    const msg: WorkerRequest = {
      type: "process",
      id,
      rgbaData,
      width,
      height,
      paramsJson,
    };
    // Input is structured-cloned (browser-optimized copy, store stays intact).
    // Output comes back via transfer in the worker's postMessage.
    worker!.postMessage(msg);
  });
}

/**
 * Pre-convert sRGB→linear in the worker and cache the result.
 *
 * Fire-and-forget: the next `processImageAsync` call with matching dimensions
 * will use the cached linear image, saving ~100-400ms of conversion time.
 */
export async function prepareImage(
  rgbaData: Uint8Array,
  width: number,
  height: number
): Promise<void> {
  await ensureWorker();
  worker!.postMessage({
    type: "prepare",
    rgbaData,
    width,
    height,
  } as WorkerRequest);
}

/**
 * Process with parallel probing via the probe worker pool.
 *
 * Supports all probe configs including TwoPass (two rounds of parallel probing):
 *   1. Ensure base is prepared with current params
 *   2. Resolve initial strengths via Rust (coarse for TwoPass, all for Explicit/Range)
 *   3. Fan out to probe pool
 *   4. For TwoPass: resolve dense window, run second round in parallel
 *   5. Merge samples, send to main worker for fit + sharpen
 *
 * Falls back to single-worker `processImageAsync` if pool is unavailable,
 * base preparation fails, or any step in the parallel path errors.
 */
export async function processImageParallel(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string,
): Promise<ProcessResult> {
  await ensureWorker();

  if (!isProbePoolReady()) {
    return processImageAsync(rgbaData, width, height, paramsJson);
  }

  try {
    return await runParallelPipeline(rgbaData, width, height, paramsJson);
  } catch {
    // Parallel path failed (stale cache, pool error, etc.) — fall back.
    return processImageAsync(rgbaData, width, height, paramsJson);
  }
}

/** Inner parallel pipeline — throws on any failure so caller can fall back. */
async function runParallelPipeline(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string,
): Promise<ProcessResult> {
  // Step 1: Ensure base is prepared with the CURRENT params.
  // This is a no-op if the cached base already matches (Rust-side check),
  // but refreshes it when params changed since the last prepareBaseImage call.
  progressCallback?.("preparing");
  await callWorker<void>({
    type: "prepare_base",
    rgbaData,
    width,
    height,
    paramsJson,
  });

  // Step 2: Get base data from main worker.
  const baseData = await getBaseData();
  if (!baseData) {
    throw new Error("base data unavailable after prepare_base");
  }

  // Distribute base data to probe workers (cached for subsequent rounds).
  await distributeBaseData(baseData);

  // Step 3: Resolve initial strengths via Rust (works for all configs).
  const initialStrengthsJson = await callWorker<string>({
    type: "resolve_initial_strengths",
    paramsJson,
  });
  const initialStrengths: number[] = JSON.parse(initialStrengthsJson);
  if (initialStrengths.length === 0) {
    throw new Error("no probe strengths resolved");
  }

  // Step 4: Run initial probes in parallel.
  progressCallback?.("probing");
  const t0 = performance.now();
  const { samplesJson: initialSamplesJson } = await runProbesParallel(
    initialStrengths, paramsJson,
  );

  // Step 5: For TwoPass, resolve dense window and run second round.
  let finalSamplesJson = initialSamplesJson;
  let passDiagnosticsJson = "";
  const denseResultStr = await callWorker<string | null>({
    type: "resolve_dense_strengths",
    coarseSamplesJson: initialSamplesJson,
    paramsJson,
    effectiveP0: baseData.effectiveP0,
  });

  if (denseResultStr != null) {
    const denseResult = JSON.parse(denseResultStr) as {
      strengths: number[];
      diagnostics: unknown;
    };
    if (denseResult.strengths.length > 0) {
      const { samplesJson: denseSamplesJson } = await runProbesParallel(
        denseResult.strengths, paramsJson,
      );
      // Merge coarse + dense, sort by strength, dedup.
      const coarse = JSON.parse(initialSamplesJson) as Array<{ strength: number }>;
      const dense = JSON.parse(denseSamplesJson) as Array<{ strength: number }>;
      const merged = [...coarse, ...dense]
        .sort((a, b) => a.strength - b.strength)
        .filter((s, i, arr) => i === 0 || Math.abs(s.strength - arr[i - 1].strength) >= 1e-5);
      finalSamplesJson = JSON.stringify(merged);
    }
    passDiagnosticsJson = JSON.stringify(denseResult.diagnostics);
  }
  const probingMs = performance.now() - t0;

  // Step 6: Send all probes to main worker for fit + sharpen.
  progressCallback?.("fitting");
  return callWorker<ProcessResult>({
    type: "process_from_probes",
    paramsJson,
    probesJson: finalSamplesJson,
    probingUs: Math.round(probingMs * 1000),
    passDiagnosticsJson,
  });
}

/** Get cached base data from the main worker. */
function getBaseData(): Promise<BaseData | null> {
  return callWorker<BaseData | null>({ type: "get_base_data" });
}

/** Send a request to the main worker and await its response. */
function callWorker<T>(msg: Omit<WorkerRequest, "id">): Promise<T> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    worker!.postMessage({ ...msg, id } as WorkerRequest);
  });
}

/**
 * Pre-compute the base image (resize + classify + baseline + evaluator).
 *
 * Returns a promise that resolves when the base is cached in the worker.
 * Can be called fire-and-forget (`.catch(() => {})`) at load time, or
 * awaited before processing to ensure the base is fresh.
 */
export async function prepareBaseImage(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string
): Promise<void> {
  await ensureWorker();
  return callWorker<void>({
    type: "prepare_base",
    rgbaData,
    width,
    height,
    paramsJson,
  });
}
