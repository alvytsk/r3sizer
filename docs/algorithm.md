# Algorithm: Automatic Sharpness Adjustment When Reducing Digital Images

This document describes the pipeline implemented in `r3sizer-core`.

---

## Stage overview

```
1.  Input decoding              (r3sizer-io / load.rs)
2.  sRGB -> linear RGB          (color.rs)
3.  Downscale                   (resize.rs)
4.  Contrast leveling           (contrast.rs)       -- optional
5.  Baseline measurement        (metrics.rs)
6.  Probe sharpening            (sharpen.rs + color.rs + metrics.rs)
7.  Cubic fit                   (fit.rs)
8.  Root solving                (solve.rs)
9.  Final sharpening            (sharpen.rs + color.rs)
10. Clamp + output              (pipeline.rs + color.rs)
11. Save                        (r3sizer-io / save.rs)
```

---

## Stage 1: Input decoding

`r3sizer-io::load::load_as_linear` opens the file via the `image` crate,
converts to `Rgb8`, normalises bytes to f32 [0, 1], and immediately applies
the sRGB -> linear transform.  The returned `LinearRgbImage` is already in
linear light.

---

## Stage 2: sRGB -> linear RGB

`color::srgb_to_linear` applies the IEC 61966-2-1 (sRGB standard) transfer
function piecewise:

```
v / 12.92                           if v <= 0.04045
((v + 0.055) / 1.055) ^ 2.4        otherwise
```

All subsequent operations are performed in linear light so that averaging and
filtering are physically correct.

---

## Stage 3: Downscale in linear space

`resize::downscale` wraps the linear f32 buffer as `image::ImageBuffer<Rgb<f32>>`
and calls `imageops::resize` with `FilterType::Lanczos3`.

No clamping is applied.  The output remains in linear f32.

**Note:** The specific resampling kernel used in the original papers is not
confirmed.  Lanczos3 is used as a high-quality standard choice.

---

## Stage 4: Contrast leveling (optional)

`contrast::apply_contrast_leveling` is applied to the downscaled image when
`enable_contrast_leveling = true`.

**Current implementation:** per-channel 1st-99th percentile stretch.

**Status:** placeholder.  The paper-exact formula is not yet known.

---

## Stage 5: Baseline measurement

Before any sharpening, the artifact ratio of the downscaled base image is
measured using the configured `ArtifactMetric`:

```
baseline_artifact_ratio = measure(base)   // dispatches on ArtifactMetric
```

This captures any out-of-range values introduced by the resize stage (e.g.
Lanczos ringing) independently of sharpening.

The baseline is used by `MetricMode::RelativeToBase` to isolate
sharpening-induced artifacts from resize-induced artifacts.

---

## Stage 6: Probe sharpening strengths

For each strength `s_i` in the probe set, sharpening is applied and artifacts
are measured.  Two sharpening modes are supported:

### Sharpening modes (`SharpenMode`)

**`SharpenMode::Rgb`** (legacy):

1. `sharpen::unsharp_mask(base, s_i, sigma)` -- unsharp mask on all RGB channels.
2. Measure artifact ratio on the sharpened image.

**`SharpenMode::Lightness`** (default):

1. `color::extract_luminance(base)` -- CIE Y luminance: `L = 0.2126R + 0.7152G + 0.0722B`.
2. Sharpen luminance only (dispatched by `SharpenModel`, see below).
3. `color::reconstruct_rgb_from_lightness(base, sharpened_L)` -- multiplicative reconstruction:
   `k = L'/L; R' = k*R, G' = k*G, B' = k*B` (with epsilon guard for L near 0).
4. Measure artifact ratio on the reconstructed RGB image.

The luminance is extracted once before the probe loop and reused across all
probes for efficiency.

### Sharpening models (`SharpenModel`)

Orthogonal to `SharpenMode` (which selects *channels*), `SharpenModel` selects
the *operator*:

- **`PracticalUsm`** (default): Gaussian unsharp mask. Engineering choice -- the
  paper's exact sharpening operator is unknown. Works with both RGB and Lightness
  modes.
- **`PaperLightnessApprox`**: Scaffold for the paper-style lightness sharpening
  operator. Currently delegates to USM internally. Requires
  `SharpenMode::Lightness`.

### Metric modes

Each probe produces a raw `artifact_ratio` (P_total) and a `metric_value`
that depends on `MetricMode`:

- **`AbsoluteTotal`**: `metric_value = P_total(s)`.
- **`RelativeToBase`** (default): `metric_value = max(0, P_total(s) - baseline)`.

The `metric_value` is what gets fitted and compared against P0.

### Artifact metrics (`ArtifactMetric`)

**Engineering proxy** — the paper describes the target constraint in terms of the
"fraction of color values outside the valid RGB gamut". Two interpretations are
implemented, selectable via `ArtifactMetric`:

