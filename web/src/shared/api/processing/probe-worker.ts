/**
 * Lightweight Web Worker for running probe batches.
 *
 * Each probe worker loads its own WASM instance and processes a subset of
 * sharpening strengths against a base image.
 *
 * Base data is sent once via `set_base` and cached for the duration of the
 * processing cycle, avoiding redundant structured clones per batch.
 */
import type { ProbeBundle } from "./probe-bundle";
import { initSync, probe_batch_with_detail } from "./wasm-pkg/r3sizer_wasm";

let ready = false;

/**
 * Everything a probe worker needs for one base: the bundle the main worker
 * minted (`get_probe_bundle`) plus the params it was minted for.
 *
 * Workers cache it as one record, so the buffers, the params and the stamp
 * can never come from different bases.
 */
export interface BaseData extends ProbeBundle {
  /** The params the bundle was minted for; every batch is measured with these. */
  paramsJson: string;
  /** Opaque stamp from `get_probe_bundle`; every batch made from this base carries it. */
  provenanceJson: string;
}

export interface ProbeWorkerRequest {
  type: "init" | "set_base" | "probe" | "clear_base";
  module?: WebAssembly.Module;
  id?: number;
  baseData?: BaseData;
  strengths?: number[];
}

export interface ProbeWorkerResponse {
  type: "ready" | "base_cached" | "probe_result";
  id?: number;
  /** One JSON `ProbeBatch` (the core type): its provenance and its samples. */
  batchJson?: string;
  error?: string;
}

/** Cached base — set once via `set_base`, reused across probes. */
let cachedBase: BaseData | null = null;

/**
 * Measure `strengths` on `base`.
 *
 * The argument order is the wasm binding's. In Lightness mode `luminance` and
 * `detail` are both W×H floats, so a swap here would compile and run.
 */
export function runProbeBatch(base: BaseData, strengths: number[]): string {
  return probe_batch_with_detail(
    base.basePixels,
    base.width,
    base.height,
    base.luminance,
    base.detail,
    JSON.stringify(strengths),
    base.paramsJson,
    base.baseline,
    base.provenanceJson,
  );
}

self.onmessage = (e: MessageEvent<ProbeWorkerRequest>) => {
  const msg = e.data;

  if (msg.type === "init") {
    initSync(msg.module!);
    ready = true;
    (self as unknown as Worker).postMessage({ type: "ready" } as ProbeWorkerResponse);
    return;
  }

  if (msg.type === "clear_base") {
    cachedBase = null;
    return;
  }

  if (msg.type === "set_base") {
    cachedBase = msg.baseData ?? null;
    (self as unknown as Worker).postMessage({
      type: "base_cached",
      id: msg.id,
    } as ProbeWorkerResponse);
    return;
  }

  if (msg.type === "probe") {
    const { id, strengths } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      if (!cachedBase) throw new Error("No base data — call set_base first");

      const batchJson = runProbeBatch(cachedBase, strengths ?? []);

      const resp: ProbeWorkerResponse = { type: "probe_result", id, batchJson };
      (self as unknown as Worker).postMessage(resp);
    } catch (err) {
      const resp: ProbeWorkerResponse = {
        type: "probe_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(resp);
    }
  }
};
