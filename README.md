# r3sizer

A Rust library and CLI for image downscaling with **automatic post-resize sharpness adjustment**.

The sharpening strength is selected automatically by fitting a cubic model of artifact
ratios and solving for a target out-of-gamut threshold — not by a generic sharpness
heuristic.

---

## Quick start

```sh
cargo build --release -p imgsharp-cli

./target/release/imgsharp \
  --input photo.jpg \
  --output out.png \
  --width 800 \
  --height 600 \
  --diagnostics diag.json
```

Output:

```
Output size                 : 800x600
Sharpen mode                : lightness (CIE Y)
Sharpen model               : practical USM
Metric mode                 : relative (sharpening-added artifacts)
Artifact metric             : channel clipping ratio
Baseline artifact ratio     : 0.000000
Selected strength           : 1.8472
Target metric value         : 0.001000
Measured metric value        : 0.000997
Measured artifact ratio     : 0.000997
Budget reachable            : yes
Fit status                  : success
Crossing status             : found
Selection mode              : polynomial root

Fit quality:
  R²                        : 0.999834
  Residual sum of squares   : 1.23e-09
  Max residual              : 2.45e-05
  Min pivot                 : 3.67e+01

Robustness:
  Monotonic                 : yes
  Quasi-monotonic           : yes
  R² ok                     : yes
  Well conditioned          : yes
  LOO stable                : yes
  Max LOO root change       : 0.0312

Timing (us):
  Resize                    : 12450
  Contrast                  : 0
  Baseline                  : 890
  Probing                   : 45230
  Fit                       : 15
  Robustness                : 98
  Final sharpen             : 6120
  Clamp                     : 340
  Total                     : 65143
```

Use `--preserve-aspect-ratio` (`-p`) when only one dimension is known:

```sh
imgsharp -i photo.jpg -o out.png --width 800 -p
```

### Sweep mode

Process a directory of images and produce an aggregate summary:

```sh
imgsharp \
  --sweep-dir ./photos \
  --sweep-output-dir ./out \
  --sweep-summary summary.json \
  --width 800 --height 600
```

The summary JSON includes per-file results (selected strength, selection mode, timing)
and aggregate statistics (mean/median strength, fit success rate, selection mode histogram).

---

## Workspace layout

```
crates/
  imgsharp-core/   pure processing (color, resize, sharpen, metrics, fit, solve, pipeline)
  imgsharp-io/     image I/O (PNG/JPEG load/save via the `image` crate)
  imgsharp-cli/    command-line interface
```

`imgsharp-core` has no I/O or CLI dependencies and can be embedded in a Tauri GUI or
compiled to WASM without modification.

---

## Algorithm summary

```
input (sRGB file)
  ↓ load + normalize
  ↓ sRGB → linear RGB  (IEC 61966-2-1)
  ↓ Lanczos3 downscale (linear space)
  ↓ optional contrast leveling
  ↓ measure baseline artifact ratio P(base)
  ↓ extract CIE Y luminance from base
  ↓ probe N sharpening strengths:
      for each s_i:
        sharpen luminance(s_i)  →  reconstruct RGB via k=L'/L  →  measure P(s_i)
  ↓ fit cubic  P_hat(s) = a·s³ + b·s² + c·s + d  (with fit quality: R², residuals)
  ↓ robustness checks  (monotonicity, LOO stability)
  ↓ solve  P_hat(s*) = P0  (default P0 = 0.001 = 0.1%)
  ↓ apply final sharpening(s*)
  ↓ clamp to [0,1]
  ↓ linear RGB → sRGB  →  save
```

See [`docs/algorithm.md`](docs/algorithm.md) for a full pipeline description.

---

## CLI reference

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | required | Input image path |
| `--output` | `-o` | required | Output image path |
| `--width` | `-W` | — | Target width (px) |
| `--height` | `-H` | — | Target height (px) |
| `--preserve-aspect-ratio` | `-p` | off | Compute the missing dimension from the input aspect ratio |
| `--target-artifact-ratio` | | `0.001` | P0 threshold (fraction, not percent) |
| `--diagnostics` | | — | Path to write a JSON diagnostics file |
| `--probe-strengths` | | `0.05,0.1,0.2,0.4,0.8,1.5,3.0` | Comma-separated explicit probe list |
| `--sharpen-sigma` | | `1.0` | Gaussian sigma for unsharp mask |
| `--sharpen-mode` | | `lightness` | `lightness` (CIE Y) or `rgb` |
| `--metric-mode` | | `relative` | `relative` (sharpening-added) or `absolute` (total) |
| `--sharpen-model` | | `practical-usm` | `practical-usm` or `paper-lightness-approx` |
| `--artifact-metric` | | `channel-clipping` | `channel-clipping` or `pixel-out-of-gamut` |
| `--enable-contrast-leveling` | | off | Enable contrast leveling stage (placeholder) |
| `--sweep-dir` | | — | Directory of images to process in batch mode |
| `--sweep-output-dir` | | — | Output directory for processed images (sweep mode) |
| `--sweep-summary` | | — | Path to write sweep summary JSON |

In single-file mode, `--input` and `--output` are required. Both `--width` and `--height`
are required unless `--preserve-aspect-ratio` is set, in which case only one is needed.

In sweep mode, `--sweep-dir` replaces `--input`/`--output`. The sweep flags
(`--sweep-output-dir`, `--sweep-summary`) require `--sweep-dir`.

---

## Building and testing

```sh
# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace

# Lint (warnings are errors)
cargo clippy --workspace -- -D warnings

# Benchmarks
cargo bench -p imgsharp-core
```

---

## Library usage

`imgsharp-core` exposes the pipeline as a single function:

```rust
use imgsharp_core::{AutoSharpParams, ProcessOutput, pipeline::process_auto_sharp_downscale};

let params = AutoSharpParams {
    target_width: 800,
    target_height: 600,
    ..Default::default()
};

let ProcessOutput { image, diagnostics } =
    process_auto_sharp_downscale(&input_linear_rgb, &params)?;
```

`AutoSharpDiagnostics` is `Serialize`-able for JSON export and contains the full probe
data, fit coefficients, selection mode, per-stage provenance tags, fit quality metrics
(R², residuals), solver robustness flags (monotonicity, LOO stability), typed fallback
reasons, per-stage timing, and composite metric breakdowns.

---

## Documentation

- [`docs/algorithm.md`](docs/algorithm.md) — implemented pipeline
- [`docs/pipeline_implementation.md`](docs/pipeline_implementation.md) — detailed walkthrough with data flow and allocations
- [`docs/assumptions.md`](docs/assumptions.md) — confirmed vs engineering approximations
- [`docs/future_work.md`](docs/future_work.md) — next steps and Tauri integration

---

## License

MIT — see [LICENSE](LICENSE).