**`ChannelClippingRatio`** (default) — per-channel fraction:

```
P(s) = (count of channel values v where v < 0.0 or v > 1.0)
       -------------------------------------------------------
              total channel values  =  W x H x 3
```

**`PixelOutOfGamutRatio`** — per-pixel fraction:

```
P(s) = (count of pixels where any channel v < 0.0 or v > 1.0)
       --------------------------------------------------------
                      total pixels  =  W x H
```

In both cases, values exactly equal to 0.0 or 1.0 are not counted as artifacts.

### Sharpening formula (unsharp mask)

```
output[i] = input[i] + amount * (input[i] - gaussian_blur(input, sigma)[i])
```

`amount` is the probe strength `s`.  Values intentionally go outside [0,1].

### Default probe strengths

```
[0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]
```

Non-uniform, denser near zero where the crossing typically occurs.  Configurable
via `ProbeConfig::Explicit` or `ProbeConfig::Range`.

---

## Stage 6b: Metric breakdown (composite scaffold)

Each probe now produces a `MetricBreakdown` via `metrics::compute_metric_breakdown`,
which returns per-component scores:

| Component | v0.1 status |
|-----------|-------------|
| `GamutExcursion` | Active — delegates to the configured `ArtifactMetric` |
| `HaloRinging` | Stub — returns 0.0 |
| `EdgeOvershoot` | Stub — returns 0.0 |
| `TextureFlattening` | Stub — returns 0.0 |

The `aggregate` field of `MetricBreakdown` equals the scalar metric value used for
fitting, preserving backward compatibility. Each `ProbeSample` stores its
`breakdown: Option<MetricBreakdown>` for downstream analysis.

---

## Stage 7: Cubic polynomial fit

`fit::fit_cubic_with_quality` fits a cubic via Vandermonde normal equations in f64 and
returns both the polynomial and a `FitQuality` record:

```
P_hat(s) = a*s^3 + b*s^2 + c*s + d
```

The function takes generic `(x, y)` data points.  In `RelativeToBase` mode, the
pipeline prepends a known anchor point `(0.0, 0.0)` -- since zero sharpening
produces zero added artifacts by definition.  This constrains the fit to pass
near the origin.

The 4x4 normal equations `(V^T V)*c = V^T*P` are solved using Gaussian
elimination with partial pivoting.

Requires >= 4 data points.  Returns `CoreError::FitFailed` if the matrix is
numerically singular (pivot < 1e-14).

### Fit quality (`FitQuality`)

After a successful fit, the following quality metrics are computed:

| Field | Description |
|-------|-------------|
| `r_squared` | Coefficient of determination; 1.0 = perfect fit |
| `residual_sum_of_squares` | Sum of squared residuals between predicted and actual values |
| `max_residual` | Largest absolute residual across all data points |
| `min_pivot` | Smallest pivot magnitude during Gaussian elimination (condition proxy) |

These are stored in `diagnostics.fit_quality` (None when fit is skipped or fails).

---

## Stage 7b: Robustness checks

After fitting and before solving, several robustness checks are performed.  Results
are stored in `diagnostics.robustness` as `RobustnessFlags`:

### Monotonicity check

`fit::check_monotonicity` scans the sorted probe samples and counts inversions
(cases where `metric_value[i+1] < metric_value[i]`).

| Flag | Meaning |
|------|---------|
| `monotonic` | Zero inversions — P(s) is non-decreasing |
| `quasi_monotonic` | At most one inversion |

### Leave-one-out (LOO) stability

The pipeline refits the cubic N times, each time dropping one probe sample,
re-solves for the root, and measures the maximum relative change in s*:

```
max_loo_root_change = max_i |s*_full - s*_drop_i| / s*_full
```

`loo_stable = true` when `max_loo_root_change < 0.5` (50% relative shift).
N=7 refits of a 4x4 system is negligible cost.

### Derived flags

| Flag | Threshold |
|------|-----------|
| `r_squared_ok` | R² > 0.85 |
| `well_conditioned` | min_pivot > 1e-8 |

### Impact on solver

If robustness checks indicate unreliable fitting (non-monotonic data or LOO
instability), the pipeline falls back to direct search and records the appropriate
`FallbackReason` (see below).

---

## Stage 7c: Fallback reason determination

When the pipeline does not use the polynomial root (i.e. `selection_mode != PolynomialRoot`),
a typed `FallbackReason` is recorded in `diagnostics.fallback_reason`:

| Reason | Trigger |
|--------|---------|
| `BudgetTooStrictForContent` | Budget structurally unreachable (baseline > P0) |
| `DirectSearchConfigured` | `FitStrategy::DirectSearch` selected by caller |
| `FitFailed` | Polynomial fit returned an error |
| `MetricNonMonotonic` | Probe data is not monotonically increasing |
| `FitUnstable` | LOO cross-validation shows unstable root |
| `RootOutOfRange` | Fit succeeded but no root in [s_min, s_max] |

