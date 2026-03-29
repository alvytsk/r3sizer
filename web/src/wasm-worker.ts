import { initSync, process_image, prepare_image, clear_cache } from "./wasm-pkg/r3sizer_wasm";

let ready = false;

export interface WorkerRequest {
  type: "init" | "process" | "prepare";
  module?: WebAssembly.Module;
  id?: number;
  rgbaData?: Uint8Array;
  width?: number;
  height?: number;
  paramsJson?: string;
}

export interface WorkerResponse {
  type: "ready" | "result" | "prepared" | "progress";
  id?: number;
  stage?: string;
  result?: {
    imageData: Uint8Array;
    outputWidth: number;
    outputHeight: number;
    diagnostics: unknown;
  };
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
