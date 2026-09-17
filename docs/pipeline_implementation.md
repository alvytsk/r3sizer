# Pipeline Implementation Summary

Detailed walkthrough of the `process_auto_sharp_downscale` pipeline as implemented in `r3sizer-core`. This document traces one invocation from entry to return, covering data flow, allocations, numerical decisions, and fallback logic.

---

## Scope: what is outside this pipeline

`r3sizer-core` handles pixels only. Two concerns sit outside every stage below,
at the caller's boundary:

| Concern | Crate | Entry points |
|---|---|---|
| File decode and encode | `r3sizer-io` | `load_as_linear`, `save_from_linear` |
| Embedded metadata | `r3sizer-metadata`, via `r3sizer-io` | `load_with_metadata`, `save_with_metadata` |

Metadata extraction runs before Stage 1 and metadata merge runs after the final
encode. Neither reads or writes a pixel, and neither changes any value this
document describes. `r3sizer-core` does not link `r3sizer-metadata` in a
production build. The only edge between them is a dev-only dependency behind the
`typegen` feature, used to emit the metadata contract types into `generated.ts`.

See [`algorithm.md`](algorithm.md) Stage 0 and Stage 12 for what the metadata
path preserves, corrects, and reports.

---

## Entry Point

`crates/r3sizer-core/src/pipeline.rs` — `process_auto_sharp_downscale(input, params)`

The caller supplies a `LinearRgbImage` (already in linear RGB — the sRGB-to-linear conversion is the responsibility of `r3sizer-io` or the caller) and an `AutoSharpParams` struct.

Returns `Result<ProcessOutput, CoreError>` where `ProcessOutput` contains the final image and a full `AutoSharpDiagnostics` record.

This document walks the one-shot entry point stage by stage. The same stages run
in the two-phase API used by the web UI and any interactive caller:

- `prepare_base(input, params, on_stage)` runs Stages 2 to 4 (downscale, classify,
  baseline) and caches them in a `PreparedBase`.
- `process_from_prepared(prepared, params, on_stage)` runs Stages 5 to 11 (probe,
  fit, solve, sharpen, output).

`PreparedBase` carries a `BaseParamsKey` fingerprint. The cache is reused only
when every base-affecting parameter still matches.

---

## Stage 1: Validate Parameters

`params.validate()` checks:
- Target width/height are non-zero
- `target_artifact_ratio` is in `[0.0, 1.0]`
- `sharpen_sigma` is positive
- `PaperLightnessApprox` requires `SharpenMode::Lightness`
- Probe config resolves to >= 4 sorted, positive values

Validation is fail-fast; any violation returns `CoreError::InvalidParams`.

---

## Stage 2: Downscale

`downscale(input, target)`

Implementation: `crates/r3sizer-core/src/resize.rs`

- Wraps the flat `Vec<f32>` as an `image::ImageBuffer<Rgb<f32>>` (zero-copy layout match)
- Calls `imageops::resize` with `FilterType::Lanczos3`
- Fast path: if target dimensions equal input dimensions, returns a `clone()` (no resampling)
- No clamping applied — Lanczos3 can introduce slight ringing (values just outside `[0, 1]`), and these are preserved intentionally

**Allocation:** One `Vec<f32>` of size `target_w * target_h * 3`.

**Design note:** The exact kernel from the original paper is unknown; Lanczos3 is a conservative engineering choice.

---

## Stage 3: Contrast Leveling (optional, disabled by default)

Implementation: `crates/r3sizer-core/src/contrast.rs`

When `enable_contrast_leveling = false` (default), this is a true no-op — returns immediately without touching the buffer.

When enabled, applies a per-channel percentile stretch:
1. For each channel (R, G, B independently):
   - Collect all values for that channel, sort them
   - Compute 1st percentile (`lo`) and 99th percentile (`hi`)
   - Rescale: `v' = (v - lo) / (hi - lo)`
2. Skip channels with constant value (range < 1e-6)

**Status:** This is a documented placeholder. The paper-exact formula is unknown. The module interface is frozen so only the body needs replacement.

