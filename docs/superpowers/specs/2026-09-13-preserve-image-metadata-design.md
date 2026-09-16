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

Add `crates/r3sizer-metadata` as a new workspace member with its own `Cargo.toml`.
It is a filesystem-independent Rust library, with no dependency on `r3sizer-core`,
`r3sizer-io`, or platform APIs. Both `r3sizer-io` and `r3sizer-wasm` depend directly
on it; WASM does not acquire a dependency on native I/O. Keep metadata outside
numerical processing buffers. Extend existing library APIs additively so pixel-only
callers continue to work.

The new crate inherits workspace package metadata, including version (currently
`0.9.0`), edition, Rust version, and license. Register its path/version dependencies
and lockfile changes through the existing workspace conventions. Existing workspace
test, lint, and documentation CI jobs include it automatically. Add a type-generation
freshness check to CI and ensure the existing WASM build covers its browser target.
Update the architecture and generation guidance in `CLAUDE.md`, `CONTRIBUTING.md`,
and the README to reflect the fifth crate.

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

## Shared types and TypeScript generation

Define metadata issue, category, reason, and serializable export-report types in
`crates/r3sizer-metadata/src/types.rs`, using serde and the established optional
`typegen` feature with `ts-rs` and `serde-compat`. Rust is the source of truth for
these cross-boundary types; do not handwrite equivalent TypeScript definitions.

Extend `crates/r3sizer-core/tests/typegen.rs` to emit the metadata declarations into
the existing `web/src/shared/lib/types/generated.ts`. Its access to the metadata
crate is a development dependency with `typegen` enabled, not a production core
dependency. The metadata crate's independence from core avoids a dependency cycle.
Keep the existing regeneration command working:

```sh
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
```

Commit regenerated declarations and consume them through the existing
`wasm-types.ts` re-export. Keep `ts-rs` out of production builds. CI regenerates
the file and checks for a diff; update the Docker build inputs to include the new
crate so its existing generation step continues to work.

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

Metadata issues are stderr-only in the CLI: do not add fields to `diag.json`, JSON
stdout, or sweep `summary.json`. The shared library and WASM still return structured
issues for callers; the CLI renders these as text. In a sweep, print issues for
each affected exported file, without a batch count or aggregated issue list in
either the summary JSON or final console summary. Metadata warnings do not mark
files as failed or enter the summary's processing-error collection. When a sweep
has no output directory and writes no images, emit no preservation warnings.

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

Assert structured issue categories and reasons in library tests. CLI integration
tests capture stderr and verify the affected output path and warning category,
successful exit status, unchanged JSON schemas, and unchanged sweep success/error
counts. Include a sweep without image outputs and assert no preservation warnings.
Verify generated TypeScript matches Rust and that the new crate builds as part of
both the native workspace and WASM target.
