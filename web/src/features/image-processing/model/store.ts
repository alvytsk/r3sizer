import { create } from "zustand";
import { useImageStore } from "@/entities/image";
import { useOutputStore } from "@/entities/output";
import { CancelledError, type ProcessJob, processingClient } from "@/shared/api/processing";

/**
 * Image-processing feature — owns the process/cancel lifecycle only.
 *
 * Reads the current image + params from the image entity, runs the pipeline
 * via the shared WASM API, and writes the result to the output entity. It holds
 * no result state itself, so consumers (export feature, preview/diagnostics
 * widgets) read the result from `entities/output` without feature-to-feature
 * coupling.
 *
 * One active job at a time: starting a new process while another is running
 * cancels the previous one.
 */
interface ProcessingState {
  isProcessing: boolean;
  progress: { stage: string; overall: number } | null;
  error: string | null;

  // Actions
  process: () => Promise<void>;
  cancelProcessing: () => void;
  setProcessingError: (message: string) => void;
  resetProcessing: () => void;
}

/** Active job handle — not UI state, so kept outside the store. */
let currentJob: ProcessJob | null = null;

export const useProcessingStore = create<ProcessingState>((set) => ({
  isProcessing: false,
  progress: null,
  error: null,

  process: async () => {
    const { params, inputFile } = useImageStore.getState();
    if (!inputFile) {
      set({ error: "No image loaded" });
      return;
    }

    set({ isProcessing: true, progress: null, error: null });

    try {
      const job = processingClient.process(params);
      currentJob = job;
      job.onProgress(({ stage, overall }) => set({ progress: { stage, overall } }));
      const result = await job.promise;

      useOutputStore.getState().setResult({
        imageData: result.imageData,
        outputWidth: result.outputWidth,
        outputHeight: result.outputHeight,
        diagnostics: result.diagnostics,
        params,
        paramsVersion: useImageStore.getState().paramsVersion,
      });

      set({ isProcessing: false, progress: null });
    } catch (e) {
      if (e instanceof CancelledError) {
        set({ isProcessing: false, progress: null });
      } else {
        set({
          error: e instanceof Error ? e.message : String(e),
          isProcessing: false,
          progress: null,
        });
      }
    } finally {
      currentJob = null;
    }
  },

  cancelProcessing: () => {
    currentJob?.cancel();
  },

  setProcessingError: (message) => set({ error: message }),

  resetProcessing: () => {
    currentJob?.cancel();
    useOutputStore.getState().clearOutput();
    set({ isProcessing: false, progress: null, error: null });
  },
}));
