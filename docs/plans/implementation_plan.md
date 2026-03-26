# Plan: r3sizer — Rust Workspace for Auto-Sharpness Image Downscaling

## Context

Implement a Rust library (and CLI) for image downscaling with automatic post-resize sharpness
adjustment. The algorithm is reconstructed from three academic papers (2016–2018). Core idea:
select sharpening strength by constraining artifact ratio P(s) — the fraction of pixel channel
values that fall outside [0,1] after sharpening — to a target threshold P0 (default 0.1%).
A cubic polynomial P_hat(s) is fitted to probe measurements, then solved for s*.

This is a greenfield repo — only `docs/auto_sharpness_algorithm_reconstruction.md` exists.

---

## Workspace Structure

```
r3sizer/
  Cargo.toml                      (workspace)
  README.md
  docs/
    algorithm.md
    assumptions.md
    future_work.md
  crates/
    imgsharp-core/
      Cargo.toml
      src/
        lib.rs         re-exports, CoreError
        types.rs       LinearRgbImage, ImageSize, AutoSharpParams, all shared types
        color.rs       sRGB ↔ linear conversion (IEC 61966-2-1)
        resize.rs      downscale via image crate Lanczos3
        contrast.rs    contrast leveling (placeholder stub)
        sharpen.rs     parameterized unsharp mask
        metrics.rs     artifact ratio P(s)
        fit.rs         Vandermonde least-squares cubic fit (f64)
        solve.rs       Cardano root finding + fallback selection
        pipeline.rs    process_auto_sharp_downscale orchestrator
      tests/
        integration.rs
      benches/
        pipeline_bench.rs
    imgsharp-io/
      Cargo.toml
      src/
        lib.rs         IoError
        load.rs        PNG/JPEG → LinearRgbImage
        save.rs        LinearRgbImage → PNG/JPEG
        convert.rs     buffer layout helpers
    imgsharp-cli/
      Cargo.toml
      src/
        main.rs
        args.rs        clap Cli struct
        run.rs         load → process → save → diagnostics
        output.rs      formatted stdout
```

---

## Public API (`imgsharp-core`)

```rust
pub fn process_auto_sharp_downscale(
    input: &LinearRgbImage,
    params: &AutoSharpParams,
) -> Result<ProcessOutput, CoreError>

pub struct ProcessOutput {
    pub image:       LinearRgbImage,
    pub diagnostics: AutoSharpDiagnostics,
}

pub struct AutoSharpDiagnostics {
    pub selected_strength:       f32,
    pub target_artifact_ratio:   f32,
    pub measured_artifact_ratio: f32,
    pub probe_samples:           Vec<ProbeSample>,
    pub fit_coefficients:        Option<CubicPolynomial>,
    pub fallback_used:           bool,
    pub fallback_reason:         Option<String>,
    pub input_size:              ImageSize,
    pub output_size:             ImageSize,
}

pub struct AutoSharpParams {
    pub target_width:              u32,
    pub target_height:             u32,
    pub probe_strengths:           ProbeConfig,    // Range{min,max,count} or Explicit(Vec)
    pub target_artifact_ratio:     f32,            // default 0.001
    pub enable_contrast_leveling:  bool,
    pub sharpen_sigma:             f32,            // unsharp mask sigma, default 1.0
    pub fit_strategy:              FitStrategy,    // Cubic | DirectSearch | ForcedLinear
    pub output_clamp:              ClampPolicy,    // Clamp | Normalize
}

// Key types
pub struct LinearRgbImage { width: u32, height: u32, data: Vec<f32> }
pub struct CubicPolynomial { pub a: f64, pub b: f64, pub c: f64, pub d: f64 }
pub struct ProbeSample     { pub strength: f32, pub artifact_ratio: f32 }

#[derive(thiserror::Error)]
pub enum CoreError {
    InvalidParams(String),
    FitFailed(String),
    NoValidRoot { reason: String },
    DimensionMismatch { .. },
    EmptyImage,
}
```

---

## Module Implementation Details

### `color.rs`
IEC 61966-2-1 piecewise formula.
- `srgb_to_linear(v)`: v/12.92 if v≤0.04045 else ((v+0.055)/1.055)^2.4
- `linear_to_srgb(v)`: v*12.92 if v≤0.0031308 else 1.055*v^(1/2.4)−0.055
- `image_srgb_to_linear(img)` / `image_linear_to_srgb(img)`: in-place on `pixels_mut()`

### `resize.rs`
Wrap `LinearRgbImage` buffer → `image::ImageBuffer<Rgb<f32>>` (same layout) → `imageops::resize`
with `FilterType::Lanczos3` → wrap back. No clamping; purely a resampling operation.

### `sharpen.rs` (unsharp mask — documented as engineering approximation)
`output[i] = src[i] + amount * (src[i] - blur[i])`
- Hand-rolled separable Gaussian kernel: radius = ceil(3σ), weights normalized
- **No clamping** — values go outside [0,1]; this is intentional and required for metric to work

### `metrics.rs`
```rust
pub fn artifact_ratio(img: &LinearRgbImage) -> f32 {
    let out = img.pixels().iter().filter(|&&v| v < 0.0 || v > 1.0).count();
    out as f32 / img.total_components() as f32
}
```

### `fit.rs` (Vandermonde least-squares, all f64)
Build 4×4 normal equations: `AtA[i][j] = Σ s_k^(i+j)`, `Atb[i] = Σ s_k^i * P_k`
Solve via Gaussian elimination with partial pivoting.
Requires ≥4 samples; returns `CoreError::FitFailed` if pivot < 1e-14.

