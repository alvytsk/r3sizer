# r3sizer-metadata

Embedded-metadata extraction, correction, and merging for JPEG, PNG, and WebP images. No pixel decoding — this crate only walks container framing (JPEG segments, PNG chunks, WebP RIFF records) to find, validate, and re-embed metadata payloads.

Used by [`r3sizer-io`](../r3sizer-io) (native load/save) and `r3sizer-wasm` (browser export) to carry a source image's metadata through resize/sharpen to the output. `r3sizer-core` depends on it only as a **dev-only** dependency, to include its types in the shared TypeScript type export — never in a production build.

## API

```rust
let bundle = r3sizer_metadata::extract(source_bytes, &limits);
let export = r3sizer_metadata::merge(encoded_output_bytes, &bundle, &output_facts, &limits);
// export.bytes: encoded_output_bytes with source metadata merged in
// export.report: MetadataIssue list for anything not preserved
```

## What's preserved

When present in the source and verifiable against the actual output, and for JPEG/PNG/WebP destinations: EXIF (orientation, GPS, camera settings, dates, copyright), XMP, IPTC, ICC color profiles, text comments, and pixel density. EXIF/XMP dimension, orientation, and color-space tags are corrected (bounded TIFF patching, not blind copy) to match the real output `OutputFacts` rather than left describing the pre-processed image.

## What's never preserved

- **MakerNote** payloads
- **Embedded previews/thumbnails**
- **Extended XMP**
- Any metadata in a container format this crate doesn't recognize (only JPEG/PNG/WebP are supported destinations for merging)
- An ICC profile that can't be verified against the destination encoder's own declared profile — it's dropped and reported, not silently mismatched

Every one of these surfaces as a typed `MetadataIssue { category, reason, field }` on the returned report. The report is not an exhaustive inventory of every possible loss case for every malformed input — it records what actually happened for the file at hand, not an anticipated list.

## Limits (`MetadataLimits`, all overridable, see `Default`)

| Limit | Default |
|---|---|
| Max source file size | 256 MiB |
| Max single payload size | 8 MiB |
| Max total metadata bytes | 16 MiB |
| Max metadata records | 4,096 |
| Max EXIF entries | 4,096 |
| Max EXIF IFDs visited | 32 |
| Max XMP XML nesting depth | 64 |

Exceeding any limit degrades to a reported `LimitExceeded` issue (or, for an oversized source, a conservative "unavailable" bundle) — never a panic, an unbounded allocation, or a hang on adversarial input.

## Scope

No companion/sidecar files — everything is embedded in the output image itself. No universal or byte-identical preservation guarantee across formats. Filesystem timestamps and permissions are outside this crate's scope entirely.
