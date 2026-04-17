# r3sizer

**Smart image downscaling with automatic sharpness correction**

Resize your images without losing sharpness. r3sizer mathematically determines the optimal sharpening strength for each image — no manual tweaking, no guesswork.

[![Deploy](https://github.com/alvytsk/r3sizer/actions/workflows/deploy.yml/badge.svg)](https://github.com/alvytsk/r3sizer/actions/workflows/deploy.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org)
[![WebAssembly](https://img.shields.io/badge/WASM-ready-blueviolet.svg)](https://alvytsk.github.io/r3sizer/)

**[Try the live demo](https://alvytsk.github.io/r3sizer/)** — runs entirely in the browser via WebAssembly.

<!-- TODO: Add screenshot of the web UI here -->
<!-- ![r3sizer web UI](docs/images/screenshot.png) -->

---

## Why r3sizer?

Every image resize loses sharpness. The standard fix — applying a fixed unsharp mask — either over-sharpens (halos, ringing) or under-sharpens (mushy output). Getting it right means tweaking per image, per resolution.

r3sizer solves this automatically. It probes multiple sharpening strengths, fits a mathematical model of artifact levels, and solves for the exact strength that hits your quality budget. The result: **the sharpest possible output without visible artifacts**.

| | Manual sharpening | r3sizer |
|---|---|---|
| Strength selection | Trial and error | Computed per-image via cubic model |
| Artifact control | Hope for the best | Mathematically bounded (P0 threshold) |
| Content awareness | Same filter everywhere | 5-class region map adapts per-pixel |
| Consistency | Varies by operator | Reproducible, deterministic |
| Batch processing | Tedious | One command for entire directories |

---

## Key Features

- **Automatic strength selection** — fits a cubic polynomial to artifact measurements and solves for the optimal sharpening strength. No manual tuning.
- **Content-adaptive sharpening** — classifies image regions (flat, textured, edges, microtexture, halo-risk) and modulates sharpening per-pixel via a gain map.
- **Perceptual lightness mode** — sharpens CIE Y luminance instead of raw RGB, preserving color fidelity. Chroma guard prevents oversaturation.
- **Robust quality control** — R² fit quality, leave-one-out stability, monotonicity checks, and typed fallback reasons. The pipeline always produces a result.
- **Fast** — SIMD-accelerated Lanczos3 resize, staged bilinear pre-reduce for large shrink ratios, detail precomputation eliminates redundant Gaussian blurs during probing.
- **Runs anywhere** — pure Rust core with no I/O dependencies. Ships as a CLI, runs in the browser via WASM, and embeds in any Rust application.

---

## Quick Start

### CLI

```sh
cargo install --path crates/r3sizer

r3sizer process -i photo.jpg -o out.png --width 800 --height 600
```

That's it. r3sizer will automatically select the optimal sharpening strength and produce the output.

Use `--preserve-aspect-ratio` (`-p`) when only one dimension is known:

```sh
r3sizer process -i photo.jpg -o out.png --width 800 -p
```

Add `--diagnostics diag.json` for a full JSON report of the pipeline decisions.

See the [full CLI reference](docs/cli.md) for all options, presets, and sweep mode.

### Web UI

The browser-based diagnostic UI lets you process images interactively with real-time parameter adjustment:

```sh
cd web
npm run build:wasm
npm run dev
```

Or visit the **[live demo](https://alvytsk.github.io/r3sizer/)** — no installation required.

### As a library

Add `r3sizer-core` and `r3sizer-io` to your `Cargo.toml`, then:

```rust
use r3sizer_core::prelude::*;
use r3sizer_io::{load_as_linear, save_from_linear};

let src = load_as_linear(Path::new("input.jpg"))?;
let params = AutoSharpParams::photo(800, 600).resolved();
let result = process_auto_sharp_downscale(&src, &params)?;
save_from_linear(&result.image, Path::new("output.png"))?;
```

For interactive use (e.g., GUI or WASM), the two-phase API avoids recomputing the expensive resize and classification steps:

```rust
use r3sizer_core::prelude::*;

// Prepare once at image-load time (resize + classify + baseline ~1.5 s).
let prepared = prepare_base(&src, &params, &|_| {})?;
// Process multiple times with different params (probing + fit ~0.5 s each).
let output = process_from_prepared(&prepared, &params, &|_| {})?;
```

See [`crates/r3sizer-io/examples/`](crates/r3sizer-io/examples/) for runnable examples:

| Example | Description |
|---------|-------------|
| `single_file.rs` | Basic load → process → save, ~40 lines |
| `two_phase.rs` | Prepare once, run with Fast / Balanced / Quality modes |
| `custom_params.rs` | Build `AutoSharpParams` by hand, compare Uniform vs ContentAdaptive |

```sh
cargo run --example single_file --manifest-path crates/r3sizer-io/Cargo.toml \
    -- photo.jpg out.png 800 600
```

---

## How It Works

```
Input image (sRGB)
  |
  |  1. Linearize + downscale (SIMD Lanczos3)
  |  2. Classify into 5 region types
  |  3. Measure baseline artifact level
  v
Probe multiple sharpening strengths
  |
  |  Two-pass adaptive: coarse scan -> find crossing -> dense refinement
  |  Each probe: sharpen -> chroma guard -> measure artifacts
  v
Fit cubic model P(s) to measurements
  |
  |  Robustness checks: R², monotonicity, LOO stability
  v
Solve P(s*) = P0 for optimal strength
  |
  |  Apply final sharpening at s* -> chroma guard -> clamp
  v
Output (sRGB)
```

The key insight: sharpening artifacts (out-of-gamut pixels) follow a smooth cubic curve as strength increases. By fitting this curve and solving for a target threshold, r3sizer finds the sweet spot between sharpness and artifact control — per image, automatically.

See [`docs/algorithm.md`](docs/algorithm.md) for the complete pipeline description.

---

## Architecture

```
crates/
  r3sizer-core/    Pure processing — no I/O, no dependencies on CLI or WASM
  r3sizer-io/      Image I/O (PNG/JPEG via the image crate)
  r3sizer/         Command-line interface (clap)
  r3sizer-wasm/    WebAssembly bindings (wasm-bindgen)
web/               React + Vite + Tailwind diagnostic UI
```

`r3sizer-core` is the heart of the project. It has zero I/O dependencies and can be embedded in a Tauri desktop app, compiled to WASM, or used as a plain Rust library.

The WASM build powers the web UI via a Web Worker architecture with a **parallel probe pool** (up to 6 workers) for real-time interactive processing.

---

## Building & Testing

```sh
cargo build --workspace          # Build all crates
cargo test --workspace           # Run all tests
cargo clippy --workspace -- -D warnings  # Lint (warnings are errors)
cargo bench -p r3sizer-core      # Benchmarks
```

### TypeScript type generation

Types for the web UI are auto-generated from Rust via [`ts-rs`](https://crates.io/crates/ts-rs):

```sh
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
```

This keeps Rust and TypeScript types in sync, including default values.

---

## Development

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, workflow, and PR expectations.

---

## Documentation

| Document | Description |
|----------|-------------|
| [`docs/algorithm.md`](docs/algorithm.md) | Full pipeline description with all stages |
| [`docs/pipeline_implementation.md`](docs/pipeline_implementation.md) | Detailed walkthrough with data flow and allocations |
| [`docs/cli.md`](docs/cli.md) | Complete CLI flag reference |
| [`docs/assumptions.md`](docs/assumptions.md) | Confirmed vs. engineering approximations |
| [`docs/future_work.md`](docs/future_work.md) | Roadmap and next steps |

---

## License

MIT — see [LICENSE](LICENSE).
