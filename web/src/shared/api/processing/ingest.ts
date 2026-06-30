/** Stripe extraction from a decoded ImageBitmap.
 *
 * Pixels stay in browser-managed memory (the bitmap); we extract sequential
 * full-width row stripes through one reused OffscreenCanvas, in chunks no
 * wider than 4096px (safely below all browser canvas limits), and hand each
 * stripe to the WASM worker as a transferable buffer.
 */

/** ~16MB per stripe message. */
const STRIPE_TARGET_BYTES = 16 * 1024 * 1024;
const MIN_STRIPE_ROWS = 16;
const MAX_STRIPE_ROWS = 1024;
/** Chunks of <= 4096x1024 are safely below all browser canvas limits. */
const MAX_CHUNK_WIDTH = 4096;
/** Preview shown in the UI for striped images (~2MP max). */
const PREVIEW_MAX_PIXELS = 2_000_000;

export interface StripePlan {
  stripeHeight: number;
  count: number;
}

export function planStripes(srcWidth: number, srcHeight: number): StripePlan {
  const rowBytes = srcWidth * 4;
  const stripeHeight = Math.min(
    Math.max(Math.floor(STRIPE_TARGET_BYTES / rowBytes), MIN_STRIPE_ROWS),
    MAX_STRIPE_ROWS,
  );
  return { stripeHeight, count: Math.ceil(srcHeight / stripeHeight) };
}

export function decodeToBitmap(file: File): Promise<ImageBitmap> {
  // Best-effort match with the previous loader's color behavior; perfect
  // cross-browser consistency is not claimed. `colorSpace` is a newer DOM
  // option not yet in the bundled TS lib, hence the cast.
  return createImageBitmap(file, {
    premultiplyAlpha: "none",
    colorSpace: "srgb",
  } as ImageBitmapOptions);
}

/** Full-size RGBA extraction — monolithic path only (image <= threshold). */
export function bitmapToRgba(bitmap: ImageBitmap): {
  data: Uint8Array;
  width: number;
  height: number;
} {
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0);
  const imageData = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
  return {
    data: new Uint8Array(imageData.data.buffer),
    width: bitmap.width,
    height: bitmap.height,
  };
}

/** Downscaled preview for the UI (striped path — full RGBA never exists). */
export function makePreview(bitmap: ImageBitmap): {
  rgbaData: Uint8Array;
  width: number;
  height: number;
} {
  const scale = Math.min(1, Math.sqrt(PREVIEW_MAX_PIXELS / (bitmap.width * bitmap.height)));
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0, w, h);
  const imageData = ctx.getImageData(0, 0, w, h);
  return { rgbaData: new Uint8Array(imageData.data.buffer), width: w, height: h };
}

/**
 * Yield sequential full-width row stripes of the bitmap as RGBA8 buffers.
 * Each stripe is assembled from <= 4096px-wide chunks through one reused
 * OffscreenCanvas.
 */
export async function* extractStripes(
  bitmap: ImageBitmap,
): AsyncGenerator<{ rgba: Uint8Array; rows: number }> {
  const { stripeHeight } = planStripes(bitmap.width, bitmap.height);
  const chunkW = Math.min(bitmap.width, MAX_CHUNK_WIDTH);
  const canvas = new OffscreenCanvas(chunkW, stripeHeight);
  const ctx = canvas.getContext("2d", { willReadFrequently: true })!;

  for (let y = 0; y < bitmap.height; y += stripeHeight) {
    const rows = Math.min(stripeHeight, bitmap.height - y);
    const stripe = new Uint8Array(bitmap.width * rows * 4);
    for (let x = 0; x < bitmap.width; x += MAX_CHUNK_WIDTH) {
      const w = Math.min(MAX_CHUNK_WIDTH, bitmap.width - x);
      ctx.clearRect(0, 0, w, rows);
      ctx.drawImage(bitmap, x, y, w, rows, 0, 0, w, rows);
      const chunk = ctx.getImageData(0, 0, w, rows).data;
      for (let r = 0; r < rows; r++) {
        stripe.set(chunk.subarray(r * w * 4, (r + 1) * w * 4), (r * bitmap.width + x) * 4);
      }
    }
    yield { rgba: stripe, rows };
  }
}
