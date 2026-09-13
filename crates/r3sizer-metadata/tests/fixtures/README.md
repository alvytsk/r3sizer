# Fixture provenance

This crate uses no binary image files as test fixtures. Every JPEG, PNG, and
WebP byte sequence exercised by `tests/containers.rs` and by the
module-local `#[cfg(test)]` blocks in `src/containers/{jpeg,png,webp}.rs`
and `src/iptc.rs` is synthesized programmatically, byte-by-byte, from the
format specifications (ITU-T T.81 for JPEG, the PNG spec, the RIFF/WebP
container spec, and Adobe's Photoshop IRB / IPTC IIM references).

Rationale:

- Hand-authoring the exact byte sequences (rather than loading real photos)
  gives precise control over edge cases this task must cover: corrupted
  CRCs, truncated segments/chunks, out-of-order or duplicate ICC segments,
  decompression-bomb-shaped zTXt/iCCP payloads, odd-size WebP chunks, and
  so on. These are difficult or impossible to reliably reproduce by editing
  a real image file.
- It guarantees no personal GPS data, camera serial numbers, or copyrighted
  photograph content ever enters the repository — every pixel-adjacent
  field (entropy-coded scan bytes, IDAT, VP8 data) is a placeholder that is
  never decoded, only skipped by length.
- All EXIF/XMP/IPTC text content in fixtures (e.g. `"Fixture Author"`,
  `"MM\0*fake tiff"`) is invented placeholder text with no relation to any
  real person, place, or device.

No fixture in this crate was downloaded, copied, or derived from an
external image file.
