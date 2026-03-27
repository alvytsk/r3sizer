# Assumptions and Engineering Approximations

This document distinguishes what is confirmed from available paper sources from
what is an engineering approximation or placeholder.

---

## Confirmed from papers

| Detail | Source |
|--------|--------|
| All processing in linear RGB space | Downsizing paper (2016) |
| Float intermediate calculations | Downsizing paper (2016) |
| Resize is a separate stage before sharpening | Downsizing paper (2016) |
| Sharpening is applied *after* downscale | Downsizing paper (2016) |
| P = fraction of color values outside valid gamut | Auto-sharpness paper (2018) |
| P(s) is approximated by a cubic polynomial | Auto-sharpness paper (2018) |
| P0 = 0.1% (= 0.001) is used as a target threshold example | Auto-sharpness paper (2018) |
| Goal is to *maximise* sharpness subject to artifact budget | Auto-sharpness paper (2018) |
| Contrast leveling is a post-resize step | Downsizing paper (2016) |
| Lightness L = 0.2126R + 0.7152G + 0.0722B (CIE Y from linear sRGB) | sRGB standard (IEC 61966-2-1) |

---

## Confirmed and now implemented

| Detail | Status |
|--------|--------|
| Lightness formula `L = 0.2126R + 0.7152G + 0.0722B` | Implemented in `color::luminance_from_linear_srgb` |
| Processing in linear sRGB | Implemented throughout the pipeline |
| sRGB ↔ linear conversion (IEC 61966-2-1) | Implemented in `color::srgb_to_linear` / `linear_to_srgb` |
| Cubic polynomial fit of P(s) | Implemented in `fit::fit_cubic` (f64 Vandermonde normal equations) |
| Cardano's formula root solve | Implemented in `solve::find_sharpness` |
| Lightness-based sharpening with RGB reconstruction | Implemented in `color::reconstruct_rgb_from_lightness` (paper-supported) |
| Two artifact metric interpretations | `channel_clipping_ratio` and `pixel_out_of_gamut_ratio` in `metrics.rs` |
| Parallel probe evaluation | `rayon::par_iter` in `pipeline.rs` (default `parallel` feature) |

---

## Engineering approximations (not paper-exact)

| Implementation choice | Reason | How to improve |
|-----------------------|--------|----------------|
| **Lanczos3 downscale** | Engineering choice — exact kernel not confirmed | Replace `resize.rs` once kernel is known |
| **Unsharp mask sharpening** | Engineering choice — exact sharpening operator not confirmed; cited values (1.09, 1.81, 2.17) are consistent with USM's linear `amount` parameter but do not confirm USM is the paper's method | Replace `sharpen.rs` once operator is known |
| **P metric counting rule** | Engineering proxy — paper says "fraction of color values outside valid gamut"; both per-channel (`ChannelClippingRatio`) and per-pixel (`PixelOutOfGamutRatio`) are now implemented as alternatives | The paper may use a different colour-space measure entirely |
| **Gaussian sigma = 1.0** | Reasonable starting value for moderate downscale ratios | Expose as parameter (already done: `sharpen_sigma`) |
| **Contrast leveling: percentile stretch** | Placeholder — exact formula not confirmed | Replace body of `contrast::apply_contrast_leveling` |
| **Contrast leveling order: before probing** | Order not confirmed | Architecture supports reordering |
| **Lightness-based sharpening via `k = L'/L`** | Paper-supported — strong inference from paper context; all available evidence supports this formula | Upgrade to Confirmed once explicitly verified |
| **RelativeToBase metric mode** | Engineering choice — isolates sharpening artifacts from resize artifacts; assumes additive independence | May not be how the paper defines P(s); replace with paper-exact metric once known |
| **Probe strengths [0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]** | Non-uniform, denser near zero where crossings typically occur | Adjust via `ProbeConfig` once paper values are known |
| **(0, 0) anchor in RelativeToBase fit** | Physically motivated: zero sharpening = zero added artifacts | Remove if paper uses a different fitting strategy |

---

## Previously open, now resolved

| Question | Resolution |
|----------|------------|
| Probe range too narrow (all samples exceed budget) | Resolved: dense-near-zero probes + lightness mode eliminate the problem |
| Baseline artifacts from Lanczos mixing into P(s) | Resolved: `RelativeToBase` metric mode subtracts baseline; lightness mode produces zero-baseline images |
| Lightness formula was vague | Resolved: confirmed as CIE Y from the sRGB-to-XYZ matrix |

---

## Open questions

1. Is the sharpening operator a spatial unsharp mask, a frequency-domain filter,
   or something else?
2. Does P count per-channel values, per-pixel (any-channel), or use a custom
   colour-space measure?
3. Does contrast leveling interact with the P(s) probe phase (i.e. is it applied
   inside the probe loop or only once)?
4. What are the exact probe strength values and sample count used in the paper?
5. Is the cubic fit applied per-channel (three separate fits) or to a
   channel-aggregated P?
6. Is the `k = L'/L` reconstruction the exact paper formula, or does the paper
   use a different colour-preserving method?
7. Is the metric computed on the full RGB reconstruction, or only on the
   lightness channel?

---

## What this implementation is NOT

- Not a claim of paper-exact reproduction.
- Not an official implementation of any specific algorithm by the original authors.
- A clean, honest engineering reconstruction based on confirmed high-level
  principles and documented approximations for unknown details.
