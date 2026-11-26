/**
 * Pool of probe workers for parallel sharpening strength evaluation.
 *
 * Distributes probe strengths across N Web Workers, each running its own WASM
 * instance. Falls back gracefully to single-worker processing if the pool
 * can't be initialized.
 *
 * The base is sent once via `distributeBaseData` and cached in each worker as
 * one record (buffers, params and provenance stamp), avoiding redundant
 * structured clones on every probe batch.
 */

import type { ProbeBatch } from "@/shared/lib";
import type { CancellationToken } from "./errors";
import type { BaseData, ProbeWorkerRequest, ProbeWorkerResponse } from "./probe-worker";

/** Result of a parallel probe round. */
export interface ProbePoolResult {
  /** One JSON `ProbeBatch`: the round's batches merged under their shared stamp. */
  batchJson: string;
  /** Wall-clock time of the round (ms). */
  probingMs: number;
}

const DEFAULT_POOL_SIZE = Math.min(navigator.hardwareConcurrency || 4, 6);

/** Timeout for individual probe operations (ms). */
const PROBE_TIMEOUT = 30_000;

let pool: Worker[] = [];
let poolReady: Promise<void> | null = null;
let nextProbeId = 0;
/** Whether workers have current base data cached. */
let workersHaveBase = false;

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
      poolReady = null; // allow retry on next call
      reject(new Error("Probe pool init timed out"));
    }, 10_000);

    for (let i = 0; i < size; i++) {
      const w = new Worker(new URL("./probe-worker.ts", import.meta.url), { type: "module" });

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
        poolReady = null; // allow retry on next call
        pool = []; // discard partially-initialized workers
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
 * Send one base to all probe workers for caching.
 *
 * Call this once per processing cycle after `get_probe_bundle`. Workers cache
 * the record and reuse it across multiple `runProbesParallel` calls (e.g.
 * coarse and dense rounds in TwoPass mode).
 */
export async function distributeBaseData(baseData: BaseData): Promise<void> {
  if (!poolReady || pool.length === 0) {
    throw new Error("Probe pool not initialized");
  }
  await poolReady;

  const promises = pool.map((w) => {
    return new Promise<void>((resolve, reject) => {
      const id = nextProbeId++;
      const timer = setTimeout(() => {
        w.removeEventListener("message", handler);
        reject(new Error("Probe worker set_base timed out"));
      }, PROBE_TIMEOUT);
      const handler = (e: MessageEvent<ProbeWorkerResponse>) => {
        if (e.data.type !== "base_cached" || e.data.id !== id) return;
        clearTimeout(timer);
        w.removeEventListener("message", handler);
        resolve();
      };
      w.addEventListener("message", handler);
      w.postMessage({ type: "set_base", id, baseData } as ProbeWorkerRequest);
    });
  });

  await Promise.all(promises);
  workersHaveBase = true;
}

/**
 * Merge the batches of one or more probe rounds into a single `ProbeBatch`.
 *
 * Every batch must carry the same provenance: a worker still holding the
 * previous base would otherwise slip samples of another base into the fit.
 * Samples are sorted by strength and near-duplicates (within 1e-5) removed.
 */
export function mergeProbeBatches(batchJsons: string[]): string {
  if (batchJsons.length === 0) throw new Error("no probe batches to merge");
  const batches = batchJsons.map((json) => JSON.parse(json) as ProbeBatch);
  const [first, ...rest] = batches;
  const stamp = JSON.stringify(first.provenance);
  for (const other of rest) {
    if (JSON.stringify(other.provenance) !== stamp) {
      throw new Error("probe batches disagree on provenance — the base changed while probing");
    }
  }
  const merged: ProbeBatch = {
    provenance: first.provenance,
    samples: batches
      .flatMap((b) => b.samples)
      .sort((a, b) => a.strength - b.strength)
      .filter((s, i, arr) => i === 0 || Math.abs(s.strength - arr[i - 1].strength) >= 1e-5),
  };
  return JSON.stringify(merged);
}

export interface ProbeRunOpts {
  token?: CancellationToken;
  /** Called as each worker's chunk completes: (probesDone, probesTotal). */
  onChunkDone?: (done: number, total: number) => void;
}

/**
 * Run one round of probes in parallel across the pool.
 *
 * Workers must hold the base via `distributeBaseData` first. Only strengths
 * are sent per batch: each worker probes with the record it was given.
 */
export async function runProbesParallel(
  strengths: number[],
  opts: ProbeRunOpts = {},
): Promise<ProbePoolResult> {
  if (!poolReady || pool.length === 0) {
    throw new Error("Probe pool not initialized");
  }
  if (!workersHaveBase) {
    throw new Error("Base data not distributed — call distributeBaseData first");
  }
  await poolReady;

  const t0 = performance.now();
  const n = pool.length;
  const chunks = splitStrengths(strengths, n);

  let done = 0;
  const promises = chunks.map((chunk, i) => {
    if (chunk.length === 0) return Promise.resolve(null);
    opts.token?.throwIfCancelled();
    return runProbeOnWorker(pool[i], chunk).then((batchJson) => {
      done += chunk.length;
      opts.onChunkDone?.(done, strengths.length);
      return batchJson;
    });
  });

  const results = await Promise.all(promises);
  const probingMs = performance.now() - t0;

  const batchJson = mergeProbeBatches(results.filter((b): b is string => b !== null));
  return { batchJson, probingMs };
}

/** Distribute strengths as evenly as possible across N workers. */
function splitStrengths(strengths: number[], n: number): number[][] {
  const chunks: number[][] = Array.from({ length: n }, () => []);
  for (let i = 0; i < strengths.length; i++) {
    chunks[i % n].push(strengths[i]);
  }
  return chunks;
}

/** Send a probe batch to one worker and await its stamped batch. */
function runProbeOnWorker(worker: Worker, strengths: number[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const id = nextProbeId++;
    const timer = setTimeout(() => {
      worker.removeEventListener("message", handler);
      reject(new Error("Probe worker timed out"));
    }, PROBE_TIMEOUT);

    const handler = (e: MessageEvent<ProbeWorkerResponse>) => {
      if (e.data.type !== "probe_result" || e.data.id !== id) return;
      clearTimeout(timer);
      worker.removeEventListener("message", handler);
      if (e.data.error) {
        reject(new Error(e.data.error));
      } else {
        resolve(e.data.batchJson!);
      }
    };

    worker.addEventListener("message", handler);

    const msg: ProbeWorkerRequest = { type: "probe", id, strengths };
    worker.postMessage(msg);
  });
}

/** Clear cached base data in all probe workers (e.g. when switching images). */
export function resetBaseCache(): void {
  for (const w of pool) {
    w.postMessage({ type: "clear_base" } as ProbeWorkerRequest);
  }
  workersHaveBase = false;
}

/** Terminate all probe workers. */
export function destroyProbePool(): void {
  for (const w of pool) w.terminate();
  pool = [];
  poolReady = null;
  workersHaveBase = false;
}
