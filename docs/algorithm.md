# Algorithm: Automatic Sharpness Adjustment When Reducing Digital Images

This document describes the pipeline implemented in `r3sizer-core`.

---

## Stage overview

```
1.   Input decoding              (r3sizer-io / load.rs)
1.5. Input color-space ingress   (color_space.rs)        -- experimental, optional
2.   sRGB -> linear RGB          (color.rs)
2a.  Resize (staged shrink)      (resize.rs / resize_strategy.rs)
       -- bilinear pre-reduce when ratio >= 3x, then Lanczos3
2.5. Region classification       (classifier.rs)         -- ContentAdaptive only
3.   Contrast leveling           (contrast.rs)           -- optional
4.   Baseline measurement        (metrics/)
4a.  Base resize quality         (base_quality.rs)
       -- ringing always; edge/texture retention only in Full diagnostics
5.   Detail precomputation       D = input - blur(input)  -- computed once
5a.  TwoPass coarse scan         (pipeline.rs)
       -- early stop after 3+ probes when P0 bracket found
5b.  Dense window selection      (pipeline.rs)
5c.  TwoPass dense refinement    (pipeline.rs)
6.   Cubic fit                   (fit.rs)
7.   Root solving                (solve.rs)
7b.  LOO stability check         (pipeline.rs)
7c.  Robustness flags + fallback reason  (pipeline.rs)
8.   Final sharpening            (sharpen.rs + color.rs)
8.1  Chroma guard override       (chroma_guard.rs)       -- optional
9.   Measure final artifact ratio  (metrics/)
9.5. Quality evaluator           (evaluator.rs)          -- experimental, optional
10.  Clamp + output              (pipeline.rs + color.rs)
11.  Recommendations             (recommendations.rs)
12.  Save                        (r3sizer-io / save.rs)
```

---

## Stage 1: Input decoding

`r3sizer-io::load::load_as_linear` opens the file via the `image` crate,
converts to `Rgb8`, normalises bytes to f32 [0, 1], and immediately applies
the sRGB -> linear transform.  The returned `LinearRgbImage` is already in
linear light.

---

## Stage 1.5: Input color-space ingress (experimental v0.4)

When `params.input_color_space` is set, `color_space::prepare_input` intercepts
the pixel data before any processing:

| `InputColorSpace` | Behaviour |
|---|---|
| `Srgb` (default / `None`) | No-op — IO layer already linearised. |
| `LinearRgb` | Validates range; records `out_of_range_fraction` if values exceed [0, 1]. |
| `RawLinear` | Normalises to [0, 1] by dividing by the per-image maximum; records `normalization_scale`. |

Diagnostics are stored in `AutoSharpDiagnostics::input_ingress` (`InputIngressDiagnostics`).

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

## Stage 2a: Downscale in linear space

`resize::downscale` performs Lanczos3 resampling via `fast_image_resize`
(SIMD-accelerated on x86-64).  The function operates on the raw f32 linear
pixel buffer.  No clamping is applied; the output remains in linear f32.

**Staged shrink:** for shrink ratios >= 3x (max of X and Y axes), a two-stage
path is used.  A fast bilinear pre-reduce brings the image to approximately 2x
the target dimensions, then a final Lanczos3 pass produces the output.  This
follows the same principle as libvips' `gap` parameter: the bilinear pass
cheaply removes the bulk of the high-frequency energy that would otherwise
alias through the Lanczos kernel, while the final Lanczos3 pass preserves
edge sharpness at the target scale.  For ratios below 3x, a single Lanczos3
pass is used directly.  The diagnostic field `used_staged_shrink` records
which path was taken.

**Note:** The specific resampling kernel used in the original paper is not
confirmed.  Lanczos3 is used as a high-quality standard choice.

### Optional: content-adaptive resize strategy (v0.4)

When `params.resize_strategy` is set, `resize_strategy::downscale_with_strategy`
is called instead.  Two variants are available:

- **`ResizeStrategy::Uniform { kernel }`** — single kernel applied to the entire image.
  Kernels: `Lanczos3`, `MitchellNetravali`, `CatmullRom`, `Gaussian`.
