import { create } from "zustand";
import type { DecodedInput } from "@/shared/api/processing";
import { processingClient } from "@/shared/api/processing";
import { loadDimsForOrientation, saveDims } from "@/shared/lib/prefs-storage";
import type { AutoSharpParams } from "@/shared/types/wasm-types";
import { DEFAULT_PARAMS } from "@/shared/types/wasm-types";

/**
 * Image entity — the decoded input image together with the sharpen/resize
 * parameters that apply to it. Params are co-located here (rather than in a
 * separate "params" entity) because they are tightly coupled to the image via
 * aspect-ratio dimension math, and are meaningless without an image loaded.
 *
 * `error` here covers input/decode errors; processing-run errors live in the
 * image-processing feature store.
 */
interface ImageState {
  // Input
  inputFile: File | null;
  /** Original source dimensions (used for aspect math and labels). */
  sourceWidth: number;
  sourceHeight: number;
  striped: boolean;
  /** Displayable pixels: full-size for monolithic, downscaled for striped. */
  previewRgbaData: Uint8Array | null;
  previewWidth: number;
  previewHeight: number;

  // Parameters (belong to this image)
  params: AutoSharpParams;
  preserveAspectRatio: boolean;
  lockDimensions: boolean;
  /** Cheap dirty counter — bumped on every param change. */
  paramsVersion: number;

  /** Input/decode error (null when healthy). */
  error: string | null;

  // Actions
  setInput: (file: File) => Promise<void>;
  updateParams: (partial: Partial<AutoSharpParams>) => void;
  setPreserveAspectRatio: (v: boolean) => void;
  setLockDimensions: (v: boolean) => void;
  clearError: () => void;
  resetImage: () => void;
}

const initDims = loadDimsForOrientation(false); // landscape default for pre-load state

export const useImageStore = create<ImageState>((set, get) => ({
  inputFile: null,
  sourceWidth: 0,
  sourceHeight: 0,
  striped: false,
  previewRgbaData: null,
  previewWidth: 0,
  previewHeight: 0,

  params: {
    ...DEFAULT_PARAMS,
    target_width: initDims.width,
    target_height: initDims.height,
  },
  preserveAspectRatio: true,
  lockDimensions: false,
  paramsVersion: 0,
  error: null,

  setInput: async (file) => {
    let decoded: DecodedInput | undefined;
    try {
      decoded = await processingClient.decode(file);
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
      return;
    }
    const { width, height } = decoded;

    const state = get();
    const params = { ...state.params };
    const isPortrait = height > width;

    if (!state.lockDimensions) {
      // Load saved dimensions for this orientation
      const saved = loadDimsForOrientation(isPortrait);
      params.target_width = saved.width;
      params.target_height = saved.height;

      if (state.preserveAspectRatio) {
        const aspect = width / height;
        if (isPortrait) {
          // Portrait: height is the long edge — derive width from it
          params.target_width = Math.round(params.target_height * aspect);
        } else {
          // Landscape/square: width is the long edge — derive height from it
          params.target_height = Math.round(params.target_width / aspect);
        }
      }

      saveDims(isPortrait, params.target_width, params.target_height);
    }

    set({
      inputFile: file,
      sourceWidth: width,
      sourceHeight: height,
      striped: decoded.striped,
      previewRgbaData: decoded.preview.rgbaData,
      previewWidth: decoded.preview.width,
      previewHeight: decoded.preview.height,
      params,
      paramsVersion: state.paramsVersion + 1,
      error: null,
    });

    // Eagerly pre-compute the base while the user reviews params
    // (monolithic only — the facade no-ops for striped inputs).
    processingClient.prewarmBase(params);
  },

  updateParams: (partial) => {
    const state = get();
    const newParams = { ...state.params, ...partial };

    if (state.preserveAspectRatio && !state.lockDimensions && state.sourceWidth > 0) {
      const aspect = state.sourceWidth / state.sourceHeight;
      if ("target_width" in partial && !("target_height" in partial)) {
        newParams.target_height = Math.round(newParams.target_width / aspect);
      } else if ("target_height" in partial && !("target_width" in partial)) {
        newParams.target_width = Math.round(newParams.target_height * aspect);
      }
    }

    if ("target_width" in partial || "target_height" in partial) {
      const isPortrait = state.sourceHeight > state.sourceWidth;
      saveDims(isPortrait, newParams.target_width, newParams.target_height);
    }

    set({ params: newParams, paramsVersion: state.paramsVersion + 1 });
  },

  setPreserveAspectRatio: (v) => {
    set({ preserveAspectRatio: v });
    if (v) {
      const state = get();
      if (state.sourceWidth > 0 && !state.lockDimensions) {
        const aspect = state.sourceWidth / state.sourceHeight;
        const isPortrait = state.sourceHeight > state.sourceWidth;
        const newParams = { ...state.params };
        if (isPortrait) {
          newParams.target_width = Math.round(newParams.target_height * aspect);
        } else {
          newParams.target_height = Math.round(newParams.target_width / aspect);
        }
        saveDims(isPortrait, newParams.target_width, newParams.target_height);
        set({ params: newParams, paramsVersion: state.paramsVersion + 1 });
      }
    }
  },

  setLockDimensions: (v) => {
    set({ lockDimensions: v });
  },

  clearError: () => set({ error: null }),

  resetImage: () => {
    const dims = loadDimsForOrientation(false);
    set({
      inputFile: null,
      sourceWidth: 0,
      sourceHeight: 0,
      striped: false,
      previewRgbaData: null,
      previewWidth: 0,
      previewHeight: 0,
      params: { ...DEFAULT_PARAMS, target_width: dims.width, target_height: dims.height },
      preserveAspectRatio: true,
      lockDimensions: false,
      paramsVersion: 0,
      error: null,
    });
  },
}));