Priority order: first matching reason wins (listed in evaluation order above).
When `selection_mode == PolynomialRoot`, `fallback_reason` is `None`.

---

## Stage 8: Solve for s*

`solve::find_sharpness` solves `P_hat(s*) = P0` via Cardano's formula
(depressed cubic via substitution `s = t - b/(3a)`):

- Discriminant > 0: three distinct real roots (trigonometric method).
- Discriminant = 0: repeated root (special case: triple root when p = q = 0).
- Discriminant < 0: one real root (Cardano).

**Root selection:** the *largest* root in `[s_min, s_max]` is chosen, maximising
sharpness while staying within the artifact budget.

**Fallback** (when no algebraic root is in range):
1. Among probe samples with `metric_value <= P0`, pick the largest strength.
2. If none qualify, pick the sample with the smallest `metric_value`.

**Budget reachability:** The pipeline checks whether the target is structurally
achievable.  In `AbsoluteTotal` mode, if `baseline > P0`, the budget is marked
unreachable before the solver even runs.

### Selection modes

The solver reports how the strength was chosen via `SelectionMode`:

| Mode | Meaning |
|------|---------|
| `PolynomialRoot` | s* found from the cubic polynomial root |
| `BestSampleWithinBudget` | No polynomial root; largest sample within P0 |
| `LeastBadSample` | All samples exceed P0; picked minimum metric value |
| `BudgetUnreachable` | Budget structurally unreachable (e.g. baseline > P0) |

---

## Stage 9: Final sharpening

The selected strength `s*` is applied once more using the same sharpening mode
(RGB or lightness) as the probe stage.

The artifact ratio on this result is measured and recorded as
`measured_artifact_ratio` (raw) and `measured_metric_value` (mode-adjusted).

---

## Stage 10: Clamp and output encoding

`ClampPolicy::Clamp` (default): every channel is clamped to [0, 1].
`ClampPolicy::Normalize`: image is divided by its maximum value.

`color::image_linear_to_srgb` applies the inverse IEC 61966-2-1 function.
The result is scaled to [0, 255] u8 and written to disk.

---

## Per-stage timing

Every pipeline invocation records wall-clock microsecond timing via `StageTiming`:

| Field | What it measures |
|-------|------------------|
| `resize_us` | Lanczos3 downscale |
| `contrast_us` | Contrast leveling (0 when disabled) |
| `baseline_us` | Baseline artifact measurement |
| `probing_us` | Entire probe loop (all strengths) |
| `fit_us` | Cubic polynomial fitting |
| `robustness_us` | Monotonicity + LOO stability checks |
| `final_sharpen_us` | Final sharpening at selected s* |
| `clamp_us` | Clamping + final measurement |
| `total_us` | End-to-end pipeline time |

Timing is always collected (no feature flag needed).  Overhead is negligible —
`Instant::now()` calls between stages.

---

## Diagnostics

`AutoSharpDiagnostics` (serialisable to JSON) contains:

| Field | Description |
|-------|-------------|
| `input_size` / `output_size` | Image dimensions |
| `sharpen_mode` | `Rgb` or `Lightness` |
| `sharpen_model` | `PracticalUsm` or `PaperLightnessApprox` |
| `metric_mode` | `AbsoluteTotal` or `RelativeToBase` |
| `artifact_metric` | `ChannelClippingRatio` or `PixelOutOfGamutRatio` |
| `target_artifact_ratio` | P0 |
| `baseline_artifact_ratio` | P(base) before sharpening |
| `probe_samples` | All `(s_i, P_total(s_i), metric_value(s_i), breakdown)` tuples |
| `fit_status` | `Success`, `Failed { reason }`, or `Skipped` |
| `fit_coefficients` | Cubic [a, b, c, d] if fit succeeded |
| `fit_quality` | `FitQuality` (R², RSS, max residual, min pivot) or None |
| `crossing_status` | `Found`, `NotFoundInRange`, or `NotAttempted` |
| `selected_strength` | s* applied |
| `selection_mode` | How s* was chosen (see Selection modes above) |
| `fallback_reason` | `FallbackReason` enum or None (see Stage 7c) |
| `robustness` | `RobustnessFlags` (monotonicity, LOO, condition) or None |
| `budget_reachable` | Whether the target P0 is achievable |
| `measured_artifact_ratio` | P_total(s*) after final sharpen |
| `measured_metric_value` | Mode-adjusted metric of the final output |
| `metric_components` | `MetricBreakdown` of the final output (component-wise scores) |
| `timing` | `StageTiming` — per-stage microsecond wall-clock times |
| `provenance` | Per-stage faithfulness classification (see `StageProvenance`) |