**Allocation:** One temporary `Vec<f32>` of size `w * h` per channel (3 allocations, freed after each channel).

---

## Stage 4: Baseline Measurement

Implementation: `crates/r3sizer-core/src/metrics.rs`

The artifact ratio of the base image is measured using the configured `ArtifactMetric`:

- **`ChannelClippingRatio`** (default): fraction of f32 channel values strictly outside `[0.0, 1.0]`, denominator `W * H * 3`.
- **`PixelOutOfGamutRatio`**: fraction of pixels where *any* channel is strictly outside `[0.0, 1.0]`, denominator `W * H`.

Values exactly equal to 0.0 or 1.0 are **not** counted as artifacts.

The implementation uses integer accumulation (`u32` sum of boolean masks) rather than `filter().count()` to enable LLVM auto-vectorization.

**Purpose:** Captures resize-induced artifacts (Lanczos ringing) before sharpening. Used by `MetricMode::RelativeToBase` to isolate sharpening-only artifacts.

**Allocation:** None (single pass over existing buffer).

---

## Stage 5: Probe Sharpening Strengths

### 5a. Resolve probe strengths

`params.probe_strengths.resolve()`

`ProbeConfig` is one of:
- `Range { min, max, count }` — linearly spaced values (requires count >= 4, min > 0, min < max)
- `Explicit(Vec<f32>)` — user-supplied list (requires >= 4 values)
- `TwoPass { coarse_count, coarse_min, coarse_max, dense_count, window_margin }` — coarse scan, then dense refinement around the bracketed crossing

Result is always sorted ascending. Default: `TwoPass { coarse_count: 7, coarse_min: 0.003, coarse_max: 1.0, dense_count: 4, window_margin: 0.5 }`. The coarse pass stops early once 3 or more probes bracket P0.

### 5b. Build Gaussian kernel

`make_kernel(params.sharpen_sigma)`

Implementation: `crates/r3sizer-core/src/sharpen.rs`

Builds a 1-D normalized Gaussian kernel with `radius = ceil(3 * sigma)`. Default sigma = 1.0 gives kernel size 7 (radius 3). The kernel is built once and reused across all probes and the final sharpen.

### 5c. Extract luminance (lightness mode only)

When `sharpen_mode == SharpenMode::Lightness`, `color::extract_luminance(&base)` computes CIE Y luminance for each pixel:

```
L = 0.2126 * R + 0.7152 * G + 0.0722 * B
```

Coefficients are the Y row of the sRGB-to-XYZ matrix (IEC 61966-2-1). Extracted once and reused across all probes.

**Allocation:** One `Vec<f32>` of size `W * H`.

### 5d. Run probes

Implementation: `probe_strengths()` helper in `pipeline.rs`

For each strength `s_i`:

1. **Sharpen** — dispatched by `sharpen_image()`:
   - **RGB mode:** `unsharp_mask_with_kernel(base, s_i, kernel)` — 3-channel USM
   - **Lightness mode:** dispatched by `SharpenModel`:
     - `PracticalUsm`: `unsharp_mask_single_channel_with_kernel(luminance, w, h, s_i, kernel)`
     - `PaperLightnessApprox`: `paper_sharpen::paper_sharpen_lightness(luminance, w, h, s_i, kernel)` (currently delegates to USM)
   - Then `reconstruct_rgb_from_lightness(base, sharpened_L)`

2. **Measure** — dispatched by `ArtifactMetric`:
   - `ChannelClippingRatio`: `channel_clipping_ratio(&sharpened)` returns `P_total(s_i)`
   - `PixelOutOfGamutRatio`: `pixel_out_of_gamut_ratio(&sharpened)` returns `P_total(s_i)`

3. **Compute metric** — `compute_metric_value(p_total, baseline, metric_mode)`:
   - `AbsoluteTotal`: `metric_value = p_total`
   - `RelativeToBase`: `metric_value = max(0, p_total - baseline)`

