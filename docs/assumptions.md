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

---

## Engineering approximations (not paper-exact)

| Implementation choice | Reason | How to improve |
|-----------------------|--------|----------------|
| **Lanczos3 downscale** | Exact kernel not confirmed | Replace `resize.rs` once kernel is known |
| **Unsharp mask sharpening** | Exact sharpening operator not confirmed | Replace `sharpen.rs`; example values (1.09, 1.81, 2.17) are consistent with a linear `amount` parameter |
| **Per-channel P metric** | Paper phrasing is ambiguous; per-channel is natural | Could be per-pixel, or evaluated in a perceptual colour space |
| **Probe range [0.5, 4.0], 9 samples** | Example values from paper suggest this range | Adjust via `ProbeConfig::Range` |
| **Gaussian sigma = 1.0** | Reasonable starting value for moderate downscale ratios | Expose as parameter (already done: `sharpen_sigma`) |
| **Contrast leveling: percentile stretch** | Exact formula not confirmed | Replace body of `contrast::apply_contrast_leveling` |
| **Contrast leveling order: before probing** | Order not confirmed | Architecture supports reordering |

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

---

## What this implementation is NOT

- Not a claim of paper-exact reproduction.
- Not an official implementation of any specific algorithm by the original authors.
- A clean, honest engineering reconstruction based on confirmed high-level
  principles and documented approximations for unknown details.
