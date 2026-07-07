import { create } from "zustand";
import { loadPrefs, savePrefs } from "@/shared/lib";

/** Output container format chosen by the user (persisted across sessions). */
export type ExportFormat = "jpeg" | "png" | "webp";

/**
 * Export preferences entity — persisted output format/quality.
 * Isolated from the image and processing slices so it survives a full reset
 * (it is a user preference, not tied to a specific image).
 */
interface ExportPrefsState {
  exportFormat: ExportFormat;
  exportQuality: number;
  setExportFormat: (format: ExportFormat) => void;
  setExportQuality: (quality: number) => void;
}

const saved = loadPrefs();

export const useExportPrefsStore = create<ExportPrefsState>((set) => ({
  exportFormat: (saved.exportFormat as ExportFormat) ?? "jpeg",
  exportQuality: saved.exportQuality ?? 90,
  setExportFormat: (format) => {
    savePrefs({ exportFormat: format });
    set({ exportFormat: format });
  },
  setExportQuality: (quality) => {
    savePrefs({ exportQuality: quality });
    set({ exportQuality: quality });
  },
}));