4. **Breakdown** — `compute_metric_breakdown(sharpened, original, luma_original, luma_sharpened, artifact_metric, weights)` returns a `MetricBreakdown` with all four component scores populated: `GamutExcursion`, `HaloRinging`, `EdgeOvershoot`, and `TextureFlattening`. The pipeline computes the breakdown per probe only when `selection_policy != GamutOnly`, and at final measurement under `Full` diagnostics. The default `GamutOnly` path skips it to keep probing fast. The `selection_score` field drives fitting; `composite_score` is diagnostic only.

5. **Collect** — `ProbeSample { strength: s_i, artifact_ratio: p_total, metric_value, breakdown: Some(breakdown) }`

**Parallelism:** With the `parallel` feature (default, enabled via `rayon`), probes run in parallel using `par_iter().map(probe_one).collect()`. Without it, probes run sequentially.

**Memory per probe:**
- RGB mode: One `Vec<f32>` for blurred image + one for the result (reuses blur allocation). Total: ~2 * W * H * 3 * 4 bytes.
- Lightness mode: One `Vec<f32>` for blurred luminance (~W * H * 4 bytes) + one `Vec<f32>` for reconstructed RGB (~W * H * 3 * 4 bytes).

In sequential mode, temporaries are freed before the next probe. In parallel mode, up to `rayon::current_num_threads()` sets exist simultaneously.

### Unsharp mask formula

```
output[i] = (1 + amount) * input[i] - amount * blur(input, sigma)[i]
```

Equivalent to `input[i] + amount * (input[i] - blur[i])` but computed as a single fused expression. No clamping — out-of-range values are the artifact signal.

### Gaussian blur implementation

`crates/r3sizer-core/src/sharpen.rs` — `gaussian_blur()` (RGB) / `gaussian_blur_single_channel()` (single-channel)

Separable 2-pass (horizontal then vertical) with explicit edge handling:
1. **Left/top edge:** clamped indexing (`saturating_sub`)
2. **Interior:** no bounds checks (computed range guarantees in-bounds)
3. **Right/bottom edge:** clamped indexing (`.min(dim - 1)`)

The vertical pass reads from the horizontal output and writes to a fresh buffer. Both passes are written for sequential row access (cache-friendly).

**Allocation:** Two `Vec<f32>` of size `W * H * channels` (one for horizontal output, one for vertical output).

---

## Stage 6: Cubic Polynomial Fit

### Fit data preparation

Probe samples are converted to `(f64, f64)` pairs of `(strength, metric_value)`. In `RelativeToBase` mode, a known anchor `(0.0, 0.0)` is prepended — zero sharpening produces zero added artifacts by definition, constraining the fit to pass near the origin.

### FitStrategy dispatch

Two strategies:

**`FitStrategy::Cubic`** (default):
1. `fit::check_monotonicity(&probe_samples)` — verifies probe data is non-decreasing
2. `fit::fit_cubic_with_quality(&fit_data)` — least-squares cubic + `FitQuality` (R², residuals, min pivot)
3. On success: robustness checks (LOO stability), then `solve::find_sharpness(poly, p0, s_min, s_max, &probe_samples)`
4. On failure (or robustness failure): falls back to `find_sharpness_direct` (direct sample search) with typed `FallbackReason`

**`FitStrategy::DirectSearch`:**
- Skips fitting entirely; calls `find_sharpness_direct` directly
- `FitStatus::Skipped`

### Monotonicity check

`fit::check_monotonicity(&probe_samples)` — O(N) scan of sorted probe samples. Returns `(monotonic: bool, quasi_monotonic: bool)`. Non-monotonic data triggers `FallbackReason::MetricNonMonotonic` and falls back to direct search.

### Cubic fit internals

`crates/r3sizer-core/src/fit.rs` — `fit_cubic_with_quality()`

Fits `P_hat(s) = a*s^3 + b*s^2 + c*s + d` by solving the 4x4 Vandermonde normal equations:

```
(V^T V) * [d, c, b, a]^T = V^T * y
```

where `V[k][j] = x_k^j` (ascending power).

**All arithmetic uses f64.** The normal-equation matrix contains terms up to `x^6`; for `x = 4.0`, `x^6 = 4096`. Using f32 would cause catastrophic cancellation in Gaussian elimination.

