import { create } from "zustand";
import type {
  AutoSharpParams,
  AutoSharpDiagnostics,
  ProcessResult,
} from "@/types/wasm-types";
import { DEFAULT_PARAMS } from "@/types/wasm-types";
import { initWasm, process_image } from "@/wasm";

interface ProcessorState {
  // Input
  inputFile: File | null;
  inputRgbaData: Uint8Array | null;
  inputWidth: number;
  inputHeight: number;

  // Parameters
  params: AutoSharpParams;
  preserveAspectRatio: boolean;

  // Processing
  isProcessing: boolean;
  error: string | null;

  // Output
  outputRgbaData: Uint8Array | null;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics | null;
  lastProcessedParams: AutoSharpParams | null;

  // Actions
  setInput: (
    file: File,
    rgbaData: Uint8Array,
    width: number,
    height: number
  ) => void;
  updateParams: (partial: Partial<AutoSharpParams>) => void;
  setPreserveAspectRatio: (v: boolean) => void;
  process: () => Promise<void>;
  reset: () => void;
}

export const useProcessorStore = create<ProcessorState>((set, get) => ({
  inputFile: null,
  inputRgbaData: null,
  inputWidth: 0,
  inputHeight: 0,

  params: { ...DEFAULT_PARAMS },
  preserveAspectRatio: true,

  isProcessing: false,
  error: null,

  outputRgbaData: null,
  outputWidth: 0,
  outputHeight: 0,
  diagnostics: null,
  lastProcessedParams: null,

  setInput: (file, rgbaData, width, height) => {
    const state = get();
    const params = { ...state.params };

    if (state.preserveAspectRatio) {
      const aspect = width / height;
      if (params.target_width && !params.target_height) {
        params.target_height = Math.round(params.target_width / aspect);
      } else if (params.target_height && !params.target_width) {
        params.target_width = Math.round(params.target_height * aspect);
      } else {
        params.target_height = Math.round(params.target_width / aspect);
      }
    }

    set({
      inputFile: file,
      inputRgbaData: rgbaData,
      inputWidth: width,
      inputHeight: height,
      params,
      outputRgbaData: null,
      outputWidth: 0,
      outputHeight: 0,
      diagnostics: null,
      error: null,
    });
  },

  updateParams: (partial) => {
    const state = get();
    const newParams = { ...state.params, ...partial };

    if (state.preserveAspectRatio && state.inputWidth > 0) {
      const aspect = state.inputWidth / state.inputHeight;
      if ("target_width" in partial && !("target_height" in partial)) {
        newParams.target_height = Math.round(newParams.target_width / aspect);
      } else if ("target_height" in partial && !("target_width" in partial)) {
        newParams.target_width = Math.round(newParams.target_height * aspect);
      }
    }

    set({ params: newParams });
  },

  setPreserveAspectRatio: (v) => {
    set({ preserveAspectRatio: v });
    if (v) {
      const state = get();
      if (state.inputWidth > 0) {
        const aspect = state.inputWidth / state.inputHeight;
        const newParams = { ...state.params };
        newParams.target_height = Math.round(newParams.target_width / aspect);
        set({ params: newParams });
      }
    }
  },

  process: async () => {
    const state = get();
    if (!state.inputRgbaData) {
      set({ error: "No image loaded" });
      return;
    }

    set({ isProcessing: true, error: null });

    try {
      await initWasm();
      const paramsJson = JSON.stringify(state.params);
      const result = process_image(
        state.inputRgbaData,
        state.inputWidth,
        state.inputHeight,
        paramsJson
      ) as ProcessResult;

      set({
        outputRgbaData: result.imageData,
        outputWidth: result.outputWidth,
        outputHeight: result.outputHeight,
        diagnostics: result.diagnostics,
        lastProcessedParams: { ...state.params },
        isProcessing: false,
      });
    } catch (e) {
      set({
        error: e instanceof Error ? e.message : String(e),
        isProcessing: false,
      });
    }
  },

  reset: () =>
    set({
      inputFile: null,
      inputRgbaData: null,
      inputWidth: 0,
      inputHeight: 0,
      params: { ...DEFAULT_PARAMS },
      preserveAspectRatio: true,
      isProcessing: false,
      error: null,
      outputRgbaData: null,
      outputWidth: 0,
      outputHeight: 0,
      diagnostics: null,
      lastProcessedParams: null,
    }),
}));
