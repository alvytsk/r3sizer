import type { WorkerRequest, WorkerResponse } from "./wasm-worker";
import type { ProcessResult } from "@/types/wasm-types";
import wasmUrl from "./wasm-pkg/imgsharp_wasm_bg.wasm?url";

let worker: Worker | null = null;
let workerReadyPromise: Promise<void> | null = null;
let nextId = 0;
const pending = new Map<
  number,
  { resolve: (r: ProcessResult) => void; reject: (e: Error) => void }
>();

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

          if (data.type === "result" && data.id != null) {
            const cb = pending.get(data.id);
            if (!cb) return;
            pending.delete(data.id);
            if (data.error) {
              cb.reject(new Error(data.error));
            } else {
              cb.resolve(data.result as ProcessResult);
            }
          }
        };

        worker = w;
        w.postMessage({ type: "init", module: wasmModule } as WorkerRequest);
      })
      .catch((err) => {
        workerReadyPromise = null; // allow retry
        rejectReady(err);
      });
  });

  return workerReadyPromise;
}

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