The 4x4 system is solved by `gauss_solve_with_pivots` — Gaussian elimination with partial pivoting (pivot threshold: 1e-14) that also tracks the minimum pivot magnitude. Returns `CoreError::FitFailed` if numerically singular.

After solving, `FitQuality` is computed:
- **R²**: coefficient of determination — `1 - (RSS / TSS)` where TSS is total sum of squares
- **Residual sum of squares**: sum of `(predicted - actual)²` across all data points
- **Max residual**: largest absolute error
- **Min pivot**: smallest pivot magnitude during elimination (condition number proxy)

### LOO stability check

After a successful fit and solve, the pipeline performs leave-one-out cross-validation:

1. For each of N probe samples, refit the cubic dropping that sample
2. Re-solve for the root using the leave-one-out polynomial
3. Record `|s*_full - s*_drop_i| / s*_full`
4. `max_loo_root_change` = maximum across all N drops
5. `loo_stable = max_loo_root_change < 0.5`

If `!loo_stable`, the pipeline falls back to direct search with `FallbackReason::FitUnstable`.

N=7 refits of a 4x4 system is negligible cost (~100μs total).

### Robustness flags assembly

`RobustnessFlags` is assembled from:

| Flag | Source | Threshold |
|------|--------|-----------|
| `monotonic` | `check_monotonicity` | zero inversions |
| `quasi_monotonic` | `check_monotonicity` | ≤ 1 inversion |
| `r_squared_ok` | `FitQuality` | R² > 0.85 |
| `well_conditioned` | `FitQuality` | min_pivot > 1e-8 |
| `loo_stable` | LOO cross-validation | max change < 0.5 |
| `max_loo_root_change` | LOO cross-validation | raw value |

### Fallback reason determination

When the pipeline does not use the polynomial root, `determine_fallback_reason` maps the first matching condition to a `FallbackReason` (priority order):

1. `BudgetTooStrictForContent` — budget unreachable (baseline > P0)
2. `DirectSearchConfigured` — `FitStrategy::DirectSearch`
3. `FitFailed` — polynomial fit returned an error
4. `MetricNonMonotonic` — probe data not monotonically increasing
5. `FitUnstable` — LOO cross-validation shows unstable root
6. `RootOutOfRange` — fit succeeded but no root in [s_min, s_max]

---

## Stage 7: Root Solving

`crates/r3sizer-core/src/solve.rs` — `find_sharpness()`

Solves `P_hat(s*) = P0` for the optimal sharpening strength.

### Algebraic path

`roots_in_range(poly, p0, s_min, s_max)`

1. Shift: solve `a*s^3 + b*s^2 + c*s + (d - P0) = 0`
2. If `|a| < 1e-12`: degenerate, solve as quadratic/linear
3. Otherwise: Cardano's formula via depressed cubic substitution `s = t - b/(3a)`:
   - **Discriminant > 0:** Three distinct real roots — trigonometric method with `acos` clamped to `[-1, 1]`
   - **Discriminant ~ 0:** Repeated root (triple when `p = q = 0`, otherwise double + simple)
   - **Discriminant < 0:** One real root via cube root formula
4. Filter to `[s_min, s_max]`, reject non-finite values

**Root selection policy:** The *largest* in-range root is chosen — maximizes sharpening while staying within the artifact budget.

### Fallback path

`fallback_from_samples(samples, p0)`

Triggered when: no in-range algebraic root, or polynomial is degenerate, or fit failed entirely.

1. **Best within budget:** Among samples with `metric_value <= P0`, pick the one with the *largest* strength → `SelectionMode::BestSampleWithinBudget`
2. **Least bad:** If all samples exceed P0, pick the one with the *smallest* `metric_value` → `SelectionMode::LeastBadSample`

### Budget reachability

Checks whether the target P0 is structurally achievable:
- In `AbsoluteTotal` mode: if `baseline > P0`, the budget is unreachable (resize alone already exceeds the target)
- In `RelativeToBase` mode: always reachable (starts at 0 by construction)
- If the solver returned `LeastBadSample`, budget is also marked unreachable

If unreachable due to baseline, `SelectionMode` is overridden to `BudgetUnreachable`.

---