- **`ResizeStrategy::ContentAdaptive { classification, kernel_table }`** — classifies the
  source image, assigns a kernel per region class, produces one downsample per kernel,
  then blends pixels according to the per-pixel class assignment.

Diagnostics are stored in `AutoSharpDiagnostics::resize_strategy_diagnostics`
(`ResizeStrategyDiagnostics`): which kernels were used and the pixel-count per kernel.

---

## Stage 2.5: Region classification (ContentAdaptive sharpening only)

When `params.sharpen_strategy == ContentAdaptive`, `classifier::classify` runs a
four-pass algorithm on the downscaled image to assign each pixel a `RegionClass`:

| Pass | What it computes |
|---|---|
| 1 | CIE Y luminance extraction |
| 2 | Unnormalized 3×3 Sobel gradient magnitude (max ≈ 5.66 for luma in [0,1]) |
| 3 | Local variance in a configurable square window (default 5×5) |
| 4 | Per-pixel classification via priority rules (see below) |

### Classification priority rules

```
g >= gradient_high && v >= variance_high  →  RiskyHaloZone
g >= gradient_high                         →  StrongEdge
v >= variance_high && g < gradient_low    →  Microtexture
v >= variance_low  || g >= gradient_low   →  Textured
else                                       →  Flat
```

Default `ClassificationParams`: `gradient_low=0.05`, `gradient_high=0.40`,
`variance_low=0.001`, `variance_high=0.010`, `variance_window=5`.

`gain_map_from_region_map` converts the `RegionMap` to a `GainMap` (per-pixel f32
multipliers) using the `GainTable`.  The gain map is consumed in Stage 8.

Per-class pixel counts are stored in `AutoSharpDiagnostics::region_coverage`
(`RegionCoverage`).

---

## Stage 3: Contrast leveling (optional)

`contrast::apply_contrast_leveling` is applied to the downscaled image when
`enable_contrast_leveling = true`.

**Current implementation:** per-channel 1st-99th percentile stretch.

**Status:** placeholder.  The paper-exact formula is not yet known.

---

## Stage 4: Baseline measurement

Before any sharpening, the artifact ratio of the downscaled base image is
measured using the configured `ArtifactMetric`:

```
baseline_artifact_ratio = measure(base)   // dispatches on ArtifactMetric
```

This captures any out-of-range values introduced by the resize stage (e.g.
Lanczos ringing) independently of sharpening.

The baseline is used by `MetricMode::RelativeToBase` to isolate
sharpening-induced artifacts from resize-induced artifacts.

When `params.evaluation_color_space` is set, the measurement uses
`chroma_guard::evaluate_in_color_space` instead of the standard metric.

### Base resize quality (`BaseResizeQuality`)

Immediately after the resize stage, `base_quality::score_base_resize` assesses
the downscaled image:

- **Ringing score** (always computed, no-reference) — sign-flip oscillation
  near edges on the resized luma.  Drives `envelope_scale`:
  `effective_p0 = target_artifact_ratio * envelope_scale`.
- **Edge retention** and **texture retention** (reference-aware, diagnostic-only
  in v1) — per-pixel Sobel energy ratio and local-variance ratio between the
  source and resized images.  These require expensive O(W*H) Sobel and variance
  passes on the full-resolution source image.

**Performance note:** source-side edge/texture retention is computed only when
`diagnostics_level == Full`.  In `Summary` mode (the default, and what
`Fast`/`Balanced` pipeline modes use), these fields are skipped to avoid the
O(W*H) Sobel + local-variance cost on the original input.

---

## Stage 5: Probe sharpening strengths

For each strength `s_i` in the probe set, sharpening is applied and artifacts
are measured.

### Detail precomputation

Before the probe loop begins, the detail signal `D = input - blur(input)` is
computed once from the base image (or base luminance in Lightness mode) using
the configured Gaussian kernel.  Each probe then applies sharpening as a
multiply-add `output = input + s * D`, avoiding a redundant blur per probe.
This collapses the per-probe cost from `O(blur) + O(metric)` to
`O(multiply-add) + O(metric)`, since `blur` dominates at typical image sizes.

### TwoPass probing (`ProbeConfig::TwoPass`)

The default probing strategy uses a two-pass adaptive scheme:

