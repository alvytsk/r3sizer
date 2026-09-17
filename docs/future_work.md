# Future Work

---

## Recently completed (through v0.10)

The following items from the original roadmap are now implemented. Versions
refer to the tagged workspace release that shipped them.

### Diagnostics and robustness (v0.1 to v0.2)

- **Fit quality reporting**: `FitQuality` struct with R2, residual sum of squares, max residual, min pivot.
- **Solver robustness checks**: `RobustnessFlags` with monotonicity, quasi-monotonicity, R2 threshold, condition number, LOO stability.
- **Typed fallback reasons**: `FallbackReason` enum with 6 variants, priority-ordered.
- **Per-stage timing**: `StageTiming` with microsecond wall-clock times for all pipeline stages.
- **Recommendations engine**: diagnostic-driven parameter suggestions (7 rules).
- **Composite metrics (v0.2)**: all four `MetricComponent` variants active: GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening. Configurable weights via `MetricWeights`.
- **Selection policy (v0.2.1)**: `SelectionPolicy` enum: GamutOnly, Hybrid, CompositeOnly.

### Adaptive processing (v0.3 to v0.5)

- **Content-adaptive sharpening (v0.3)**: region classification (5 classes), per-pixel gain maps, adaptive backoff loop.
- **Content-adaptive resize (v0.4)**: per-region kernel selection (Lanczos3, MitchellNetravali, CatmullRom, Gaussian).
- **Chroma guard (v0.5)**: soft chroma clamping with context-aware thresholds, on by default.
- **Quality evaluator (v0.5)**: heuristic feature extraction plus advisory strength cap, on by default.

### Interactive pipeline (v0.6)

- **Two-phase pipeline**: `prepare_base` / `process_from_prepared` split for interactive use. `PreparedBase` carries a `BaseParamsKey` fingerprint for safe cache reuse.
- **Parallel probing in WASM**: probe worker pool (up to 6 workers), TwoPass two-round parallel probing, base data caching in workers.
- **Two calibrated presets**: Photo (P0=0.003, range [0.003, 1.0]) and Precision (P0=0.001, range [0.003, 0.5]).
- **CLI sweep mode**: batch processing with aggregate statistics (mean/median strength, fit success rate, selection mode histogram).

### Speed (v0.7)

- **Detail precomputation**: `D = input - blur(input)` computed once per probe phase. Each probe applies `out = input + s * D` (trivial multiply-add). WASM probe workers receive precomputed detail via `compute_probe_detail` / `probe_batch_with_detail`, eliminating redundant Gaussian blur across workers.
- **Staged shrink**: for shrink ratios >= 3x, a bilinear pre-reduce to ~2x target precedes the final Lanczos3 pass, following the libvips `gap` principle.
- **Pipeline modes**: `PipelineMode` enum (Fast / Balanced / Quality) controls probe budget, adaptive complexity, chroma guard, and evaluator. Applied via `AutoSharpParams::resolved()`.
- **Early stopping**: coarse probing exits after 3 or more probes once a P0 crossing bracket is found, saving the remaining probes.
- **fast_image_resize**: Lanczos3 downscaling via the `fast_image_resize` crate with SSE4.1/AVX2 SIMD on x86-64 (~3.5x faster than the `image` crate).
- **Base quality fast path**: source-side Sobel/variance diagnostics skipped in non-Full diagnostics mode (~40x faster).

### Packaging and CLI structure (v0.8)

- **Subcommand CLI**: `process`, `sweep`, `diff`, `corpus`, and `presets` replace the flat flag set. Shared pipeline flags live in `PipelineArgs` and flatten into `process` and `sweep`.
- **Structured stdout**: `--output-format json` emits the diagnostics summary as JSON instead of text.
- **CI workflows**: `ci.yml` runs fmt, clippy with `-D warnings`, workspace tests, `cargo doc` with `-D warnings`, and a `wasm-pack` build. `audit.yml` runs a weekly `cargo audit`.
- **crates.io readiness**: the CLI crate was renamed from `r3sizer-cli` to `r3sizer`, and all crates carry publishable manifests.
- **WASM binary size**: reduced to roughly 460 KB.

### Large images and web architecture (v0.9)

- **Striped ingest**: `StripedPreReducer` in `ingest.rs` accepts sequential full-width sRGB RGBA8 stripes and accumulates an area-weighted pre-reduce directly into the ~2x intermediate. Peak memory is bounded to one stripe plus the intermediate instead of the full source, which makes 100 MP inputs tractable in the browser. `compute_intermediate_size` shares the staged-shrink math with `resize.rs`, so a striped ingest lands on the same intermediate dimensions as the monolithic path. `validate_striped_shrink` rejects shrink ratios below 3x, where the pre-reduce would be meaningless.
- **sRGB LUT moved into core**: the u8 to linear lookup table now lives in `color.rs` and is shared by the WASM and native paths.
- **Core review fixes**: `Normalize` clamp no longer brightens in-gamut images, `Explicit` probe values are validated as positive and distinct, aliased adaptive resize kernels are deduplicated, probing time is included in `total_us`, externally supplied probe samples are sorted defensively, the solver fallback ranking is NaN-safe via `total_cmp`, per-probe breakdowns are computed for composite policies, and the evaluator strength cap is recorded as an advisory selection stage.
- **Web architecture**: the UI migrated to Feature-Sliced Design v2.1 with public-API barrels, swapped ESLint for Biome plus Steiger, slimmed its dependencies, and lazy-loads heavy chunks.

### Metadata preservation and theming (v0.10)

- **`r3sizer-metadata` crate**: a fifth workspace member with no pixel decoding and no filesystem access. `extract` inventories JPEG/PNG/WebP metadata and `merge` re-embeds it into already-encoded output, correcting dimensions, orientation, and the color-space tag against `OutputFacts`. Every skipped, corrected, or dropped field surfaces as a typed `MetadataIssue`.
- **CLI and web preservation**: `r3sizer-io` gained `load_with_metadata` / `save_with_metadata`, the CLI reports issues on stderr, and the web export path runs `preserveEncodedMetadata` and shows a localized warning.
- **Bounded parsing**: `MetadataLimits` caps source size, payload size, total metadata, record count, EXIF entry count, IFD count, and XMP nesting depth. Anything past a limit degrades to a reported issue, never a panic or a hang.
- **Web fallback fix**: a large image with a modest downscale ratio now falls back to the monolithic path instead of attempting striped ingest.
- **Gruvbox theme**: theme tokens, Manrope for Cyrillic coverage, and simplified English and Russian welcome copy.

---

## Algorithm improvements

### Exact sharpening operator
Replace `sharpen::unsharp_mask` once the paper-exact kernel formula is identified.
The module boundary is clean: change only `sharpen.rs`.

### Exact downscale kernel
Replace the `fast_image_resize` Lanczos3 call in `resize.rs`, and the
`image::imageops::resize` calls in `resize_strategy.rs`, with the confirmed
resampling strategy. No other module changes required.

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
in `types.rs`. The current default is a `TwoPass` schedule: 7 coarse probes over
[0.003, 1.0], then 4 dense probes inside the bracketed crossing.

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
- Backfill `CHANGELOG.md` for the releases before 0.10.0. The history currently
  lives only in git tags.
- Add a README for `r3sizer-wasm`. The other four crates each have one.
- Publish `r3sizer-metadata` on crates.io. The first publish claims the name.
  See [`releasing.md`](releasing.md) for the procedure and the current
  published versions.
