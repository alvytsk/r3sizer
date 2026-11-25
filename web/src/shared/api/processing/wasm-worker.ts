import type { MetadataExportRequest, MetadataExportResponse } from "@/shared/lib";
import {
  clear_cache,
  get_probe_bundle,
  ingest_abort,
  ingest_begin,
  ingest_end,
  ingest_stripe,
  initSync,
  prepare_base,
  prepare_image,
  preserve_metadata,
  process_from_probes,
  process_image,
  resolve_dense_strengths,
  resolve_initial_strengths,
} from "./wasm-pkg/r3sizer_wasm";

let ready = false;

/**
 * What `get_probe_bundle` minted for one base: its pixels and luminance, the
 * detail signal, and the provenance stamp every batch made from them carries.
 */
export interface ProbeBundle {
  basePixels: Float32Array;
  luminance: Float32Array;
  width: number;
  height: number;
  baseline: number;
  effectiveP0: number;
  detail: Float32Array;
  provenanceJson: string;
}

export interface WorkerRequest {
  type:
    | "init"
    | "process"
    | "prepare"
    | "prepare_base"
    | "get_probe_bundle"
    | "process_from_probes"
    | "resolve_initial_strengths"
    | "resolve_dense_strengths"
    | "clear_cache"
    | "ingest_begin"
    | "ingest_stripe"
    | "ingest_end"
    | "ingest_abort"
    | "metadata_export";
  module?: WebAssembly.Module;
  id?: number;
  rgbaData?: Uint8Array;
  width?: number;
  height?: number;
  paramsJson?: string;
  /** One JSON `ProbeBatch`: the pool's batches merged under one stamp. */
  batchJson?: string;
  probingUs?: number;
  passDiagnosticsJson?: string;
  coarseSamplesJson?: string;
  effectiveP0?: number;
  targetWidth?: number;
  targetHeight?: number;
  rows?: number;
  metadataRequest?: MetadataExportRequest;
}

export interface WorkerResponse {
  type:
    | "ready"
    | "result"
    | "prepared"
    | "base_prepared"
    | "probe_bundle"
    | "progress"
    | "strengths"
    | "dense_result"
    | "cache_cleared"
    | "ingest_result"
    | "metadata_exported";
  id?: number;
  stage?: string;
  result?: {
    imageData: Uint8Array;
    outputWidth: number;
    outputHeight: number;
    diagnostics: unknown;
  };
  bundle?: ProbeBundle;
  strengthsJson?: string;
  denseResult?: string | null;
  ingest?: { width: number; height: number } | null;
  metadataResponse?: MetadataExportResponse;
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

  if (msg.type === "clear_cache") {
    const { id } = msg;
    if (ready) clear_cache();
    (self as unknown as Worker).postMessage({ type: "cache_cleared", id } as WorkerResponse);
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
    const { id, rgbaData, width, height, paramsJson } = msg;
    try {
      prepare_base(rgbaData!, width!, height!, paramsJson!);
    } catch {
      // Non-fatal — process_image falls back to full pipeline.
    }
    (self as unknown as Worker).postMessage({ type: "base_prepared", id } as WorkerResponse);
    return;
  }

  if (msg.type === "ingest_begin") {
    const { id, width, height, targetWidth, targetHeight } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const dims = ingest_begin(width!, height!, targetWidth!, targetHeight!) as {
        width: number;
        height: number;
      };
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        ingest: dims,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_stripe") {
    const { id, rgbaData, rows } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      ingest_stripe(rgbaData!, rows!);
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_end") {
    const { id } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const dims = ingest_end() as { width: number; height: number };
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        ingest: dims,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_abort") {
    // Fire-and-forget: drop partial ingest state.
    if (ready) ingest_abort();
    return;
  }

  if (msg.type === "metadata_export") {
    const { id } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const response = preserve_metadata(msg.metadataRequest!) as MetadataExportResponse;
      (self as unknown as Worker).postMessage(
        { type: "metadata_exported", id, metadataResponse: response } satisfies WorkerResponse,
        [response.bytes.buffer as ArrayBuffer],
      );
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "metadata_exported",
        id,
        error: err instanceof Error ? err.message : String(err),
      } satisfies WorkerResponse);
    }
    return;
  }

  if (msg.type === "get_probe_bundle") {
    const { id, paramsJson } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const bundle = get_probe_bundle(paramsJson!) as ProbeBundle;
      (self as unknown as Worker).postMessage({
        type: "probe_bundle",
        id,
        bundle,
      } satisfies WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "probe_bundle",
        id,
        error: err instanceof Error ? err.message : String(err),
      } satisfies WorkerResponse);
    }
    return;
  }

  if (msg.type === "process_from_probes") {
    const { id, paramsJson, batchJson, probingUs, passDiagnosticsJson } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");

      const result = process_from_probes(
        paramsJson!,
        batchJson!,
        probingUs!,
        passDiagnosticsJson ?? "",
      );
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

  if (msg.type === "resolve_initial_strengths") {
    const { id, paramsJson } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const json = resolve_initial_strengths(paramsJson!);
      const resp: WorkerResponse = { type: "strengths", id, strengthsJson: json };
      (self as unknown as Worker).postMessage(resp);
    } catch (err) {
      const resp: WorkerResponse = {
        type: "strengths",
        id,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(resp);
    }
    return;
  }

  if (msg.type === "resolve_dense_strengths") {
    const { id, coarseSamplesJson, paramsJson, effectiveP0 } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const result = resolve_dense_strengths(coarseSamplesJson!, paramsJson!, effectiveP0!);
      const resp: WorkerResponse = {
        type: "dense_result",
        id,
        denseResult: result != null ? result : null,
      };
      (self as unknown as Worker).postMessage(resp);
    } catch (err) {
      const resp: WorkerResponse = {
        type: "dense_result",
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
