/**
 * What `get_probe_bundle` minted for one base: its pixels and luminance, the
 * detail signal, and the provenance stamp every batch made from them carries.
 *
 * `get_probe_bundle` builds this object field by field in Rust rather than
 * serializing a struct, so, like `ProcessResult`, it has no generated type.
 */
export interface ProbeBundle {
  basePixels: Float32Array;
  luminance: Float32Array;
  width: number;
  height: number;
  baseline: number;
  effectiveP0: number;
  detail: Float32Array;
  provenanceJson: string;
}
