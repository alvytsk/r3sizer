import type { WorkerRequest, WorkerResponse } from "./wasm-worker";
import type { ProcessResult } from "@/types/wasm-types";
import type { BaseData } from "./probe-pool";
import { initProbePool, isProbePoolReady, runProbesParallel } from "./probe-pool";
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

          // Ignore fire-and-forget acknowledgements.
          if (data.type === "prepared" || data.type === "base_prepared") return;

          // Resolve pending calls (result or base_data).
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
 * 1. Gets cached base data from main worker
 * 2. Fans out probes to the pool
 * 3. Sends collected probes back to main worker for fit + sharpen
 *
 * Falls back to single-worker `processImageAsync` if pool is unavailable
 * or base data is not cached.
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

  // Step 1: Get base data from main worker.
  const baseData = await getBaseData();
  if (!baseData) {
    return processImageAsync(rgbaData, width, height, paramsJson);
  }

  // Step 2: Resolve probe strengths and fan out to pool.
  progressCallback?.("probing");
  const params = JSON.parse(paramsJson);
  const strengths = resolveProbeStrengths(params);

  const { samplesJson, probingMs } = await runProbesParallel(baseData, strengths, paramsJson);

  // Step 3: Send probes to main worker for fit + sharpen.
  progressCallback?.("fitting");
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, {
      resolve,
      reject,
    });
    worker!.postMessage({
      type: "process_from_probes",
      id,
      rgbaData,
      width,
      height,
      paramsJson,
      probesJson: samplesJson,
      probingUs: Math.round(probingMs * 1000),
    } as WorkerRequest);
  });
}

/** Get cached base data from the main worker. */
function getBaseData(): Promise<BaseData | null> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, {
      resolve,
      reject,
    });
    worker!.postMessage({ type: "get_base_data", id } as WorkerRequest);
  });
}

/** Resolve probe strengths from params (mirrors Rust ProbeConfig::resolve). */
function resolveProbeStrengths(params: Record<string, unknown>): number[] {
  const config = params.probe_strengths;
  if (Array.isArray(config)) return config;
  if (typeof config === "object" && config !== null) {
    const c = config as Record<string, unknown>;
    if (c.type === "two_pass" || c.TwoPass || c.coarse_count) {
      // TwoPass — can't easily pre-resolve from JS.
      // Fall back to single-worker for two-pass probing.
      return [];
    }
    if (c.type === "explicit" || c.Explicit) {
      const values = (c.values ?? c.Explicit) as number[];
      return Array.isArray(values) ? values : [];
    }
  }
  // Default: use a standard probe set.
  return [0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0];
}

/**
 * Pre-compute the base image (resize + classify + baseline + evaluator).
 *
 * Fire-and-forget: runs in the worker while the user adjusts params.
 * The next `processImageAsync` call skips ~1.5 s of base preparation
 * when the cached base dimensions match.
 */
export async function prepareBaseImage(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string
): Promise<void> {
  await ensureWorker();
  worker!.postMessage({
    type: "prepare_base",
    rgbaData,
    width,
    height,
    paramsJson,
  } as WorkerRequest);
}
