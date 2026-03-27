# Pipeline Implementation Summary

Detailed walkthrough of the `process_auto_sharp_downscale` pipeline as implemented in `imgsharp-core`. This document traces one invocation from entry to return, covering data flow, allocations, numerical decisions, and fallback logic.

---

## Entry Point

`crates/imgsharp-core/src/pipeline.rs:38` — `process_auto_sharp_downscale(input, params)`

The caller supplies a `LinearRgbImage` (already in linear RGB — the sRGB-to-linear conversion is the responsibility of `imgsharp-io` or the caller) and an `AutoSharpParams` struct.

Returns `Result<ProcessOutput, CoreError>` where `ProcessOutput` contains the final image and a full `AutoSharpDiagnostics` record.

---

## Stage 1: Validate Parameters

`pipeline.rs:45` — `params.validate()`

Checks:
- Target width/height are non-zero
- `target_artifact_ratio` is in `[0.0, 1.0]`
- `sharpen_sigma` is positive
- Probe config resolves to >= 4 sorted, positive values

Validation is fail-fast; any violation returns `CoreError::InvalidParams`.

---

## Stage 2: Downscale

`pipeline.rs:53` — `downscale(input, target)`

Implementation: `crates/imgsharp-core/src/resize.rs:20`

- Wraps the flat `Vec<f32>` as an `image::ImageBuffer<Rgb<f32>>` (zero-copy layout match)
- Calls `imageops::resize` with `FilterType::Lanczos3`
- Fast path: if target dimensions equal input dimensions, returns a `clone()` (no resampling)
- No clamping applied — Lanczos3 can introduce slight ringing (values just outside `[0, 1]`), and these are preserved intentionally

**Allocation:** One `Vec<f32>` of size `target_w * target_h * 3`.

**Design note:** The exact kernel from the original paper is unknown; Lanczos3 is a conservative engineering choice.

---

## Stage 3: Contrast Leveling (optional, disabled by default)

`pipeline.rs:58-60`

Implementation: `crates/imgsharp-core/src/contrast.rs:35`

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

`pipeline.rs:65` — `artifact_ratio(&base)`

Implementation: `crates/imgsharp-core/src/metrics.rs:28`

Counts the fraction of f32 channel values strictly outside `[0.0, 1.0]`:

```
P_base = (count of v where v < 0.0 or v > 1.0) / (W * H * 3)
```

Values exactly equal to 0.0 or 1.0 are **not** counted as artifacts.

The implementation uses integer accumulation (`u32` sum of boolean masks) rather than `filter().count()` to enable LLVM auto-vectorization.

**Purpose:** Captures resize-induced artifacts (Lanczos ringing) before sharpening. Used by `MetricMode::RelativeToBase` to isolate sharpening-only artifacts.

**Allocation:** None (single pass over existing buffer).

---

## Stage 5: Probe Sharpening Strengths

`pipeline.rs:70-90`

### 5a. Resolve probe strengths

`pipeline.rs:70` — `params.probe_strengths.resolve()`

`ProbeConfig` is either:
- `Range { min, max, count }` — linearly spaced values (requires count >= 4, min > 0, min < max)
- `Explicit(Vec<f32>)` — user-supplied list (requires >= 4 values)

Result is always sorted ascending. Default: `[0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]` (7 probes, non-uniform, denser near zero where the threshold crossing typically occurs).

### 5b. Build Gaussian kernel

`pipeline.rs:73` — `make_kernel(params.sharpen_sigma)`

Implementation: `crates/imgsharp-core/src/sharpen.rs:304`

Builds a 1-D normalized Gaussian kernel with `radius = ceil(3 * sigma)`. Default sigma = 1.0 gives kernel size 7 (radius 3). The kernel is built once and reused across all probes and the final sharpen.

### 5c. Extract luminance (lightness mode only)

`pipeline.rs:76-80`

When `sharpen_mode == SharpenMode::Lightness`, `color::extract_luminance(&base)` computes CIE Y luminance for each pixel:

```
L = 0.2126 * R + 0.7152 * G + 0.0722 * B
```

