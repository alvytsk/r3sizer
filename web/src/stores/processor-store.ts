import { create } from "zustand";
import type {
  AutoSharpParams,
  AutoSharpDiagnostics,
} from "@/types/wasm-types";
import { DEFAULT_PARAMS } from "@/types/wasm-types";
import { processImageParallel, prepareImage, prepareBaseImage, clearAllCaches, setProgressCallback } from "@/wasm";

export type ExportFormat = "jpeg" | "png" | "webp";

// ---------------------------------------------------------------------------
// localStorage persistence (dimensions + export prefs only)
// ---------------------------------------------------------------------------

const PREFS_KEY = "r3sizer-prefs";

type OrientationDims = { width: number; height: number };

type PersistedPrefs = {
  exportFormat: ExportFormat;
  exportQuality: number;
  landscape: OrientationDims;
  portrait: OrientationDims;
};

function loadPrefs(): Partial<PersistedPrefs> {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const parsed = JSON.parse(raw) as any;
    // Migrate old flat targetWidth/targetHeight → per-orientation
    if ("targetWidth" in parsed && !("landscape" in parsed)) {
      const w = parsed.targetWidth as number;
      const h = (parsed.targetHeight as number) ?? Math.round(w * 0.75);
      parsed.landscape = { width: w, height: h };
      parsed.portrait = { width: h, height: w };
      delete parsed.targetWidth;
      delete parsed.targetHeight;
      localStorage.setItem(PREFS_KEY, JSON.stringify(parsed));
    }
    return parsed as Partial<PersistedPrefs>;
  } catch {
    return {};
  }
}

function savePrefs(patch: Partial<PersistedPrefs>): void {
  try {
    const current = loadPrefs();
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...current, ...patch }));
  } catch {
    // localStorage unavailable (private mode, storage full, etc.)
  }
}

/** Save target dimensions for the current image orientation. */
function saveDims(isPortrait: boolean, width: number, height: number): void {
  savePrefs(
    isPortrait
      ? { portrait: { width, height } }
      : { landscape: { width, height } },
  );
}