1. **Coarse scan** — `coarse_count` strengths (default 7) are evaluated at
   uniform spacing over `[coarse_min, coarse_max]`.  **Early stopping:** after
   the first 3 probes, the coarse scan exits as soon as a P0 crossing bracket
   is found (i.e. `metric_value[i] <= P0 && metric_value[i+1] > P0`).  This
   typically saves 2-4 probes at the tail of the range.

2. **Dense refinement** — the crossing bracket from the coarse pass is expanded
   by `window_margin` (default 0.5x the bracket width) on each side, then
   `dense_count` (default 4) probes are evaluated uniformly within that window.

3. **Merge** — coarse and dense samples are combined, sorted by strength, and
   near-duplicates (within 1e-5) are removed before fitting.

Both phases share the precomputed detail signal `D`, so the total cost is
`1 x blur + (coarse_used + dense_count) x (multiply-add + metric)`.

Diagnostics for the two-pass scheme are stored in
`AutoSharpDiagnostics::probe_pass_diagnostics` (`ProbePassDiagnostics`),
including the dense window bounds and how many coarse probes were actually
evaluated before early stopping.

For static probe configurations (`ProbeConfig::Explicit` or
`ProbeConfig::Range`), all strengths are evaluated in a single pass using
the same detail-precomputation optimization.

### Sharpening modes (`SharpenMode`)

**`SharpenMode::Rgb`** (legacy):

1. `sharpen::unsharp_mask(base, s_i, sigma)` -- unsharp mask on all RGB channels.
2. Measure artifact ratio on the sharpened image.

**`SharpenMode::Lightness`** (default):

1. `color::extract_luminance(base)` -- CIE Y luminance: `L = 0.2126R + 0.7152G + 0.0722B`.
2. Sharpen luminance only via unsharp mask.
3. `color::reconstruct_rgb_from_lightness(base, sharpened_L)` -- multiplicative reconstruction:
   `k = L'/L; R' = k*R, G' = k*G, B' = k*B` (with epsilon guard for L near 0).
4. Measure artifact ratio on the reconstructed RGB image.

The luminance is extracted once before the probe loop and reused across all
probes for efficiency.

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

## Stage 5b: Metric breakdown (composite — v0.2, all components active)

Each probe produces a `MetricBreakdown` via `metrics::compute_metric_breakdown`.
All four components are computed from real signal (none are stubs):

| Component | Method | What it measures |
|---|---|---|
| `GamutExcursion` | Dispatches on `ArtifactMetric` | Fraction of channel values outside [0, 1] |
| `HaloRinging` | Cross-edge profiles (Sobel + bilinear sampling) | Fraction of edge profiles with ≥2 sign changes in the diff |
| `EdgeOvershoot` | Same profiles | Mean of `max(0, peak_excursion / gradient_magnitude - 1)` |
| `TextureFlattening` | Local 5×5 variance comparison | Mean `|log2(var_sharpened / var_original)|` over textured pixels |

The `selection_score` field equals `GamutExcursion` and is what the solver fits.
`composite_score` is a weighted sum (`MetricWeights`) available for diagnostics
but does **not** drive s* selection in v0.2.

`aggregate` is a deprecated alias for `selection_score` kept for JSON backward
compatibility.

Each `ProbeSample` stores its `breakdown: Option<MetricBreakdown>`.  In
`DiagnosticsLevel::Summary` mode (default) per-probe breakdowns are stripped
from the serialized output to reduce JSON size.

---

## Stage 6: Cubic polynomial fit

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
|---|---|
| `r_squared` | Coefficient of determination; 1.0 = perfect fit |
| `residual_sum_of_squares` | Sum of squared residuals between predicted and actual values |
| `max_residual` | Largest absolute residual across all data points |
| `min_pivot` | Smallest pivot magnitude during Gaussian elimination (condition proxy) |

These are stored in `diagnostics.fit_quality` (None when fit is skipped or fails).

---

## Stage 7: Solve for s*

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
|---|---|
| `PolynomialRoot` | s* found from the cubic polynomial root |
| `BestSampleWithinBudget` | No polynomial root; largest sample within P0 |
| `LeastBadSample` | All samples exceed P0; picked minimum metric value |
| `BudgetUnreachable` | Budget structurally unreachable (e.g. baseline > P0) |