### `solve.rs` (Cardano + fallback)
Solve `a*s^3 + b*s^2 + c*s + (d-P0) = 0` via depressed cubic / trigonometric method.
Root selection: pick **largest** root in `[s_min, s_max]` (maximize sharpness within budget).

Fallback (recorded in diagnostics):
1. Among probes with `artifact_ratio ≤ P0` → pick largest strength
2. If none qualify → pick probe with smallest artifact_ratio (least-bad)

### `contrast.rs`
Real struct + function signature with `enabled: bool`. When false → zero-cost no-op.
When true → placeholder percentile normalization, clearly commented as non-paper-exact.

### `pipeline.rs`
1. Validate params
2. Downscale to target size
3. Optional contrast leveling (on downscaled image)
4. Probe loop: for each s_i → `sharpen(downscaled, s_i)` → `artifact_ratio()` → store ProbeSample
   - `downscaled` is never mutated; each probe is a fresh allocation
5. Fit cubic (or DirectSearch fallback)
6. Find s* via solve or fallback
7. Final `sharpen(downscaled, s*)` → measure actual artifact ratio
8. Apply clamp policy
9. Return ProcessOutput

---

## Dependencies

| Crate | Key deps |
|---|---|
| `imgsharp-core` | `thiserror`, `serde`, `image = "0.25"` |
| `imgsharp-io` | `image`, `thiserror`, `serde_json`, `imgsharp-core` |
| `imgsharp-cli` | `clap = {features=["derive"]}`, `anyhow`, `serde_json`, both core crates |
| dev/test | `approx = "0.5"`, `criterion = "0.5"` |

No `nalgebra` — cubic fit uses manual Vandermonde.
No `ndarray` — plain `Vec<f32>` is sufficient.

---

## Testing

### Unit tests (inline per module)
- `color`: round-trip, known values (sRGB 128/255 ≈ linear 0.216), piecewise boundary
- `metrics`: zero image → P=0, all-ones → P=0, known out-of-range → exact P
- `fit`: 9-point recovery of known cubic to 1e-9; <4 samples → FitFailed
- `solve`: single root at s=2.0; multiple roots → largest in range selected; no root → fallback
- `sharpen`: output dims = input dims; amount=0 → output ≈ input; large amount → values OOB
- `resize`: output dims match target; 1×1→1×1 trivial case

### Integration tests (`tests/integration.rs`)
- 16×16 gradient → 4×4: full pipeline, output dims correct, selected_strength in probe range
- 8×8 checkerboard → 4×4: no panic, fallback_used matches diagnostics
- All-black / all-white: P(s)=0 → s* at max probe strength
- 1×1 image: no panic
- Diagnostics consistency: selected_strength ∈ [probe_min, probe_max]

### Benchmark
`criterion`: 1920×1080 → 960×540 full pipeline + individual sharpen/metrics/fit sub-benchmarks

---

## CLI

```
imgsharp --input <FILE> --output <FILE> --width <N> --height <N>
         [--target-artifact-ratio 0.001]
         [--diagnostics <FILE>]          # writes JSON
         [--probe-strengths 0.5,1.0,2.0] # explicit override
         [--enable-contrast-leveling]
         [--sharpen-sigma 1.0]
```

Stdout:
```
Selected sharpness strength : 1.847
Measured artifact ratio     : 0.000998  (target: 0.001000)
Polynomial fit              : success
Fallback used               : no
Output size                 : 800x600
```

---

## Implementation Phases

1. **Workspace scaffolding**: all Cargo.toml files, stub `lib.rs`, types with derives — `cargo build` passes
2. **Color + metrics**: pure functions, no deps, with inline tests
3. **Resize + sharpen**: add `image` crate dep; implement with tests
4. **Fit + solve**: Vandermonde GE + Cardano; highest numerical risk — unit test thoroughly
5. **Contrast stub**: minimal real structure
6. **Pipeline**: integrate all modules; integration tests
7. **I/O layer**: load/save PNG/JPEG in `imgsharp-io`
8. **CLI**: wire up `imgsharp-cli`, smoke test on real image
9. **Docs + polish**: README, algorithm.md, assumptions.md, future_work.md, clippy clean

---

## Key Design Rationale

- **f32 image, f64 polynomial**: image bandwidth favors f32; fitting normal equations have s^6 terms (4^6=4096) that need f64 precision to avoid cancellation
- **Unsharp mask, not paper formula**: exact sharpening operator is unconfirmed; USM has continuous `amount`, produces OOB values needed by metric, is swappable
- **Per-channel artifact metric**: captures partial clips; more sensitive than per-pixel; natural reading of "fraction of color values"
- **Probe image never mutated**: clean separation of probe vs. final-apply phases; simpler testing
- **Fallback in diagnostics not error**: image processing callers (GUI) need a result even when fit degrades; fallback from real measurements is always a valid answer

---

## Assumptions and Unknowns

**Confirmed from docs**: linear pipeline, float intermediates, post-resize sharpening, out-of-gamut artifact metric, cubic polynomial fit, P0=0.1% example, maximize-sharpness root selection

**Engineering approximations (documented)**: Lanczos3 downscale, unsharp mask sharpening, per-channel P metric, probe range [0.5, 4.0] with 9 samples, sigma=1.0

**Unknown (future research)**: exact sharpening kernel, exact P definition (channels vs pixels), contrast leveling position and formula, exact probe count and range from paper

---

## Verification

```bash
# Build the workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Smoke test CLI on a real image
cargo run -p imgsharp-cli -- \
  --input sample.jpg --output out.png \
  --width 800 --height 600 \
  --diagnostics diag.json

# Inspect diagnostics
cat diag.json

# Benchmarks
cargo bench -p imgsharp-core
```

Expected: output file saved at target size, stdout shows selected_strength and fit status,
diag.json contains all probe_samples and fit_coefficients.
