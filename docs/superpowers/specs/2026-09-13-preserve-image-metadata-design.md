# Preserve original image metadata in web and CLI exports

## Approved behavior

Preserve original embedded image metadata by default in web downloads and CLI
outputs. Retain everything the destination format and implementation can safely
represent, including capture information, camera settings, GPS, copyright,
descriptions, and supported EXIF, XMP, IPTC, and text data. No companion files.

Export succeeds when metadata is unsupported or malformed, provided the image
itself can be exported. Report metadata loss or uncertainty visibly in the web
app and on stderr in the CLI. Do not promise universal or byte-identical
preservation across formats. Filesystem timestamps and permissions are outside
the embedded-metadata scope.

## Existing integration points

- `web/src/features/export/ui/download-button.tsx` currently encodes output
  pixels with canvas and downloads the resulting blob. The original `File` is
  available from the image store.
- `web/src/shared/api/processing/ingest.ts` decodes pixels through browser
  facilities. Both monolithic and striped processing must retain access to the
  original metadata without adding a full-resolution pixel allocation.
- `crates/r3sizer-io/src/load.rs` and `save.rs` currently pass pixel data alone.
- CLI `run.rs` and `sweep.rs` export resized source images. Both should propagate
  source metadata; generated corpus images have no source metadata to preserve.

## Architecture and alternatives

Use a dedicated, filesystem-independent Rust metadata component shared by native
I/O and WASM bindings. Keep metadata outside `r3sizer-core` and outside numerical
processing buffers. This makes preservation policy and warning behavior reusable
across both clients. Extend existing library APIs additively so pixel-only callers
continue to work.

Separate browser and native implementations would integrate locally with less
initial binding work but duplicate preservation policy and tests. An external
command-line metadata tool would not satisfy browser-only operation. Prefer the
shared component; dependency and codec selection require verification during
implementation planning.

The component reads original encoded bytes into a metadata bundle, adjusts it
using explicit output facts, and merges supported metadata into encoded output
bytes. Return structured issues alongside the final bytes. Issues identify the
metadata category or field when known and distinguish unsupported data, malformed
data, intentional removal of stale data, and inability to verify preservation.
Unknown metadata must never be silently classified as successfully preserved.

## Data correctness

Update existing dimension fields to match the exported pixels, including supported
EXIF and XMP equivalents. Normalize orientation only when the corresponding pixel
transform has occurred; verify native and browser behavior independently.

Preserve a source color profile only when it accurately describes output pixels.
When decoding or processing changes the color representation, use matching output
color metadata and report any original profile that cannot be retained. Blindly
reattaching a source profile is not acceptable. A complete new color-management
engine is outside scope; unsupported cases must produce an explicit message.

Remove stale embedded thumbnails or previews and report their removal unless they
can be regenerated correctly. Preserve opaque metadata only where its structure
and placement remain valid; do not blindly copy offset-dependent records or
format-specific blocks across incompatible containers.

## Formats and failure handling

Provide preservation for the existing web export formats: JPEG, PNG, and WebP.
For other source or CLI destination formats, preserve supported metadata when an
adapter exists and otherwise explicitly report that preservation could not be
verified or performed. Existing image-format support must not be reduced.

Parse metadata with bounds and resource limits. Invalid lengths, offsets, and
oversized payloads become metadata issues rather than crashes. On metadata failure,
fall back to the valid encoded image and report the failure. Pixel encoding or
file-writing failures remain export errors, not successful exports with warnings.

## Client behavior

Web export retains the source file corresponding to the processed output, encodes
pixels, applies metadata, then downloads. Show a persistent, accessible export
message when data is lost or unverified, with readable category details. Clear
obsolete messages on a new image or export. Disable duplicate export requests
while an export is pending. Translate messages using the existing localization
system. A file with verified absence of metadata needs no warning.

CLI processing carries the source bundle to saving and prints concise issues to
stderr with the affected output path. Sweep outputs inherit the same source
metadata with their own output facts. Metadata warnings do not change the success
exit code or corrupt machine-readable stdout and diagnostics.

## Verification

Use small fixtures containing known EXIF, GPS, copyright, XMP, text, and profiles.
Verify retained values by independently reading exported files, not solely by
round-tripping through the new writer and reader. Cover same-format and
cross-format JPEG/PNG/WebP exports, corrected dimensions, rotated source images,
profiles, stale thumbnails, malformed metadata, unsupported formats, and images
without metadata. Confirm output pixels still decode correctly.

Cover CLI process and sweep warning behavior, web export warnings and source/output
association, and both browser ingestion paths. Run relevant Rust and web tests,
type checks and builds, plus the repository-required checks before a PR. No
implementation or test results are claimed by this design document.