---

## Stage 7b: Robustness checks

After fitting and before solving, several robustness checks are performed.  Results
are stored in `diagnostics.robustness` as `RobustnessFlags`:

### Monotonicity check

`fit::check_monotonicity` scans the sorted probe samples and counts inversions
(cases where `metric_value[i+1] < metric_value[i]`).

| Flag | Meaning |
|---|---|
| `monotonic` | Zero inversions — P(s) is non-decreasing |
| `quasi_monotonic` | At most one inversion |

### Leave-one-out (LOO) stability

The pipeline refits the cubic N times, each time dropping one probe sample,
re-solves for the root, and measures the maximum relative change in s*:

```
max_loo_root_change = max_i |s*_full - s*_drop_i| / s*_full
```

`loo_stable = true` when `max_loo_root_change < 0.25` (25% relative shift).
N=7 refits of a 4x4 system is negligible cost.

### Derived flags

| Flag | Threshold |
|---|---|
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
|---|---|
| `BudgetTooStrictForContent` | Budget structurally unreachable (baseline > P0) |
| `DirectSearchConfigured` | `FitStrategy::DirectSearch` selected by caller |
| `FitFailed` | Polynomial fit returned an error |
| `MetricNonMonotonic` | Probe data is not monotonically increasing |
| `FitUnstable` | LOO cross-validation shows unstable root |
| `RootOutOfRange` | Fit succeeded but no root in [s_min, s_max] |

Priority order: first matching reason wins (listed in evaluation order above).
When `selection_mode == PolynomialRoot`, `fallback_reason` is `None`.

---

## Stage 8: Final sharpening

The selected strength `s*` is applied once more using the same sharpening mode
(RGB or lightness) as the probe stage.

### Uniform path (`SharpenStrategy::Uniform`)

Standard unsharp mask at `s*` — same as a single probe step.

### Content-adaptive path (`SharpenStrategy::ContentAdaptive`)

The gain map computed in Stage 2.5 modulates the effective strength per pixel:
`effective_strength(x, y) = s* * gain_map(x, y)`.  In lightness mode, the
detail signal (`luma - gaussian_blur(luma)`) is computed once and applied via
`sharpen::apply_adaptive_lightness_from_detail`.

**Backoff loop:** if the adaptive result exceeds the artifact budget P0, the
pipeline iteratively scales down the global strength by `backoff_scale_factor`
(default 0.8) for up to `max_backoff_iterations` (default 4) attempts.  The
outcome is reported via `AdaptiveValidationOutcome`:

| Outcome | Meaning |
|---|---|
| `PassedDirect` | Budget met on first try |
| `PassedAfterBackoff` | Budget met after N backoff iterations |
| `FailedBudgetExceeded` | Best result after all iterations still exceeds budget |

Stored in `AutoSharpDiagnostics::adaptive_validation`.

---

## Stage 8.1: Chroma guard override (experimental v0.4)

When `params.experimental_sharpen_mode = Some(LumaPlusChromaGuard { max_chroma_shift })`
(the default since v0.5), `chroma_guard::sharpen_with_chroma_guard` replaces the
output from Stage 8.

The chroma guard sharpens luminance, then monitors the per-pixel chroma shift
(in Cb/Cr space).  Where the shift exceeds `max_chroma_shift` (default 10%), a
soft clamp brings chroma back toward the original value.

Diagnostics are stored in `AutoSharpDiagnostics::chroma_guard` (`ChromaGuardDiagnostics`):

| Field | Description |
|---|---|
| `pixels_clamped_fraction` | Fraction of pixels where chroma clamping was applied |
| `mean_chroma_shift` | Mean chroma shift magnitude across all pixels |
| `max_chroma_shift` | Maximum chroma shift magnitude |

**Note:** This stage is **on by default** since v0.5.  To disable, set
`experimental_sharpen_mode: None` explicitly.

---

## Stage 9: Measure final artifact ratio

