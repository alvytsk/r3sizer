/**
 * localStorage-backed preferences store (dimensions + export prefs).
 *
 * Lives in `shared/lib` so it can be consumed by multiple entity/feature slices
 * (`entities/images` for target dimensions, `entities/export-preferences` for
 * format/quality) without introducing cross-slice coupling.
 */
import { DEFAULT_PARAMS } from "./types/wasm-types";

const PREFS_KEY = "r3sizer-prefs";

type OrientationDims = { width: number; height: number };

type PersistedPrefs = {
  exportFormat: string;
  exportQuality: number;
  landscape: OrientationDims;
  portrait: OrientationDims;
};

export function loadPrefs(): Partial<PersistedPrefs> {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    // biome-ignore lint/suspicious/noExplicitAny: parsed shape is validated below before use
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

export function savePrefs(patch: Partial<PersistedPrefs>): void {
  try {
    const current = loadPrefs();
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...current, ...patch }));
  } catch {
    // localStorage unavailable (private mode, storage full, etc.)
  }
}

/** Save target dimensions for the current image orientation. */
export function saveDims(isPortrait: boolean, width: number, height: number): void {
  savePrefs(isPortrait ? { portrait: { width, height } } : { landscape: { width, height } });
}

/** Load saved dimensions for the given orientation, with sensible defaults. */
export function loadDimsForOrientation(isPortrait: boolean): OrientationDims {
  const prefs = loadPrefs();
  const defaultLong = DEFAULT_PARAMS.target_width; // 800
  const defaultShort = DEFAULT_PARAMS.target_height; // 600
  if (isPortrait) {
    return prefs.portrait ?? { width: defaultShort, height: defaultLong };
  }
  return prefs.landscape ?? { width: defaultLong, height: defaultShort };
}
