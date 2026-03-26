# Future Work

---

## Algorithm improvements

### Exact sharpening operator
Replace `sharpen::unsharp_mask` once the paper-exact kernel formula is identified.
The module boundary is clean: change only `sharpen.rs`.

### Exact downscale kernel
Replace `imageops::resize` in `resize.rs` with the confirmed resampling strategy.
No other module changes required.

### Exact artifact metric
Replace `metrics::artifact_ratio` if the paper evaluates P in a different colour
space (e.g. perceptual, or per-pixel rather than per-channel).

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

---

## Performance optimisations

### SIMD for inner loops
`metrics::artifact_ratio`, `sharpen::gaussian_blur`, and
`sharpen::gaussian_blur_single_channel` are the hot paths.
Both operate on flat `&[f32]` slices -- well-suited for auto-vectorisation or
explicit SIMD via `std::simd` (once stabilised) or `wide`.

### Parallel probing
Each probe (`sharpen -> measure`) is independent.  Add `rayon::par_iter` to the
probe loop in `pipeline.rs` for multi-core speedup.

### Separable blur cache
The Gaussian kernel is already computed once per probe call, but the luminance
extraction is also cached across probes in lightness mode.  Further gains are
possible by parallelising the probe loop.

---

## Tauri GUI integration

`imgsharp-core` is deliberately free of I/O, async, and GUI concerns.  The
integration path is:

1. Add a `crates/imgsharp-tauri/` crate with `tauri = "2"` as a dependency.
2. Expose `process_auto_sharp_downscale` as a Tauri command using `#[tauri::command]`.
3. Use `imgsharp-io` for file I/O inside the Tauri command handler.
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

`imgsharp-core` has no platform-specific dependencies.  It can be compiled to
WASM with `wasm-pack` to run the algorithm in the browser:

```sh
wasm-pack build crates/imgsharp-core --target web
```

The only dependency that may need attention is the `image` crate's `Lanczos3`
resize (used in `resize.rs`), which is pure Rust and should compile fine.

---

## Documentation

- Add `#[doc = ...]` examples to the public API in `lib.rs`.
- Publish `imgsharp-core` on crates.io once the API stabilises.
