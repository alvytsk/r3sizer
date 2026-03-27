# Summary — Lightness `L` in linear sRGB and its role in the algorithm

## New confirmed information

In **linear sRGB**, lightness `L` is computed as the `Y` coordinate of **CIEXYZ**:

```text
L = 0.2126 * R + 0.7152 * G + 0.0722 * B
```

Where:

- `R`, `G`, `B` are **linear** sRGB components
- values are represented as floating-point numbers in the range `[0.0, 1.0]`
- coefficients `0.2126`, `0.7152`, `0.0722` come from the sRGB → CIEXYZ conversion matrix
- the coefficients sum to `1.0`

## Practical consequences for the project

### 1. Linearization is mandatory

This formula is valid for **linear sRGB**, not for gamma-encoded image values from files.

So the processing pipeline must start with:

```text
nonlinear sRGB -> linear sRGB
```

Any sharpening / saturation / contrast-related processing should happen **after** linearization.

### 2. Lightness-based processing is now much more concrete

This strongly supports a lightness-oriented pipeline:

1. decode image
2. convert to linear sRGB
3. compute lightness `L`
4. apply sharpening to `L` (or to a lightness-related representation)
5. reconstruct RGB in a color-preserving way
6. measure out-of-range RGB values for artifact estimation

### 3. `L` is a first-class utility in the core library

Implemented in `color.rs`:

```rust
pub fn luminance_from_linear_srgb(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}
```

Also: `color::extract_luminance(&img) -> Vec<f32>` for whole-image extraction.

## Strongly reinforced reconstruction

The currently most plausible sharpening reconstruction is:

1. start from linear `R, G, B`
2. compute original lightness `L`
3. sharpen `L` to get `L'`
4. reconstruct color using a multiplicative ratio:

```text
k = L' / L
R' = k * R
G' = k * G
B' = k * B
```

This is classified as **paper-supported** — a strong inference from the paper context. All available evidence supports this formula, but it has not been explicitly confirmed verbatim from the paper. The implementation is complete in `color::reconstruct_rgb_from_lightness`.

## Important implementation notes

### Handle `L = 0`

For black pixels or near-black pixels, `L' / L` is unsafe.

The implementation should include a safe branch for:

- `L <= epsilon`
- black / near-black pixels
- division-by-zero avoidance

### Measure artifacts after RGB reconstruction

Even if sharpening is applied through `L`, the reconstructed `R'`, `G'`, `B'` may still go outside `[0, 1]`.

So the artifact metric `P(s)` should be measured on the reconstructed RGB result, not only on the lightness channel.

### Achromatic colors are a useful test case

For achromatic colors (`R = G = B`):

```text
L = R = G = B
```

This makes grayscale pixels a very useful test category for validation.

## What can now be considered close to baseline

### Strong practical baseline

- working color space: **linear sRGB**
- lightness formula:
  `L = 0.2126R + 0.7152G + 0.0722B`
- linearization before processing is required
- lightness-based processing is highly plausible for sharpening-related transforms

### Still not fully confirmed

- exact sharpening formula applied to `L` (USM is used as an engineering choice)
- exact RGB reconstruction formula after sharpening (`k = L'/L` is paper-supported but not confirmed verbatim)
- exact definition of `P(s)` counting rule (both per-channel and per-pixel are now implemented; paper's exact method is unknown)
- exact interaction with contrast leveling

## Updated project-level algorithm sketch

```text
1. Load raster image
2. Convert nonlinear sRGB-like RGB to linear sRGB
3. Compute lightness L = 0.2126R + 0.7152G + 0.0722B
4. Downscale in linear space
5. Apply sharpening to lightness-related representation
6. Reconstruct RGB in a color-preserving way
7. Compute artifact metric P(s) from resulting RGB out-of-range values
8. Fit / solve for target artifact threshold P0
9. Apply final sharpening
10. Convert back to output color space
```

## Main takeaway

This information significantly strengthens the current project reconstruction:

- `linear sRGB` is now even more firmly established as the working space
- lightness `L` is no longer vague — it has a concrete formula
- a lightness-centered sharpening pipeline is now much more actionable
- the `color` module should explicitly include both:
  - nonlinear ↔ linear conversion
  - lightness / luminance computation
