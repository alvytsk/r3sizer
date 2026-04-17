# r3sizer-core

[![Crates.io](https://img.shields.io/crates/v/r3sizer-core.svg)](https://crates.io/crates/r3sizer-core)
[![Docs.rs](https://docs.rs/r3sizer-core/badge.svg)](https://docs.rs/r3sizer-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/alvytsk/r3sizer/blob/main/LICENSE)

Pure image-processing core for [r3sizer](https://github.com/alvytsk/r3sizer) — automatic-sharpness image downscaling.
No I/O, no CLI, no WASM glue — just the pipeline, suitable for embedding in CLIs, desktop apps (Tauri/egui), `wasm-bindgen` wrappers, or batch workers.

## What it does

Given a linear-RGB image and a target size, the pipeline:

1. Downscales with SIMD Lanczos3 (with a staged bilinear pre-reduce for large shrink ratios).
2. Classifies pixels into five region types (flat, textured, strong edge, microtexture, halo-risk).
3. Probes several sharpening strengths, measures the out-of-gamut artifact ratio at each, fits a cubic `P(s)`.
4. Solves `P(s*) = P0` for the target artifact budget, applies the final sharpen with a content-adaptive gain map and chroma guard.

The result: **the sharpest output that stays within your artifact budget**, computed per-image.

## Install

```toml
[dependencies]
r3sizer-core = "0.8"
```

## Quick start (one-shot)

```rust
use r3sizer_core::prelude::*;

// src: LinearRgbImage from your I/O layer (see `r3sizer-io`).
let params = AutoSharpParams::photo(800, 600).resolved();
let result = process_auto_sharp_downscale(&src, &params)?;
// result.image: LinearRgbImage, result.diagnostics: AutoSharpDiagnostics
```

## Quick start (two-phase — for interactive/GUI use)

`prepare_base` performs the expensive resize + classify + baseline work once; `process_from_prepared` runs only probing + fit + sharpen, so you can re-process with different parameters without redoing the base.

```rust
use r3sizer_core::prelude::*;

let prepared = prepare_base(&src, &params, &|_| {})?;
let output   = process_from_prepared(&prepared, &params, &|_| {})?;
```

`PreparedBase` carries a `BaseParamsKey` fingerprint, so cache reuse is safe across parameter changes.

## Feature flags

| Feature   | Default | Description                                                              |
|-----------|---------|--------------------------------------------------------------------------|
| `parallel` | ✅      | Enables Rayon inside the pipeline and the `image` crate.                 |
| `formats`  | ✅      | Enables PNG/JPEG/GIF/BMP/TIFF/WebP decoders in the `image` crate.        |
| `typegen`  |         | Enables `ts-rs` derivations for exporting TypeScript bindings.           |

Disable both `parallel` and `formats` for a minimal WASM build:

```toml
r3sizer-core = { version = "0.8", default-features = false }
```

## Key design decisions

- **f32 for pixels, f64 for polynomial fitting.** The Vandermonde matrix has terms up to `s⁶`; f32 causes catastrophic cancellation.
- **No clamping during sharpening.** Out-of-range values are the artifact signal — clamping happens only at final output.
- **Detail signal precomputed once per probe phase.** `D = input − blur(input)` is independent of strength `s`, so the Gaussian blur runs once and each probe becomes a cheap multiply-add.
- **Fallback is not an error.** If the cubic solve finds no root in-range, the pipeline returns the best probe sample with a typed `SelectionMode`.

## Documentation

- [Algorithm overview](https://github.com/alvytsk/r3sizer/blob/main/docs/algorithm.md)
- [Pipeline implementation walkthrough](https://github.com/alvytsk/r3sizer/blob/main/docs/pipeline_implementation.md)
- [Confirmed vs. engineering approximations](https://github.com/alvytsk/r3sizer/blob/main/docs/assumptions.md)
- API docs on [docs.rs](https://docs.rs/r3sizer-core)

## License

MIT — see [LICENSE](https://github.com/alvytsk/r3sizer/blob/main/LICENSE).
