/**
 * Lightweight Web Worker for running probe batches.
 *
 * Each probe worker loads its own WASM instance and processes a subset of
 * sharpening strengths against a base image received from the main worker.
 */
import { initSync, probe_batch } from "./wasm-pkg/r3sizer_wasm";

let ready = false;

export interface ProbeWorkerRequest {
  type: "init" | "probe";
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
  type: "ready" | "probe_result";
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

  if (msg.type === "probe") {
    const { id, basePixels, luminance, width, height, strengths, paramsJson, baseline } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");

      const samplesJson = probe_batch(
        basePixels!,
        width!,
        height!,
        luminance!,
        JSON.stringify(strengths),
        paramsJson!,
        baseline!,
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