The artifact ratio on the final sharpened image (pre-clamp) is measured and recorded:
- `measured_artifact_ratio` — P_total(s*) raw.
- `measured_metric_value` — mode-adjusted (relative or absolute).
- `metric_components` — full `MetricBreakdown` of the final output.

---

## Stage 9.5: Quality evaluator (experimental v0.4)

When `params.evaluator_config = Some(Heuristic)` (the default since v0.5),
`evaluator::HeuristicEvaluator` runs after final sharpening and produces an
advisory quality prediction.  It does **not** change the selected s*.

The evaluator extracts `ImageFeatures` from the sharpened image:

| Feature | Description |
|---|---|
| `edge_density` | Fraction of pixels with Sobel magnitude > threshold |
| `mean_gradient_magnitude` | Mean Sobel magnitude across all pixels |
| `gradient_variance` | Variance of gradient magnitudes |
| `mean_local_variance` | Mean 5×5 local variance |
| `local_variance_variance` | Variance of local variances (texture heterogeneity) |
| `laplacian_variance` | Variance of the Laplacian response (frequency content proxy) |
| `luminance_histogram_entropy` | Shannon entropy of 64-bin luminance histogram |

Output is stored in `AutoSharpDiagnostics::evaluator_result` (`QualityEvaluation`):
`predicted_quality_score`, `confidence`, optional `suggested_strength`, and the raw
`features`.  All values are advisory.

**Note:** On by default since v0.5.  To disable, set `evaluator_config: None`.

---

## Stage 10: Clamp and output encoding

`ClampPolicy::Clamp` (default): every channel is clamped to [0, 1].
`ClampPolicy::Normalize`: image is divided by its maximum value.

`color::image_linear_to_srgb` applies the inverse IEC 61966-2-1 function.
The result is scaled to [0, 255] u8 and written to disk.

---

## Stage 11: Recommendations

`recommendations::generate_recommendations` inspects the completed
`AutoSharpDiagnostics` and emits a list of `Recommendation` objects.
Each recommendation contains:

- `kind` (`RecommendationKind`): what type of change is suggested
  (e.g. `SwitchToContentAdaptive`, `LowerStrongEdgeGain`, `RaiseArtifactBudget`,
  `SwitchToLightness`, `WidenProbeRange`, `LowerSigma`).
- `severity` (`Severity`): `Info`, `Suggestion`, or `Warning`.
- `confidence`: float in [0, 1].
- `reason`: human-readable explanation.
- `patch` (`ParamPatch`): self-contained partial parameter update the UI can
  apply and re-run.

Stored in `AutoSharpDiagnostics::recommendations`.

---

## Per-stage timing

Every pipeline invocation records wall-clock microsecond timing via `StageTiming`:

| Field | What it measures |
|---|---|
| `resize_us` | Lanczos3 or strategy-based downscale (includes staged shrink when applicable) |
| `contrast_us` | Contrast leveling (0 when disabled) |
| `baseline_us` | Baseline artifact measurement |
| `base_quality_us` | Base resize quality scoring (ringing + optional edge/texture retention) |
| `probing_us` | Entire probe loop (detail precomputation + all probe strengths) |
| `fit_us` | Cubic polynomial fitting + solve |
| `robustness_us` | Monotonicity + LOO stability checks |
| `final_sharpen_us` | Final sharpening at selected s* (including chroma guard) |
| `clamp_us` | Clamping + final measurement |
| `total_us` | End-to-end pipeline time |
| `classification_us` | Region classification (None when Uniform) |
| `adaptive_validation_us` | Adaptive backoff (None when Uniform) |
| `ingress_us` | Input color-space ingress (None when not configured) |
| `evaluator_us` | Quality evaluator (None when not configured) |

Timing is always collected (no feature flag needed).

---

## Pipeline modes (`PipelineMode`)

Three runtime performance-quality tradeoff modes are available.  Each mode
adjusts probe budgets, adaptive complexity, and diagnostic depth while
preserving user-chosen P0, sigma, target dimensions, metric, and sharpen mode.
Apply via `PipelineMode::apply` before passing params to the pipeline (or set
`params.pipeline_mode` and call `params.resolved()`).

### Fast

Minimal probing for lowest latency.  Trades some quality on complex images
for speed.

