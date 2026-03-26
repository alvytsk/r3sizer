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
Selected sharpness strength : 1.8472
Measured artifact ratio     : 0.000997  (target: 0.001000)
Polynomial fit              : success
Fallback used               : no
Output size                 : 800×600
```

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
  ↓ probe N sharpening strengths:
      for each s_i:  sharpen(s_i)  →  measure P(s_i) = out-of-range fraction
  ↓ fit cubic  P_hat(s) = a·s³ + b·s² + c·s + d
  ↓ solve  P_hat(s*) = P0  (default P0 = 0.001 = 0.1%)
  ↓ apply final sharpening(s*)
  ↓ clamp to [0,1]
  ↓ linear RGB → sRGB  →  save
```

See [`docs/algorithm.md`](docs/algorithm.md) for a full pipeline description.

---

## CLI reference

| Flag | Default | Description |
|------|---------|-------------|
| `--input` | required | Input image path |
| `--output` | required | Output image path |
| `--width` | required | Target width (px) |
| `--height` | required | Target height (px) |
| `--target-artifact-ratio` | `0.001` | P0 threshold (fraction, not percent) |
| `--diagnostics` | — | JSON file for full diagnostics |
| `--probe-strengths` | 9 samples, 0.5–4.0 | Comma-separated explicit probe list |
| `--sharpen-sigma` | `1.0` | Gaussian sigma for unsharp mask |
| `--enable-contrast-leveling` | off | Enable contrast leveling stage |

---

## Running tests

```sh
cargo test --workspace
```

## Linting

```sh
cargo clippy --workspace -- -D warnings
```

---

## Documentation

- [`docs/algorithm.md`](docs/algorithm.md) — implemented pipeline
- [`docs/assumptions.md`](docs/assumptions.md) — confirmed vs engineering approximations
- [`docs/future_work.md`](docs/future_work.md) — next steps and Tauri integration