Coefficients are the Y row of the sRGB-to-XYZ matrix (IEC 61966-2-1). Extracted once and reused across all probes.

**Allocation:** One `Vec<f32>` of size `W * H`.

### 5d. Run probes

`pipeline.rs:82-90` — `probe_strengths(...)`

Implementation: `pipeline.rs:264-290`

For each strength `s_i`:

1. **Sharpen** — dispatched by `sharpen_image()` (`pipeline.rs:238`):
   - **RGB mode:** `unsharp_mask_with_kernel(base, s_i, kernel)` — 3-channel USM
   - **Lightness mode:** `unsharp_mask_single_channel_with_kernel(luminance, w, h, s_i, kernel)` then `reconstruct_rgb_from_lightness(base, sharpened_L)`

2. **Measure** — `artifact_ratio(&sharpened)` returns `P_total(s_i)`

3. **Compute metric** — `compute_metric_value(p_total, baseline, metric_mode)`:
   - `AbsoluteTotal`: `metric_value = p_total`
   - `RelativeToBase`: `metric_value = max(0, p_total - baseline)`

4. **Collect** — `ProbeSample { strength: s_i, artifact_ratio: p_total, metric_value }`

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

`crates/imgsharp-core/src/sharpen.rs:43` (RGB) / `sharpen.rs:190` (single-channel)

Separable 2-pass (horizontal then vertical) with explicit edge handling:
1. **Left/top edge:** clamped indexing (`saturating_sub`)
2. **Interior:** no bounds checks (computed range guarantees in-bounds)
3. **Right/bottom edge:** clamped indexing (`.min(dim - 1)`)

The vertical pass reads from the horizontal output and writes to a fresh buffer. Both passes are written for sequential row access (cache-friendly).

**Allocation:** Two `Vec<f32>` of size `W * H * channels` (one for horizontal output, one for vertical output).

---

## Stage 6: Cubic Polynomial Fit

`pipeline.rs:95-135`

### Fit data preparation

`pipeline.rs:101-107`

Probe samples are converted to `(f64, f64)` pairs of `(strength, metric_value)`. In `RelativeToBase` mode, a known anchor `(0.0, 0.0)` is prepended — zero sharpening produces zero added artifacts by definition, constraining the fit to pass near the origin.

### FitStrategy dispatch

Two strategies:

**`FitStrategy::Cubic`** (default):
1. `fit::fit_cubic(&fit_data)` — least-squares cubic via Vandermonde normal equations
2. On success: `solve::find_sharpness(poly, p0, s_min, s_max, &probe_samples)`
3. On failure: falls back to `find_sharpness_direct` (direct sample search)

**`FitStrategy::DirectSearch`:**
- Skips fitting entirely; calls `find_sharpness_direct` directly
- `FitStatus::Skipped`

### Cubic fit internals

`crates/imgsharp-core/src/fit.rs:29`

Fits `P_hat(s) = a*s^3 + b*s^2 + c*s + d` by solving the 4x4 Vandermonde normal equations:

```
(V^T V) * [d, c, b, a]^T = V^T * y
```

where `V[k][j] = x_k^j` (ascending power).

**All arithmetic uses f64.** The normal-equation matrix contains terms up to `x^6`; for `x = 4.0`, `x^6 = 4096`. Using f32 would cause catastrophic cancellation in Gaussian elimination.

The 4x4 system is solved by Gaussian elimination with partial pivoting (pivot threshold: 1e-14). Returns `CoreError::FitFailed` if numerically singular.

---

## Stage 7: Root Solving

`crates/imgsharp-core/src/solve.rs:43`

Solves `P_hat(s*) = P0` for the optimal sharpening strength.

### Algebraic path

`solve.rs:89` — `roots_in_range(poly, p0, s_min, s_max)`

1. Shift: solve `a*s^3 + b*s^2 + c*s + (d - P0) = 0`
2. If `|a| < 1e-12`: degenerate, solve as quadratic/linear
3. Otherwise: Cardano's formula via depressed cubic substitution `s = t - b/(3a)`:
   - **Discriminant > 0:** Three distinct real roots — trigonometric method with `acos` clamped to `[-1, 1]`
   - **Discriminant ~ 0:** Repeated root (triple when `p = q = 0`, otherwise double + simple)
   - **Discriminant < 0:** One real root via cube root formula
