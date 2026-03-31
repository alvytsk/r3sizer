/**
 * Lightweight Web Worker for running probe batches.
 *
 * Each probe worker loads its own WASM instance and processes a subset of
 * sharpening strengths against a base image.
 *
 * Base image data is sent once via `set_base` and cached for the duration of
 * the processing cycle, avoiding redundant structured clones per batch.
 */
import { initSync, probe_batch } from "./wasm-pkg/r3sizer_wasm";

let ready = false;

/** Cached base image data — set once via `set_base`, reused across probes. */
let cachedBase: {
  pixels: Float32Array;
  luminance: Float32Array;
  width: number;
  height: number;
  baseline: number;
} | null = null;

export interface ProbeWorkerRequest {
  type: "init" | "set_base" | "probe" | "clear_base";
  module?: WebAssembly.Module;
  id?: number;
  basePixels?: Float32Array;
  luminance?: Float32Array;
  width?: number;
  height?: number;
  strengths?: number[];
  paramsJson?: string;
  baseline?: number;
}

export interface ProbeWorkerResponse {
  type: "ready" | "base_cached" | "probe_result";
  id?: number;
  samplesJson?: string;
  error?: string;
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
    const { id, basePixels, luminance, width, height, baseline } = msg;
    cachedBase = {
      pixels: basePixels!,
      luminance: luminance!,
      width: width!,
      height: height!,
      baseline: baseline!,
    };
    (self as unknown as Worker).postMessage({ type: "base_cached", id } as ProbeWorkerResponse);
    return;
  }

  if (msg.type === "probe") {
    const { id, strengths, paramsJson } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      if (!cachedBase) throw new Error("No base data — call set_base first");

      const samplesJson = probe_batch(
        cachedBase.pixels,
        cachedBase.width,
        cachedBase.height,
        cachedBase.luminance,
        JSON.stringify(strengths),
        paramsJson!,
        cachedBase.baseline,
      );

      const resp: ProbeWorkerResponse = { type: "probe_result", id, samplesJson };
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
