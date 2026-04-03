export function sliderValue(v: number | readonly number[]): number {
  return Array.isArray(v) ? v[0] : (v as number);
}

export const SHARPEN_MODE: Record<string, string> = {
  lightness: "Lightness",
  rgb: "RGB",
};

export const METRIC_MODE: Record<string, string> = {
  relative_to_base: "Relative to Baseline",
  absolute_total: "Absolute Total",
};

export const ARTIFACT_METRIC: Record<string, string> = {
  channel_clipping_ratio: "Channel Clipping Ratio",
  pixel_out_of_gamut_ratio: "Pixel Out-of-Gamut Ratio",
};

export const SELECTION_POLICY: Record<string, string> = {
  gamut_only: "Gamut Only",
  hybrid: "Hybrid",
  composite_only: "Composite Only (exp)",
};

export const FIT_STRATEGY: Record<string, string> = {
  Cubic: "Cubic",
  DirectSearch: "Direct Search",
};

export const CLAMP_POLICY: Record<string, string> = {
  Clamp: "Clamp",
  Normalize: "Normalize",
};

export const SHARPEN_STRATEGY: Record<string, string> = {
  uniform: "Uniform",
  content_adaptive: "Content Adaptive",
};

export const RESIZE_KERNEL: Record<string, string> = {
  lanczos3: "Lanczos3",
  mitchell_netravali: "Mitchell-Netravali",
  catmull_rom: "Catmull-Rom",
  gaussian: "Gaussian",
  content_adaptive: "Content Adaptive",
};

interface DimensionPreset {
  label: string;
  detail: string;
  w: number;
  h: number;
}

export const DIMENSION_PRESETS: { group: string; items: DimensionPreset[] }[] = [
  { group: "Screens", items: [
    { label: "HD 720p", detail: "1280 × 720", w: 1280, h: 720 },
    { label: "HD 4:3", detail: "960 × 720", w: 960, h: 720 },
    { label: "Full HD", detail: "1920 × 1080", w: 1920, h: 1080 },
    { label: "FHD 4:3", detail: "1440 × 1080", w: 1440, h: 1080 },
    { label: "QHD 1440p", detail: "2560 × 1440", w: 2560, h: 1440 },
    { label: "QHD 4:3", detail: "1920 × 1440", w: 1920, h: 1440 },
    { label: "4K UHD", detail: "3840 × 2160", w: 3840, h: 2160 },
    { label: "4K 4:3", detail: "2880 × 2160", w: 2880, h: 2160 },
  ]},
  { group: "Web", items: [
    { label: "Small", detail: "800 × 600", w: 800, h: 600 },
    { label: "Small 4:3", detail: "640 × 480", w: 640, h: 480 },
    { label: "Medium", detail: "1200 × 800", w: 1200, h: 800 },
    { label: "Medium 4:3", detail: "1024 × 768", w: 1024, h: 768 },
    { label: "Large", detail: "1600 × 900", w: 1600, h: 900 },
    { label: "Large 4:3", detail: "1600 × 1200", w: 1600, h: 1200 },
  ]},
  { group: "Social", items: [
    { label: "Square", detail: "1080 × 1080", w: 1080, h: 1080 },
    { label: "OG Image", detail: "1200 × 630", w: 1200, h: 630 },
    { label: "Thumbnail", detail: "300 × 300", w: 300, h: 300 },
  ]},
];

export const ALL_PRESETS = DIMENSION_PRESETS.flatMap((g) => g.items);