/** Load saved dimensions for the given orientation, with sensible defaults. */
function loadDimsForOrientation(isPortrait: boolean): OrientationDims {
  const prefs = loadPrefs();
  const defaultLong = DEFAULT_PARAMS.target_width;   // 800
  const defaultShort = DEFAULT_PARAMS.target_height;  // 600
  if (isPortrait) {
    return prefs.portrait ?? { width: defaultShort, height: defaultLong };
  }
  return prefs.landscape ?? { width: defaultLong, height: defaultShort };
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

interface ProcessorState {
  // Input
  inputFile: File | null;
  inputRgbaData: Uint8Array | null;
  inputWidth: number;
  inputHeight: number;

  // Parameters
  params: AutoSharpParams;
  preserveAspectRatio: boolean;
  lockDimensions: boolean;

  // Export preferences (persisted)
  exportFormat: ExportFormat;
  exportQuality: number;

  // Processing
  isProcessing: boolean;
  processingStage: string | null;
  error: string | null;

  // Output
  outputRgbaData: Uint8Array | null;
  outputWidth: number;
  outputHeight: number;
  diagnostics: AutoSharpDiagnostics | null;
  lastProcessedParams: AutoSharpParams | null;

  // Change tracking (cheap dirty check without JSON.stringify)
  paramsVersion: number;
  lastProcessedVersion: number;

  // Actions
  setInput: (
    file: File,
    rgbaData: Uint8Array,
    width: number,
    height: number
  ) => void;
  updateParams: (partial: Partial<AutoSharpParams>) => void;
  setPreserveAspectRatio: (v: boolean) => void;
  setLockDimensions: (v: boolean) => void;
  setExportFormat: (format: ExportFormat) => void;
  setExportQuality: (quality: number) => void;
  process: () => Promise<void>;
  reset: () => void;
}

const savedPrefs = loadPrefs();
const initDims = loadDimsForOrientation(false); // landscape default for pre-load state

export const useProcessorStore = create<ProcessorState>((set, get) => ({
  inputFile: null,
  inputRgbaData: null,
  inputWidth: 0,
  inputHeight: 0,

  params: {
    ...DEFAULT_PARAMS,
    target_width: initDims.width,
    target_height: initDims.height,
  },
  preserveAspectRatio: true,
  lockDimensions: false,

  exportFormat: savedPrefs.exportFormat ?? "jpeg",
  exportQuality: savedPrefs.exportQuality ?? 90,

  isProcessing: false,
  processingStage: null,
  error: null,

  outputRgbaData: null,
  outputWidth: 0,
  outputHeight: 0,
  diagnostics: null,
  lastProcessedParams: null,
  paramsVersion: 0,
  lastProcessedVersion: 0,

  setInput: (file, rgbaData, width, height) => {
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

    // Invalidate all WASM caches before preparing the new image.
    // Without this, a same-dimension image would reuse stale cached pixels.
    clearAllCaches()
      .catch(() => {});

    // Pre-convert sRGB→linear in the background (fire-and-forget).
    prepareImage(rgbaData, width, height)
      .then(() => {
        // After linear conversion, eagerly pre-compute the base image
        // (resize + classify + baseline + evaluator) while user reviews params.
        const s = get();
        const paramsJson = JSON.stringify(s.params);
        return prepareBaseImage(rgbaData, width, height, paramsJson);
      })
      .catch(() => {});
  },

  updateParams: (partial) => {
    const state = get();
    const newParams = { ...state.params, ...partial };

    if (state.preserveAspectRatio && !state.lockDimensions && state.inputWidth > 0) {
      const aspect = state.inputWidth / state.inputHeight;
      if ("target_width" in partial && !("target_height" in partial)) {
        newParams.target_height = Math.round(newParams.target_width / aspect);
      } else if ("target_height" in partial && !("target_width" in partial)) {
        newParams.target_width = Math.round(newParams.target_height * aspect);
      }
    }

    if ("target_width" in partial || "target_height" in partial) {
      const isPortrait = state.inputHeight > state.inputWidth;
      saveDims(isPortrait, newParams.target_width, newParams.target_height);
    }

    set({ params: newParams, paramsVersion: state.paramsVersion + 1 });
  },

  setPreserveAspectRatio: (v) => {
    set({ preserveAspectRatio: v });
    if (v) {
      const state = get();
      if (state.inputWidth > 0 && !state.lockDimensions) {
        const aspect = state.inputWidth / state.inputHeight;
        const isPortrait = state.inputHeight > state.inputWidth;
        const newParams = { ...state.params };
        if (isPortrait) {
          newParams.target_width = Math.round(newParams.target_height * aspect);
        } else {
          newParams.target_height = Math.round(newParams.target_width / aspect);
        }
        saveDims(isPortrait, newParams.target_width, newParams.target_height);
        set({ params: newParams });
      }
    }
  },

  setLockDimensions: (v) => {
    set({ lockDimensions: v });
  },

  setExportFormat: (format) => {
    savePrefs({ exportFormat: format });
    set({ exportFormat: format });
  },

  setExportQuality: (quality) => {
    savePrefs({ exportQuality: quality });
    set({ exportQuality: quality });
  },

  process: async () => {
    const state = get();
    if (!state.inputRgbaData) {
      set({ error: "No image loaded" });
      return;
    }

    set({ isProcessing: true, processingStage: null, error: null });
    setProgressCallback((stage) => set({ processingStage: stage }));

    try {
      const paramsJson = JSON.stringify(state.params);
      // Try parallel probing first (uses probe worker pool).
      // Falls back to single-worker if pool unavailable or base not cached.
      const result = await processImageParallel(
        state.inputRgbaData,
        state.inputWidth,
        state.inputHeight,
        paramsJson
      );

      set({
        outputRgbaData: result.imageData,
        outputWidth: result.outputWidth,
        outputHeight: result.outputHeight,
        diagnostics: result.diagnostics,
        lastProcessedParams: { ...state.params },
        lastProcessedVersion: get().paramsVersion,
        isProcessing: false,
        processingStage: null,
      });
    } catch (e) {
      set({
        error: e instanceof Error ? e.message : String(e),
        isProcessing: false,
        processingStage: null,
      });
    } finally {
      setProgressCallback(null);
    }
  },

  // Full reset: clears image + processing state, restores saved landscape dims.
  // Export format/quality are intentionally kept — they are user preferences,
  // not tied to a specific image.
  reset: () => {
    const dims = loadDimsForOrientation(false);
    set({
      inputFile: null,
      inputRgbaData: null,
      inputWidth: 0,
      inputHeight: 0,
      params: { ...DEFAULT_PARAMS, target_width: dims.width, target_height: dims.height },
      preserveAspectRatio: true,
      lockDimensions: false,
      isProcessing: false,
      processingStage: null,
      error: null,
      outputRgbaData: null,
      outputWidth: 0,
      outputHeight: 0,
      diagnostics: null,
      lastProcessedParams: null,
      paramsVersion: 0,
      lastProcessedVersion: 0,
    });
  },
}));
