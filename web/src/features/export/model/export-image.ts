import type { ExportFormat } from "@/entities/export-preferences";
import { preserveEncodedMetadata } from "@/shared/api";
import type { MetadataReport } from "@/shared/lib";

/** Everything one export run needs, captured once from output state at click time. */
export interface ExportSnapshot {
  sourceFile: File;
  rgba: Uint8Array;
  width: number;
  height: number;
  format: ExportFormat;
  quality: number;
}

export interface ExportResult {
  blob: Blob;
  report: MetadataReport;
  filename: string;
}

const FORMAT_EXT: Record<ExportFormat, string> = {
  jpeg: "jpg",
  png: "png",
  webp: "webp",
};

const FORMAT_MIME: Record<ExportFormat, string> = {
  jpeg: "image/jpeg",
  png: "image/png",
  webp: "image/webp",
};

/**
 * Encodes a snapshot's pixels to a Blob in the requested format, then merges
 * in the source file's metadata. Never silently downgrades to a different
 * pixel format: a canvas failure, or a mismatch between the requested and
 * actually-encoded MIME type, rejects instead of returning the wrong bytes.
 */
export async function exportImage(snapshot: ExportSnapshot): Promise<ExportResult> {
  const canvas = document.createElement("canvas");
  canvas.width = snapshot.width;
  canvas.height = snapshot.height;
  const ctx = canvas.getContext("2d", { colorSpace: "srgb" });
  if (!ctx) throw new Error("Image encoding failed");

  const clamped = new Uint8ClampedArray(snapshot.rgba.length);
  clamped.set(snapshot.rgba);
  ctx.putImageData(new ImageData(clamped, snapshot.width, snapshot.height), 0, 0);

  const mime = FORMAT_MIME[snapshot.format];
  const quality = snapshot.format === "png" ? undefined : snapshot.quality / 100;
  const encoded = await new Promise<Blob>((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        if (blob) resolve(blob);
        else reject(new Error("Image encoding failed"));
      },
      mime,
      quality,
    );
  });
  if (encoded.type !== mime) throw new Error("Image encoding failed");

  const color = ctx.getContextAttributes?.().colorSpace === "srgb" ? "srgb" : "unverified";
  const { blob, report } = await preserveEncodedMetadata(snapshot.sourceFile, encoded, {
    width: snapshot.width,
    height: snapshot.height,
    orientation: "normalize",
    color,
  });

  const stem = snapshot.sourceFile.name.replace(/\.[^.]+$/, "");
  const filename = `${stem}-${snapshot.width}x${snapshot.height}.${FORMAT_EXT[snapshot.format]}`;

  return { blob, report, filename };
}
