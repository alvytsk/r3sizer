/**
 * Pool of probe workers for parallel sharpening strength evaluation.
 *
 * Distributes probe strengths across N Web Workers, each running its own WASM
 * instance. Falls back gracefully to single-worker processing if the pool
 * can't be initialized.
 */
import type { ProbeWorkerRequest, ProbeWorkerResponse } from "./probe-worker";

/** Base image data extracted from the main worker's PreparedBase. */
export interface BaseData {
  basePixels: Float32Array;
  luminance: Float32Array;
  width: number;
  height: number;
  baseline: number;
  effectiveP0: number;
}

/** Result of a parallel probe batch. */
export interface ProbePoolResult {
  /** JSON array of merged ProbeSamples, sorted by strength. */
  samplesJson: string;
  /** Wall-clock time of the parallel probing phase (ms). */
  probingMs: number;
}

const DEFAULT_POOL_SIZE = Math.min(navigator.hardwareConcurrency || 4, 6);

let pool: Worker[] = [];
let poolReady: Promise<void> | null = null;
let nextProbeId = 0;

/**
 * Initialize the probe pool with a pre-compiled WASM module.
 *
 * Call this once after the main worker's WASM module is compiled.
 * The same module is shared across all probe workers (no recompilation).
 */
export function initProbePool(
  module: WebAssembly.Module,
  size: number = DEFAULT_POOL_SIZE,
): Promise<void> {
  if (poolReady) return poolReady;

  poolReady = new Promise<void>((resolve, reject) => {
    let initialized = 0;
    const timeout = setTimeout(() => {
      reject(new Error("Probe pool init timed out"));
    }, 10_000);

    for (let i = 0; i < size; i++) {
      const w = new Worker(
        new URL("./probe-worker.ts", import.meta.url),
        { type: "module" },
      );

      w.onmessage = (e: MessageEvent<ProbeWorkerResponse>) => {
        if (e.data.type === "ready") {
          initialized++;
          if (initialized === size) {
            clearTimeout(timeout);
            resolve();
          }
        }
      };

      w.onerror = () => {
        clearTimeout(timeout);
        reject(new Error("Probe worker failed to load"));
      };

      w.postMessage({ type: "init", module } as ProbeWorkerRequest);
      pool.push(w);
    }
  });

  return poolReady;
}

/** Whether the probe pool is ready. */
export function isProbePoolReady(): boolean {
  return pool.length > 0 && poolReady !== null;
}

/**
 * Run probes in parallel across the pool.
 *
 * Splits `strengths` into chunks, sends each chunk to a worker with the base
 * image data, and merges the results.
 */
export async function runProbesParallel(
  baseData: BaseData,
  strengths: number[],
  paramsJson: string,
): Promise<ProbePoolResult> {
  if (!poolReady || pool.length === 0) {
    throw new Error("Probe pool not initialized");
  }
  await poolReady;

  const t0 = performance.now();
  const n = pool.length;
  const chunks = splitStrengths(strengths, n);

  const promises = chunks.map((chunk, i) => {
    if (chunk.length === 0) return Promise.resolve("[]");
    return runProbeOnWorker(pool[i], chunk, baseData, paramsJson);
  });

  const results = await Promise.all(promises);
  const probingMs = performance.now() - t0;

  // Merge all ProbeSample arrays and sort by strength.
  const merged = results
    .flatMap((json) => JSON.parse(json) as Array<{ strength: number }>)
    .sort((a, b) => a.strength - b.strength);

  return {
    samplesJson: JSON.stringify(merged),
    probingMs,
  };
}

/** Distribute strengths as evenly as possible across N workers. */
function splitStrengths(strengths: number[], n: number): number[][] {
  const chunks: number[][] = Array.from({ length: n }, () => []);
  for (let i = 0; i < strengths.length; i++) {
    chunks[i % n].push(strengths[i]);
  }
  return chunks;
}

/** Send a probe batch to one worker and await its result. */
function runProbeOnWorker(
  worker: Worker,
  strengths: number[],
  baseData: BaseData,
  paramsJson: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const id = nextProbeId++;

    const handler = (e: MessageEvent<ProbeWorkerResponse>) => {
      if (e.data.type !== "probe_result" || e.data.id !== id) return;
      worker.removeEventListener("message", handler);
      if (e.data.error) {
        reject(new Error(e.data.error));
      } else {
        resolve(e.data.samplesJson!);
      }
    };

    worker.addEventListener("message", handler);

    const msg: ProbeWorkerRequest = {
      type: "probe",
      id,
      basePixels: baseData.basePixels,
      luminance: baseData.luminance,
      width: baseData.width,
      height: baseData.height,
      strengths,
      paramsJson,
      baseline: baseData.baseline,
    };
    worker.postMessage(msg);
  });
}

/** Terminate all probe workers. */
export function destroyProbePool(): void {
  for (const w of pool) w.terminate();
  pool = [];
  poolReady = null;
}