## Stage 8: Final Sharpening

Applies the selected strength `s*` to the base image using the same `sharpen_image()` helper as the probe loop — same sharpening mode (RGB or lightness), same pre-built kernel, same base luminance.

**Key invariant:** The `base` image is never mutated during probing. Each probe produces a fresh allocation, and the final sharpen also produces a fresh allocation from the original `base`.

---

## Stage 9: Measure Final Artifact Ratio

Before clamping, the artifact ratio of the final sharpened image is measured:
- `measured_artifact_ratio` — raw P_total(s*)
- `measured_metric_value` — mode-adjusted (absolute or relative-to-base)

These are recorded in diagnostics for quality verification. They may differ slightly from what the polynomial predicted due to fitting error.

---

## Stage 10: Clamp Policy

Two policies:

### `ClampPolicy::Clamp` (default)
```rust
*v = v.clamp(0.0, 1.0);
```
Simple hard clamp. Negative values become 0, values > 1 become 1.

### `ClampPolicy::Normalize`
1. Find the global maximum across all channels
2. If max > 0: divide all values by max, then clamp negatives to 0
3. If max <= 0 (degenerate): set all to 0

The negative-clamp after normalization is necessary because sharpening ringing can produce negative values, and the subsequent `linear_to_srgb(v.powf(1/2.4))` would produce NaN on negative input.

---

## Stage 11: Return Diagnostics

Assembles `AutoSharpDiagnostics` with:

| Field | Source |
|-------|--------|
| `input_size` / `output_size` | Recorded at entry |
| `sharpen_mode` | From params |
| `metric_mode` / `artifact_metric` | From params |
| `selection_policy` | From params |
| `target_artifact_ratio` | P0 from params |
| `baseline_artifact_ratio` | Stage 4 measurement |
| `probe_samples` | Stage 5 probe results (each with optional `MetricBreakdown`) |
| `fit_status` | `Success` / `Failed { reason }` / `Skipped` |
| `fit_coefficients` | `Some(CubicPolynomial)` if fit succeeded |
| `fit_quality` | `Some(FitQuality)` with R², RSS, max residual, min pivot |
| `crossing_status` | `Found` / `NotFoundInRange` / `NotAttempted` |
| `selected_strength` | s* from solver |
| `selection_mode` | How s* was chosen |
| `fallback_reason` | `Some(FallbackReason)` when not using polynomial root |
| `robustness` | `Some(RobustnessFlags)` — monotonicity, LOO, condition |
| `budget_reachable` | Whether P0 is achievable |
| `measured_artifact_ratio` | Actual P_total(s*) pre-clamp |
| `measured_metric_value` | Mode-adjusted final metric |
| `metric_components` | `Some(MetricBreakdown)` of final output |
| `timing` | `StageTiming` — per-stage microsecond wall-clock times |
| `provenance` | `StageProvenance` — per-stage faithfulness classification |

Returns `ProcessOutput { image, diagnostics }`.

---

## Data Types

All types defined in `crates/r3sizer-core/src/types.rs`.

### `LinearRgbImage`
- Interleaved `[R, G, B, R, G, B, ...]` flat `Vec<f32>`, row-major
- Values nominally in `[0.0, 1.0]` but intentionally unclamped during processing
- Constructors validate `data.len() == width * height * 3` and non-zero dimensions

### `AutoSharpParams`
Default configuration:
```
target: 800x600
probes: TwoPass { coarse_count: 7, coarse_min: 0.003, coarse_max: 1.0,
                  dense_count: 4, window_margin: 0.5 }
P0: 0.003 (0.3%)
sigma: 1.0
fit: Cubic
clamp: Clamp
sharpen_mode: Lightness
sharpen_strategy: ContentAdaptive
metric_mode: RelativeToBase
artifact_metric: ChannelClippingRatio
selection_policy: GamutOnly
experimental_sharpen_mode: LumaPlusChromaGuard (chroma guard on)
evaluator_config: Heuristic
contrast leveling: disabled
```

### `ProbeSample`
Contains `(strength, artifact_ratio, metric_value)` in f32, plus an optional
`breakdown: Option<MetricBreakdown>` with per-component scores.

