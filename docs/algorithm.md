# Algorithm: Automatic Sharpness Adjustment When Reducing Digital Images

This document describes the pipeline implemented in `imgsharp-core`.

---

## Stage overview

```
1. Input decoding          (imgsharp-io / load.rs)
2. sRGB → linear RGB       (color.rs)
3. Downscale               (resize.rs)
4. Contrast leveling       (contrast.rs)   — optional
5. Probe sharpening        (sharpen.rs + metrics.rs)
6. Cubic fit               (fit.rs)
7. Root solving            (solve.rs)
8. Final sharpening        (sharpen.rs)
9. Clamp + output          (pipeline.rs + color.rs)
10. Save                   (imgsharp-io / save.rs)
```

---

## Stage 1: Input decoding

`imgsharp-io::load::load_as_linear` opens the file via the `image` crate,
converts to `Rgb8`, normalises bytes to f32 [0, 1], and immediately applies
the sRGB → linear transform.  The returned `LinearRgbImage` is already in
linear light.

---

## Stage 2: sRGB → linear RGB

`color::srgb_to_linear` applies the IEC 61966-2-1 (sRGB standard) transfer
function piecewise:

```
v / 12.92                           if v ≤ 0.04045
((v + 0.055) / 1.055) ^ 2.4         otherwise
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

**Current implementation:** per-channel 1st–99th percentile stretch.

**Status:** placeholder.  The paper-exact formula is not yet known.

---

## Stage 5: Probe sharpening strengths

For each strength `s_i` in the probe set:

1. `sharpen::unsharp_mask(downscaled, s_i, sigma)` — unsharp mask, no clamping.
2. `metrics::artifact_ratio(sharpened)` — fraction of channel values outside [0,1].

The result is a list of `ProbeSample { strength, artifact_ratio }` pairs.

### Artifact ratio definition

```
P(s) = (count of channel values v where v < 0.0 or v > 1.0)
       -------------------------------------------------------
              total channel values  =  W × H × 3
```

Values exactly equal to 0.0 or 1.0 are not counted.

### Sharpening formula (unsharp mask)

```
output[i] = input[i] + amount × (input[i] − gaussian_blur(input, σ)[i])
```

`amount` is the probe strength `s`.  Values intentionally go outside [0,1].

---

## Stage 6: Cubic polynomial fit

`fit::fit_cubic` fits a cubic via Vandermonde normal equations in f64:

```
P_hat(s) = a·s³ + b·s² + c·s + d
```

The 4×4 normal equations `(VᵀV)·c = Vᵀ·P` are solved using Gaussian
elimination with partial pivoting.

Requires ≥ 4 probe samples.  Returns `CoreError::FitFailed` if the matrix is
numerically singular (pivot < 1e-14).

---

## Stage 7: Solve for s*

`solve::find_sharpness` solves `P_hat(s*) = P0` via Cardano's formula
(depressed cubic via substitution `s = t − b/(3a)`):

- Discriminant > 0: three distinct real roots (trigonometric method).
- Discriminant = 0: repeated root (special case: triple root when p = q = 0).
- Discriminant < 0: one real root (Cardano).

**Root selection:** the *largest* root in `[s_min, s_max]` is chosen, maximising
sharpness while staying within the artifact budget.

**Fallback** (when no algebraic root is in range):
1. Among probe samples with `artifact_ratio ≤ P0`, pick the largest strength.
2. If none qualify, pick the sample with the smallest `artifact_ratio`.

The fallback is recorded in `AutoSharpDiagnostics.fallback_used` and
`fallback_reason`.

---

## Stage 8: Final sharpening

`sharpen::unsharp_mask(downscaled, s*, sigma)` is applied once more.

The artifact ratio on this result is measured and recorded as
`measured_artifact_ratio` in the diagnostics.

---

## Stage 9: Clamp and output encoding

`ClampPolicy::Clamp` (default): every channel is clamped to [0, 1].
`ClampPolicy::Normalize`: image is divided by its maximum value.

`color::image_linear_to_srgb` applies the inverse IEC 61966-2-1 function.
The result is scaled to [0, 255] u8 and written to disk.

---

## Diagnostics

`AutoSharpDiagnostics` (serialisable to JSON) contains:

| Field | Description |
|-------|-------------|
| `selected_strength` | s* applied |
| `target_artifact_ratio` | P0 |
| `measured_artifact_ratio` | P(s*) after final sharpen |
| `probe_samples` | all (s_i, P(s_i)) pairs |
| `fit_coefficients` | cubic [a, b, c, d] if fit succeeded |
| `fallback_used` | true when direct-sample fallback was triggered |
| `fallback_reason` | human-readable explanation if fallback |
| `input_size` / `output_size` | image dimensions |
