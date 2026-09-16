import type { MetadataIssueReason, MetadataReport, OutputFacts } from "@/shared/lib";
import { exportMetadata } from "./wasm";

/** Largest source file whose metadata we read. Larger sources export without it. */
const MAX_SOURCE_BYTES = 256 * 1024 * 1024;

function fallback(encoded: Blob, reason: MetadataIssueReason) {
  return {
    blob: encoded,
    report: { issues: [{ category: "unknown" as const, reason, field: null }] },
  };
}

/**
 * Copy the source file's metadata into an already-encoded output image.
 * Never rejects: any metadata-side failure returns the original `encoded`
 * blob with a report explaining what was lost.
 */
export async function preserveEncodedMetadata(
  source: File,
  encoded: Blob,
  facts: OutputFacts,
): Promise<{ blob: Blob; report: MetadataReport }> {
  if (source.size > MAX_SOURCE_BYTES) return fallback(encoded, "limit_exceeded");

  let sourceBytes: Uint8Array;
  try {
    sourceBytes = new Uint8Array(await source.arrayBuffer());
  } catch {
    return fallback(encoded, "unverified");
  }

  try {
    // Fresh buffers: exportMetadata transfers (detaches) them.
    const encodedBytes = new Uint8Array(await encoded.arrayBuffer());
    const response = await exportMetadata({ source: sourceBytes, encoded: encodedBytes, facts });
    return {
      blob: new Blob([response.bytes as Uint8Array<ArrayBuffer>], { type: encoded.type }),
      report: response.report,
    };
  } catch {
    return fallback(encoded, "merge_failed");
  }
}
