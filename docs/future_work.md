# Future Work

---

## Recently completed (through v0.6)

The following items from the original roadmap are now implemented:

- **Fit quality reporting** — `FitQuality` struct with R², residual sum of squares, max residual, min pivot.
- **Solver robustness checks** — `RobustnessFlags` with monotonicity, quasi-monotonicity, R² threshold, condition number, LOO stability.
- **Typed fallback reasons** — `FallbackReason` enum with 6 variants, priority-ordered.
- **Per-stage timing** — `StageTiming` with microsecond wall-clock times for all pipeline stages.
- **Composite metrics (v0.2)** — all four `MetricComponent` variants active: GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening. Configurable weights via `MetricWeights`.
- **Selection policy (v0.2.1)** — `SelectionPolicy` enum: GamutOnly, Hybrid, CompositeOnly.
- **Content-adaptive sharpening (v0.3)** — region classification (5 classes), per-pixel gain maps, adaptive backoff loop.
- **Content-adaptive resize (v0.4)** — per-region kernel selection (Lanczos3, MitchellNetravali, CatmullRom, Gaussian).
- **Chroma guard (v0.5)** — soft chroma clamping with context-aware thresholds, on by default.
- **Quality evaluator (v0.5)** — heuristic feature extraction + advisory strength cap, on by default.
- **Recommendations engine** — diagnostic-driven parameter suggestions (7 rules).
- **Two-phase pipeline (v0.6)** — `prepare_base` / `process_from_prepared` split for interactive use. `PreparedBase` carries a `BaseParamsKey` fingerprint for safe cache reuse.
- **Parallel probing in WASM (v0.6)** — probe worker pool (up to 6 workers), TwoPass two-round parallel probing, base data caching in workers.
- **Two calibrated presets (v0.6)** — Photo (P0=0.003, range [0.003, 1.0]) and Precision (P0=0.001, range [0.003, 0.5]).
- **CLI sweep mode** — batch processing with aggregate statistics (mean/median strength, fit success rate, selection mode histogram).
- **Detail precomputation (v0.7)** — `D = input - blur(input)` computed once per probe phase; each probe applies `out = input + s * D` (trivial multiply-add). WASM probe workers receive precomputed detail via `compute_probe_detail` / `probe_batch_with_detail`, eliminating redundant Gaussian blur across workers.
- **Staged shrink (v0.7)** — for shrink ratios >= 3x, a bilinear pre-reduce to ~2x target precedes the final Lanczos3 pass, following the libvips `gap` principle.
- **Pipeline modes (v0.7)** — `PipelineMode` enum (Fast / Balanced / Quality) controls probe budget, adaptive complexity, chroma guard, and evaluator. Applied via `AutoSharpParams::resolved()`.
- **Early stopping (v0.7)** — coarse probing exits after 3+ probes when a P0 crossing bracket is found, saving remaining probes.
- **fast_image_resize (v0.7)** — Lanczos3 downscaling via `fast_image_resize` crate with SSE4.1/AVX2 SIMD on x86-64 (~3.5x faster than `image` crate).
- **Base quality fast path (v0.7)** — source-side Sobel/variance diagnostics skipped in non-Full diagnostics mode (~40x faster).

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

### CompositeOnly selection policy
`CompositeOnly` is currently treated as Hybrid. Future work:
1. Add composite-driven polynomial fitting with a separate `target_selection_score`.
2. Sweep-based comparison of GamutOnly vs Hybrid vs CompositeOnly on a diverse corpus.

### Robustness threshold tuning
Current thresholds (R² > 0.85, min_pivot > 1e-8, LOO change < 0.5) are engineering
choices.  Use sweep mode across diverse image corpora to validate and tune.

---

## Performance optimisations

### ~~SIMD for resize~~ (done)
Resize now uses `fast_image_resize` with SSE4.1/AVX2 on x86-64.

### ~~Parallel probing~~ (done)
Probes run in parallel via `rayon::par_iter` (native) or a Web Worker pool (WASM).

### ~~Detail precomputation~~ (done)
Blur computed once per probe phase, not per probe. Workers receive precomputed detail.

### ~~Staged shrink~~ (done)
Bilinear pre-reduce for large shrink ratios (>= 3x).

### SIMD for Gaussian blur
`sharpen::gaussian_blur` and `gaussian_blur_single_channel` are hand-rolled
separable passes on flat `&[f32]`.  After detail precomputation, the blur runs
only once per probe phase (not per probe), so the ROI is reduced — but for
large images it remains the single most expensive operation.  Candidates:
explicit SIMD via `std::simd` (once stabilised), or delegation to
`fast_image_resize`'s internal blur if exposed.

### Tile-based probing
Evaluate probes on representative tiles instead of the full output image.
Would reduce per-probe `reconstruct_rgb` and `compute_selection_metric` cost
from O(W*H) to O(tile_count * tile_size^2).  Risk: tile metrics may diverge
from full-image behavior on unusual images.

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

## WASM / browser support — COMPLETED

The web UI is live at https://alvytsk.github.io/r3sizer/ with full pipeline support,
parallel probing via Web Worker pool, and two-phase caching for interactive use.

---

## Documentation

- Add `#[doc = ...]` examples to the public API in `lib.rs`.
- Publish `r3sizer-core` on crates.io once the API stabilises.
