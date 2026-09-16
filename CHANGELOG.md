# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 — breaking changes may occur in any release.

---

## [Unreleased]

## [0.10.0] - 2026-09-16

### Added

#### CI / toolchain
- New **Rust CI workflow** (`.github/workflows/ci.yml`) running on every push
  and PR: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test
  --workspace`, `cargo doc` with `RUSTDOCFLAGS="-D warnings"`, and a WASM build
  via `wasm-pack`.
- New **weekly security audit** (`.github/workflows/audit.yml`) using
  `rustsec/audit-check`, also triggered on any `Cargo.lock` change.
- CI regenerates the TypeScript bindings and fails if `generated.ts` is not
  committed, and runs `wasm-pack test --node crates/r3sizer-wasm` before the
  WASM build.

#### Embedded metadata preservation (`r3sizer-metadata`)
- New **`r3sizer-metadata`** crate (fifth workspace member, no pixel decoding,
  no filesystem access): `extract` inventories JPEG/PNG/WebP metadata and
  `merge` re-embeds it into already-encoded JPEG/PNG/WebP output.
- Preserves EXIF (including GPS, capture settings, dates, copyright) and XMP
  across JPEG, PNG and WebP; IPTC, JPEG comments, PNG text chunks and density
  on same-format exports; and ICC profiles only when they can be verified to
  still describe the output.
- Corrects output-dependent fields: EXIF/XMP pixel dimensions, orientation
  (normalized only when the pixels were actually rotated), and color-space
  declarations. Unverifiable color declarations are dropped rather than
  relabeled as sRGB.
- Deliberately not preserved: MakerNote, SubIFDs, embedded previews and
  thumbnails, extended XMP. Unsupported formats and categories are reported,
  never claimed as preserved.
- Every omission or correction is returned as a typed
  `MetadataIssue { category, reason, field }` in a `MetadataReport`; `field`
  names a tag or chunk, never a value.
- Bounded parsing with `MetadataLimits` defaults: 256 MiB source, 8 MiB per
  payload, 16 MiB total metadata, 4,096 records, 4,096 EXIF entries, 32 IFDs,
  XML depth 64. Exceeding a limit skips that metadata with an issue; it never
  rejects a decodable image.
- Merge failures return the original encoded bytes unchanged with
  `merge_failed` issues.

#### Metadata-aware I/O (`r3sizer-io`)
- New `LoadedImage`, `load_with_metadata(path, &DecodeLimits, &MetadataLimits)`
  and `save_with_metadata(&image, path, &loaded, &MetadataLimits)`, which read
  the source once and return a `MetadataReport` from the save.
- Re-exports `MetadataLimits`, `MetadataReport`, `MetadataIssue`,
  `MetadataCategory` and `MetadataIssueReason`.

#### CLI metadata warnings
- `process` and `sweep` (with `--out-dir`) preserve source metadata in outputs
  and print one stderr line per issue:
  `warning: <output path>: metadata <category>/<reason> (<field>)`.
- Warnings never appear in `--diagnostics` JSON, `--output-format json`
  stdout, or `summary.json`, and never change exit codes or sweep counts. A
  sweep without `--out-dir` emits no metadata warnings.

#### WASM and web export
- New stateless WASM export `preserve_metadata`, with `MetadataExportRequest` /
  `MetadataExportResponse` and seven related metadata types generated into
  `generated.ts` (binary payloads cross as `Uint8Array`).
- Web downloads now carry the source file's metadata. Each processed output is
  bound to the file that produced it, and stale exports after the image
  changes are discarded.
- Localized (EN/RU) accessible export warning when metadata is lost or
  unverified; duplicate export clicks are ignored while a download is being
  prepared.

#### CLI — subcommand migration
- CLI restructured from flag-multiplexed modes to **clap subcommands**:
  `process`, `sweep`, `diff`, `corpus`, and `presets list` / `presets show`.
- `process` gains `--output-format json` for machine-readable stdout output.
- `sweep` flags renamed: `--sweep-dir` → `--in-dir`, `--sweep-output-dir` →
  `--out-dir`, `--sweep-summary` → `--summary`.
- New integration test suite (`crates/r3sizer/tests/cli.rs`) using `assert_cmd`
  + `predicates` covering 10 scenarios.

#### Decode limits (`r3sizer-io`)
- New `DecodeLimits` struct with `max_pixels` (default 100 MP) and
  `max_dimension` (default 16 384 px) fields.
- New `load_as_linear_with_limits(path, &limits)` function that reads the image
  header before allocating the pixel buffer, returning `IoError::TooLarge` on
  oversized inputs.
- `load_as_linear` is now a thin wrapper around `load_as_linear_with_limits`
  with default limits — existing callers get protection automatically.
- New `IoError::TooLarge { width, height }` variant.
- CLI flags `--max-pixels` and `--max-dimension` wired into `process` and
  `sweep` subcommands.

#### API quality (`r3sizer-core`)
- New `r3sizer_core::prelude` module re-exporting the stable public surface:
  `LinearRgbImage`, `AutoSharpParams`, preset constructors, pipeline entrypoints
  (`process_auto_sharp_downscale`, `prepare_base`, `process_from_prepared`,
  `PreparedBase`), output types, and the `CoreError` type.
- `PreparedBase::compute_detail(&self, params)` — convenience wrapper around the
  free function `compute_probe_detail`.
- `PreparedBase::run_probes(&self, strengths, params)` — convenience wrapper
  around `run_probes_standalone`.
- Stability tier doc-comments added to experimental modules: `evaluator`,
  `base_quality`, `contrast`, `recommendations`.

#### Examples (`r3sizer-io/examples/`)
- `single_file.rs` — load → `process_auto_sharp_downscale` → save, ~40 lines.
- `two_phase.rs` — `prepare_base` once, run `process_from_prepared` with
  `Fast`, `Balanced`, and `Quality` `PipelineMode` settings.
- `custom_params.rs` — manual `AutoSharpParams` construction; contrasts
  `Uniform` + `SharpenMode::Rgb` against `ContentAdaptive` + `SharpenMode::Lightness`.

#### Repo hygiene
- `SECURITY.md` — threat model, decode-limit mitigations, vulnerability
  reporting address.
- `CONTRIBUTING.md` — dev setup, project structure, PR checklist, stability
  tiers, commit style.
- `CHANGELOG.md` — this file.

### Changed

- **Minimum supported Rust version raised to 1.88** (was 1.87): the `image` 0.25.10
  dependency requires it.
- **Crate renamed:** `r3sizer-cli` → `r3sizer` so that `cargo install r3sizer`
  works naturally.  The produced binary name (`r3sizer`) is unchanged.
- `README.md` CLI examples updated to use the new subcommand syntax
  (`r3sizer process -i … -o …`).
- **Metadata is preserved by default** in CLI outputs and web downloads;
  previously exports carried no source metadata.
- `save_from_linear` now encodes in memory before writing, so a failed encode
  no longer leaves a partial output file.
- Web ingestion requests EXIF orientation (`imageOrientation: "from-image"`)
  and sRGB canvas contexts explicitly; web exports report color as unverified
  when the browser cannot confirm an sRGB canvas.

---

[Unreleased]: https://github.com/alvytsk/r3sizer/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/alvytsk/r3sizer/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/alvytsk/r3sizer/compare/v0.8.0...v0.9.0
