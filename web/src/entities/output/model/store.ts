import { create } from "zustand";
import type { AutoSharpParams, AutoSharpDiagnostics } from "@/shared/types/wasm-types";

/**
 * Output entity — the processed result (sharpened image + diagnostics).
 *
 * Modelled as an entity (not processing-feature state) so that several slices
 * can read it without cross-feature coupling: the image-processing feature
 * produces it, while the export feature and the preview/diagnostics widgets
 * consume it — all via this entity, never feature-to-feature.
 */
export interface OutputResult {
  imageData: Uint8Array;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics;
  params: AutoSharpParams;
  paramsVersion: number;
}

interface OutputState {
  outputRgbaData: Uint8Array | null;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics | null;
  lastProcessedParams: AutoSharpParams | null;
  lastProcessedVersion: number;

  setResult: (r: OutputResult) => void;
  clearOutput: () => void;
}

const CLEARED = {
  outputRgbaData: null,
  outputWidth: 0,
  outputHeight: 0,
  diagnostics: null,
  lastProcessedParams: null,
  lastProcessedVersion: 0,
} as const;

export const useOutputStore = create<OutputState>((set) => ({
  ...CLEARED,
  setResult: (r) =>
    set({
      outputRgbaData: r.imageData,
      outputWidth: r.outputWidth,
      outputHeight: r.outputHeight,
      diagnostics: r.diagnostics,
      lastProcessedParams: { ...r.params },
      lastProcessedVersion: r.paramsVersion,
    }),
  clearOutput: () => set({ ...CLEARED }),
}));