### `CubicPolynomial`
Coefficients `(a, b, c, d)` in f64. `P_hat(s) = a*s^3 + b*s^2 + c*s + d`.

---

## Error Handling

`CoreError` variants:

| Variant | When |
|---------|------|
| `InvalidParams(String)` | Bad params (zero dims, bad sigma, too few probes) |
| `FitFailed(String)` | Singular matrix or too few data points — triggers fallback, not a hard error at pipeline level |
| `NoValidRoot { reason }` | Empty probe samples and no polynomial path — only if probes are truly empty |
| `BufferLengthMismatch` | `LinearRgbImage::new` with wrong data length |
| `EmptyImage` | Zero-dimension image construction |

The pipeline is designed to **always return a result**. `FitFailed` triggers the direct-search fallback. `NoValidRoot` is structurally impossible when at least 4 probes are configured (validation enforces this).

---

## Per-Stage Timing

Every pipeline invocation records wall-clock microsecond timing in a `StageTiming` struct.
Each stage is bracketed by `Instant::now()` / `.elapsed().as_micros()`:

| Field | Pipeline stage |
|-------|---------------|
| `resize_us` | Stage 2: downscale |
| `contrast_us` | Stage 3: contrast leveling (0 when disabled) |
| `baseline_us` | Stage 4: baseline measurement |
| `probing_us` | Stage 5: entire probe loop (all strengths, including parallel dispatch) |
| `fit_us` | Stage 6: cubic polynomial fit |
| `robustness_us` | Stage 7b: monotonicity + LOO stability checks |
| `final_sharpen_us` | Stage 8: final sharpening at s* |
| `clamp_us` | Stage 10: clamping + final measurement |
| `total_us` | End-to-end (includes validation and diagnostics assembly) |

Timing overhead is negligible — only `Instant::now()` calls between existing stages.

---

## Feature Flags

`Cargo.toml` features:

| Feature | Default | Effect |
|---------|---------|--------|
| `parallel` | yes | Enables `rayon` for parallel probe evaluation |
| `formats` | yes | Enables the PNG/JPEG/GIF/BMP/TIFF/WebP decoders in the `image` crate |
| `typegen` | no | Enables `ts-rs` derives and the TypeScript exporter test. Never enabled in production builds |

Without `parallel`, probes run sequentially. The `probe_strengths` function uses `#[cfg(feature = "parallel")]` to switch between `par_iter` and `iter`.

---

## Paper-Faithfulness Status

The current pipeline follows a paper-aligned architecture: linear-light processing,
post-resize sharpening, artifact-limited parameter selection, and cubic modeling of
artifact growth.

However, some internal operators remain practical approximations rather than verified
paper-exact reproductions. The classification system used in this document:

- **Confirmed** — matches a formula explicitly stated in the papers or standards
- **Paper-supported** — strong inference from paper context; not yet explicitly confirmed
- **Engineering choice** — a well-motivated practical choice; the paper's exact method is unknown
- **Engineering proxy** — measures something similar to the paper, but exact definition may differ
- **Placeholder** — stub implementation; paper method completely unknown

## Confirmed vs. Approximated

| Component | Status |
|-----------|--------|
| sRGB transfer function | **Confirmed** — IEC 61966-2-1 |
| CIE Y luminance coefficients | **Confirmed** — sRGB-to-XYZ Y row |
| Unsharp mask formula | **Engineering choice** — standard USM, consistent with cited values (1.09, 1.81, 2.17), but the paper's exact sharpening operator is not confirmed |
| Lightness reconstruction (`k = L'/L`) | **Paper-supported** — strong inference from paper; all available evidence supports this formula |
| Downscale kernel | **Engineering choice** — Lanczos3; paper kernel unknown |
| Contrast leveling formula | **Placeholder** — paper formula unknown |
| Artifact metric (channel-count fraction) | **Engineering proxy** — paper says "fraction of color values outside valid gamut" but exact counting rule (per-channel vs per-pixel) is unconfirmed |
| Cubic fit + Cardano solve | **Confirmed** — standard numerical methods |
