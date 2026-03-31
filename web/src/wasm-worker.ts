import {
  initSync, process_image, prepare_image, prepare_base,
  get_base_data, process_from_probes, clear_cache,
} from "./wasm-pkg/r3sizer_wasm";

let ready = false;

export interface WorkerRequest {
  type: "init" | "process" | "prepare" | "prepare_base" | "get_base_data" | "process_from_probes";
  module?: WebAssembly.Module;
  id?: number;
  rgbaData?: Uint8Array;
  width?: number;
  height?: number;
  paramsJson?: string;
  probesJson?: string;
  probingUs?: number;
}

export interface WorkerResponse {
  type: "ready" | "result" | "prepared" | "base_prepared" | "base_data" | "progress";
  id?: number;
  stage?: string;
  result?: {
    imageData: Uint8Array;
    outputWidth: number;
    outputHeight: number;
    diagnostics: unknown;
  };
  baseData?: {
    basePixels: Float32Array;
    luminance: Float32Array;
    width: number;
    height: number;
    baseline: number;
    effectiveP0: number;
  } | null;
  error?: string;
}

self.onmessage = (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;

  if (msg.type === "init") {
    initSync(msg.module!);
    ready = true;
    (self as unknown as Worker).postMessage({ type: "ready" } as WorkerResponse);
    return;
  }

  if (msg.type === "prepare") {
    if (!ready) return;
    const { rgbaData, width, height } = msg;
    try {
      prepare_image(rgbaData!, width!, height!);
    } catch {
      // Silently ignore — process_image will convert fresh if cache misses.
      clear_cache();
    }
    (self as unknown as Worker).postMessage({ type: "prepared" } as WorkerResponse);
    return;
  }

  if (msg.type === "prepare_base") {
    if (!ready) return;
    const { rgbaData, width, height, paramsJson } = msg;
    try {
      prepare_base(rgbaData!, width!, height!, paramsJson!);
    } catch {
      // Non-fatal — process_image falls back to full pipeline.
    }
    (self as unknown as Worker).postMessage({ type: "base_prepared" } as WorkerResponse);
    return;
  }

  if (msg.type === "get_base_data") {
    const { id } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const data = get_base_data();
      const resp: WorkerResponse = {
        type: "base_data",
        id,
        baseData: data ? {
          basePixels: data.basePixels,
          luminance: data.luminance,
          width: data.width,
          height: data.height,
          baseline: data.baseline,
          effectiveP0: data.effectiveP0,
        } : null,
      };
      (self as unknown as Worker).postMessage(resp);
    } catch (err) {
      const resp: WorkerResponse = {
        type: "base_data",
        id,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(resp);
    }
    return;
  }

  if (msg.type === "process_from_probes") {
    const { id, rgbaData, width, height, paramsJson, probesJson, probingUs } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");

      const result = process_from_probes(rgbaData!, width!, height!, paramsJson!, probesJson!, probingUs!);
      const resp: WorkerResponse = {
        type: "result",
        id,
        result: {
          imageData: result.imageData,
          outputWidth: result.outputWidth,
          outputHeight: result.outputHeight,
          diagnostics: result.diagnostics,
        },
      };
      (self as unknown as Worker).postMessage(resp, [result.imageData.buffer]);
    } catch (err) {
      const resp: WorkerResponse = {
        type: "result",
        id,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(resp);
    }
    return;
  }

  if (msg.type === "process") {
    const { id, rgbaData, width, height, paramsJson } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");

      const result = process_image(rgbaData!, width!, height!, paramsJson!);
      const resp: WorkerResponse = {
        type: "result",
        id,
        result: {
          imageData: result.imageData,
          outputWidth: result.outputWidth,
          outputHeight: result.outputHeight,
          diagnostics: result.diagnostics,
        },
      };
      // Transfer only the output buffer (fresh allocation, zero-copy back)
      (self as unknown as Worker).postMessage(resp, [result.imageData.buffer]);
    } catch (err) {
      const resp: WorkerResponse = {
        type: "result",
        id,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(resp);
    }
  }
};
