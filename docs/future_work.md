# Future Work

---

## Recently completed (v0.1)

The following items from the original roadmap are now implemented:

- **Fit quality reporting** — `FitQuality` struct with R², residual sum of squares, max residual, min pivot. Computed in `fit::fit_cubic_with_quality`.
- **Solver robustness checks** — `RobustnessFlags` with monotonicity, quasi-monotonicity, R² threshold, condition number, LOO stability. Computed in `pipeline.rs`.
- **Typed fallback reasons** — `FallbackReason` enum with 6 variants, priority-ordered. Replaces implicit fallback logic.
- **Per-stage timing** — `StageTiming` with microsecond wall-clock times for all 8 pipeline stages.
- **Composite metric scaffold** — `MetricBreakdown` with `MetricComponent` variants. Only `GamutExcursion` is active in v0.1; others are stubs for v0.2.
- **CLI sweep mode** — `--sweep-dir`, `--sweep-output-dir`, `--sweep-summary` flags. Batch processing with aggregate statistics (mean/median strength, fit success rate, selection mode histogram).

---

## Algorithm improvements

### Exact sharpening operator
Replace `sharpen::unsharp_mask` once the paper-exact kernel formula is identified.
The module boundary is clean: change only `sharpen.rs`.

### Exact downscale kernel
Replace `imageops::resize` in `resize.rs` with the confirmed resampling strategy.
No other module changes required.

### Exact artifact metric
Two metrics are now implemented (`ChannelClippingRatio` and `PixelOutOfGamutRatio`).
Replace or extend if the paper evaluates P in a different colour space or uses a
different counting rule entirely.

### Exact contrast leveling
Replace the placeholder body of `contrast::apply_contrast_leveling`.
The function signature and placement in the pipeline are already correct.

### Confirm lightness reconstruction
The current multiplicative reconstruction `k = L'/L` is a strong inference.
If the paper uses a different colour-preserving method, replace
`color::reconstruct_rgb_from_lightness`.

### Per-channel cubic fit
If the paper fits three separate cubics (one per R, G, B channel), modify
`pipeline.rs` to call `fit_cubic` three times and aggregate the selected
strengths (e.g. take the median or minimum).

### Probe count and range tuning
Once paper values are known, update the `AutoSharpParams::default()` constants
in `types.rs`.  Current defaults are non-uniform, denser near zero.

### Adaptive probe strategy
Instead of a fixed probe list, consider a two-pass approach: coarse scan to
find the approximate crossing region, then dense probing near the crossing.
This would reduce the number of expensive sharpen+measure operations.

### Composite metric components (v0.2) — COMPLETED

All four `MetricComponent` variants are now active and fully implemented:
1. `GamutExcursion` — fraction of channel values outside [0, 1]
2. `HaloRinging` — sign-alternating oscillations near strong edges
3. `EdgeOvershoot` — sharpening exceeding local edge-strength proxy
4. `TextureFlattening` — changes in fine-scale local variance

Configurable weights are supported via `MetricWeights` (default: 1.0, 0.3, 0.3, 0.1).

### Selection policy (v0.2.1)

`SelectionPolicy` controls how fallback candidates are ranked:
- `GamutOnly` (default): gamut excursion drives both fitting and fallback ranking.
- `Hybrid`: gamut excursion is the hard safety constraint; composite score ranks
  fallback candidates. Polynomial fitting still uses gamut excursion.
- `CompositeOnly` (experimental): currently treated as Hybrid. Future work will
  add composite-driven polynomial fitting with a separate `target_selection_score`.

Next steps:
1. Sweep-based comparison of GamutOnly vs Hybrid on a diverse corpus.
2. Add `target_selection_score` parameter for CompositeOnly mode.
3. Investigate composite-driven polynomial fitting (requires monotonicity analysis).

### Robustness threshold tuning
Current thresholds (R² > 0.85, min_pivot > 1e-8, LOO change < 0.5) are engineering
choices.  Use sweep mode across diverse image corpora to validate and tune.

---

## Performance optimisations

### SIMD for inner loops
`metrics::artifact_ratio`, `sharpen::gaussian_blur`, and
`sharpen::gaussian_blur_single_channel` are the hot paths.
Both operate on flat `&[f32]` slices -- well-suited for auto-vectorisation or
explicit SIMD via `std::simd` (once stabilised) or `wide`.

### ~~Parallel probing~~ (done)
Probes now run in parallel via `rayon::par_iter` when the `parallel` feature is
enabled (default).  The Gaussian kernel and luminance buffer are shared read-only
across threads.

### Further parallelism
The Gaussian blur inner loops (`sharpen.rs`) could benefit from per-row
parallelism or explicit SIMD.  The current bottleneck for large images is the
separable blur, not the probe dispatch.

---

## Tauri GUI integration

`r3sizer-core` is deliberately free of I/O, async, and GUI concerns.  The
integration path is:

1. Add a `crates/r3sizer-tauri/` crate with `tauri = "2"` as a dependency.
2. Expose `process_auto_sharp_downscale` as a Tauri command using `#[tauri::command]`.
3. Use `r3sizer-io` for file I/O inside the Tauri command handler.
4. Stream `AutoSharpDiagnostics` (it is `serde::Serialize`) back to the frontend
   as a JSON event for live display of the probe curve.

Suggested GUI features:
- Input/output image preview side-by-side.
- Live P(s) scatter plot with fitted cubic overlay.
- Selected strength highlighted on the curve.
- Sharpen mode toggle (RGB / Lightness).
- Metric mode toggle (Absolute / Relative).
- Baseline artifact ratio display.
- Budget reachability indicator.
- Probe strength slider for manual override.
- Full diagnostics panel with fit/crossing/selection status.

---

## WASM / browser support

`r3sizer-core` has no platform-specific dependencies.  It can be compiled to
WASM with `wasm-pack` to run the algorithm in the browser:

```sh
wasm-pack build crates/r3sizer-core --target web
```

The only dependency that may need attention is the `image` crate's `Lanczos3`
resize (used in `resize.rs`), which is pure Rust and should compile fine.

---

## Documentation

- Add `#[doc = ...]` examples to the public API in `lib.rs`.
- Publish `r3sizer-core` on crates.io once the API stabilises.