4. Filter to `[s_min, s_max]`, reject non-finite values

**Root selection policy:** The *largest* in-range root is chosen — maximizes sharpening while staying within the artifact budget.

### Fallback path

`solve.rs:189` — `fallback_from_samples(samples, p0)`

Triggered when: no in-range algebraic root, or polynomial is degenerate, or fit failed entirely.

1. **Best within budget:** Among samples with `metric_value <= P0`, pick the one with the *largest* strength → `SelectionMode::BestSampleWithinBudget`
2. **Least bad:** If all samples exceed P0, pick the one with the *smallest* `metric_value` → `SelectionMode::LeastBadSample`

### Budget reachability

`pipeline.rs:140-152`

Checks whether the target P0 is structurally achievable:
- In `AbsoluteTotal` mode: if `baseline > P0`, the budget is unreachable (resize alone already exceeds the target)
- In `RelativeToBase` mode: always reachable (starts at 0 by construction)
- If the solver returned `LeastBadSample`, budget is also marked unreachable

If unreachable due to baseline, `SelectionMode` is overridden to `BudgetUnreachable`.

---

## Stage 8: Final Sharpening

`pipeline.rs:157-164`

Applies the selected strength `s*` to the base image using the same `sharpen_image()` helper as the probe loop — same sharpening mode (RGB or lightness), same pre-built kernel, same base luminance.

**Key invariant:** The `base` image is never mutated during probing. Each probe produces a fresh allocation, and the final sharpen also produces a fresh allocation from the original `base`.

---

## Stage 9: Measure Final Artifact Ratio

`pipeline.rs:169-174`

Before clamping, the artifact ratio of the final sharpened image is measured:
- `measured_artifact_ratio` — raw P_total(s*)
- `measured_metric_value` — mode-adjusted (absolute or relative-to-base)

These are recorded in diagnostics for quality verification. They may differ slightly from what the polynomial predicted due to fitting error.

---

## Stage 10: Clamp Policy

`pipeline.rs:179-207`

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

`pipeline.rs:212-230`

Assembles `AutoSharpDiagnostics` with:

| Field | Source |
|-------|--------|
| `input_size` / `output_size` | Recorded at entry |
| `sharpen_mode` / `metric_mode` | From params |
| `target_artifact_ratio` | P0 from params |
| `baseline_artifact_ratio` | Stage 4 measurement |
| `probe_samples` | Stage 5 probe results |
| `fit_status` | `Success` / `Failed { reason }` / `Skipped` |
| `fit_coefficients` | `Some(CubicPolynomial)` if fit succeeded |
| `crossing_status` | `Found` / `NotFoundInRange` / `NotAttempted` |
| `selected_strength` | s* from solver |
| `selection_mode` | How s* was chosen |
| `budget_reachable` | Whether P0 is achievable |
| `measured_artifact_ratio` | Actual P_total(s*) pre-clamp |
| `measured_metric_value` | Mode-adjusted final metric |

Returns `ProcessOutput { image, diagnostics }`.

---

## Data Types

All types defined in `crates/imgsharp-core/src/types.rs`.

### `LinearRgbImage`
- Interleaved `[R, G, B, R, G, B, ...]` flat `Vec<f32>`, row-major
- Values nominally in `[0.0, 1.0]` but intentionally unclamped during processing
- Constructors validate `data.len() == width * height * 3` and non-zero dimensions

### `AutoSharpParams`
Default configuration:
```
target: 800x600
probes: [0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]
P0: 0.001 (0.1%)
sigma: 1.0
fit: Cubic
clamp: Clamp
sharpen: Lightness
metric: RelativeToBase
contrast leveling: disabled
```

### `ProbeSample`
Triple of `(strength, artifact_ratio, metric_value)` in f32.

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

## Feature Flags

`Cargo.toml` features:

| Feature | Default | Effect |
|---------|---------|--------|
| `parallel` | yes | Enables `rayon` for parallel probe evaluation |

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