| Setting | Value |
|---|---|
| TwoPass coarse/dense | 4 / 2 |
| Sharpen strategy | `Uniform` (no classification, no gain map) |
| Chroma guard | Disabled |
| Evaluator | Disabled |

### Balanced (default)

Current default behaviour — no overrides.  Content-adaptive sharpening,
two-pass probing (7 coarse / 4 dense), chroma guard, heuristic evaluator.

### Quality

Extended probing with a wider dense window and full diagnostics.

| Setting | Value |
|---|---|
| TwoPass coarse/dense | 9 / 6 |
| TwoPass window margin | 0.7 |
| Sharpen strategy | `ContentAdaptive` (ensured; up to 6 backoff iterations) |
| Chroma guard | Ensured on |
| Evaluator | Ensured on |

---

## Diagnostics

`AutoSharpDiagnostics` (serialisable to JSON) contains:

| Field | Description |
|---|---|
| `input_size` / `output_size` | Image dimensions |
| `sharpen_mode` | `Rgb` or `Lightness` |
| `metric_mode` | `AbsoluteTotal` or `RelativeToBase` |
| `artifact_metric` | `ChannelClippingRatio` or `PixelOutOfGamutRatio` |
| `target_artifact_ratio` | P0 |
| `baseline_artifact_ratio` | P(base) before sharpening |
| `probe_samples` | All `(s_i, P_total(s_i), metric_value(s_i), breakdown)` tuples |
| `fit_status` | `Success`, `Failed { reason }`, or `Skipped` |
| `fit_coefficients` | Cubic [a, b, c, d] if fit succeeded |
| `fit_quality` | `FitQuality` (R², RSS, max residual, min pivot) or None |
| `crossing_status` | `Found`, `NotFoundInRange`, or `NotAttempted` |
| `robustness` | `RobustnessFlags` (monotonicity, LOO, condition) or None |
| `selected_strength` | s* applied |
| `selection_mode` | How s* was chosen (see Selection modes above) |
| `fallback_reason` | `FallbackReason` enum or None (see Stage 7c) |
| `budget_reachable` | Whether the target P0 is achievable |
| `measured_artifact_ratio` | P_total(s*) after final sharpen |
| `measured_metric_value` | Mode-adjusted metric of the final output |
| `metric_components` | `MetricBreakdown` of the final output (all four components) |
| `metric_weights` | `MetricWeights` used for composite score computation |
| `region_coverage` | Per-class pixel counts (None when Uniform) |
| `adaptive_validation` | `AdaptiveValidationOutcome` (None when Uniform) |
| `timing` | `StageTiming` — per-stage microsecond wall-clock times |
| `input_ingress` | `InputIngressDiagnostics` (None when not configured) |
| `resize_strategy_diagnostics` | Kernels used (None when default Lanczos3) |
| `chroma_guard` | `ChromaGuardDiagnostics` (None when not configured) |
| `evaluator_result` | `QualityEvaluation` advisory result (None when not configured) |
| `recommendations` | List of `Recommendation` objects (empty when none triggered) |

---

## Default parameter values

`AutoSharpParams::default()` is the canonical starting point.  Notable defaults:

| Parameter | Default |
|---|---|
| `probe_strengths` | `[0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]` |
| `target_artifact_ratio` | `0.001` (0.1%) |
| `sharpen_sigma` | `1.0` |
| `sharpen_mode` | `Lightness` |
| `metric_mode` | `RelativeToBase` |
| `artifact_metric` | `ChannelClippingRatio` |
| `fit_strategy` | `Cubic` |
| `output_clamp` | `Clamp` |
| `sharpen_strategy` | `Uniform` |
| `diagnostics_level` | `Summary` |
| `experimental_sharpen_mode` | `Some(LumaPlusChromaGuard { max_chroma_shift: 0.10 })` (**on by default**) |
| `evaluator_config` | `Some(Heuristic)` (**on by default**) |
| `input_color_space` | `None` (Srgb) |
| `resize_strategy` | `None` (Lanczos3) |
| `evaluation_color_space` | `None` (Rgb) |

The last two rows (chroma guard and evaluator) changed in v0.5 — callers that need
the minimal baseline should set both to `None` explicitly.
