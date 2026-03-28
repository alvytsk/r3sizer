# v0.2 Composite Metrics Design

## Overview

Implement the three remaining metric components (`HaloRinging`, `EdgeOvershoot`, `TextureFlattening`) that currently return 0.0, add a weighted composite score for diagnostic observation, and enrich the diagnostics pipeline — all without changing the solver's selection metric.

**Status:** The composite metric is observation-only in v0.2. The solver continues to use `GamutExcursion` (v0.1 behavior) for fitting and selection. The composite becomes eligible as a selection metric only after validation against the evaluation harness.

---

## 1. Module structure

Split `metrics.rs` into a submodule directory:

```
crates/r3sizer-core/src/
  metrics/
    mod.rs          — re-exports, compute_metric_breakdown(), MetricWeights
    gamut.rs        — channel_clipping_ratio, pixel_out_of_gamut_ratio (moved from metrics.rs)
    edges.rs        — Sobel gradient, EdgeProfile, cross-edge profile sampling (shared by halo + overshoot)
    halo.rs         — halo_ringing_score() — sign-change detection on EdgeProfile data
    overshoot.rs    — edge_overshoot_score() — relative peak excursion on EdgeProfile data
    texture.rs      — texture_flattening_score() — local variance ratio on luminance
    composite.rs    — weighted_aggregate() — combines 4 components into scalar
```

All existing public API is preserved (`crate::metrics::channel_clipping_ratio`, etc.). Zero breaking changes to external consumers.

---

## 2. Shared infrastructure: cross-edge profile sampling

Both HaloRinging and EdgeOvershoot operate on the same intermediate representation. A shared abstraction avoids duplicate edge detection and directional sampling.

### Pipeline

1. **Sobel gradient** on `luma_original` — one pass producing magnitude + direction per pixel.
2. **Edge selection:** pixels where `gradient_magnitude > edge_threshold` (default 0.05, with epsilon floor 1e-6 to avoid division by zero).
3. **Directional sampling:** For each edge pixel, sample a 1D profile of `diff = luma_sharpened - luma_original` **along the local gradient direction** (i.e. perpendicular to the edge tangent). Profile length = 5 pixels, centered on the edge pixel. Use bilinear interpolation for sub-pixel positions.
4. Return `Vec<EdgeProfile>` where each profile carries: position, gradient magnitude, and the 5 sampled diff values.

### EdgeProfile

```rust
struct EdgeProfile {
    /// Gradient magnitude at this edge pixel (local edge-strength proxy).
    pub gradient_magnitude: f32,
    /// 5 diff samples along the gradient direction, centered on the edge pixel.
    /// diff[i] = luma_sharpened[pos_i] - luma_original[pos_i]
    pub diff_samples: [f32; 5],
}
```

This is a pipeline-internal type, not part of the public API.

**Geometric note:** The gradient direction points across the edge (dark to bright). Sampling along it yields the cross-edge profile needed for ringing and overshoot detection.

---

## 3. Component algorithms

### 3.1 GamutExcursion (gamut.rs)

**Unchanged from v0.1.** Fraction of channel values (or pixels) outside [0, 1] in the sharpened RGB image. Evaluated on RGB (required by definition).

### 3.2 HaloRinging (halo.rs)

**Goal:** Detect sign-alternating oscillations near strong edges.

**Input:** luma_original + luma_sharpened (practical perceptual design choice for v0.2).

**Algorithm:** For each `EdgeProfile`, count sign changes in the 5-sample diff profile. Values below an epsilon floor (`|diff_sample| < 1e-5`) are treated as zero to avoid noise-driven false positives. 2+ sign changes in the profile = ringing.

**Score:** `(edge profiles with ringing) / (total edge profiles)`. Returns 0.0 if no edge pixels detected.

### 3.3 EdgeOvershoot (overshoot.rs)

**Goal:** Measure how much sharpening exceeds the chosen local edge-strength proxy.

**Input:** luma_original + luma_sharpened (practical perceptual design choice for v0.2).

**Algorithm:** For each `EdgeProfile`:
1. Find `peak_excursion = max(|diff_sample|)` across the 5-sample profile (captures the actual peak of the overshoot lobe, stronger than single-pixel measurement).
2. Compute `overshoot_ratio = peak_excursion / gradient_magnitude` (gradient magnitude is the local edge-strength proxy, not claimed to be true edge amplitude).

**Score:** Mean of `max(0, overshoot_ratio - 1.0)` over all edge profiles. A ratio of 1.0 means the sharpening difference equals the chosen local edge-strength proxy. Anything above is overshoot. Returns 0.0 if no edge pixels or no overshoot.

### 3.4 TextureFlattening (texture.rs)

**Goal:** Detect changes in fine-scale local variance in textured regions, penalizing both flattening and over-enhancement. Measures local fine-scale energy changes, not guaranteed semantic texture.

**Input:** luma_original + luma_sharpened (practical perceptual design choice for v0.2).

**Algorithm:**
1. Compute local variance of `luma_original` in 5x5 sliding windows.
2. Identify textured pixels: `variance_original > texture_threshold` (default 0.001).
3. Compute local variance of `luma_sharpened` in the same 5x5 windows.
4. For each textured pixel: `ratio = variance_sharpened / variance_original`.
5. Score = mean of `|log2(ratio)|` over textured pixels.

**Why log2:** Linear ratio is asymmetric — halving texture (0.5) deviates by 0.5 while doubling (2.0) deviates by 1.0. Log makes the penalty symmetric: `log2(0.5) = -1`, `log2(2.0) = +1`, `log2(1.0) = 0`.

Returns 0.0 if no textured pixels found.

---

## 4. Composite metric and weights

### MetricWeights

```rust
pub struct MetricWeights {
    pub gamut_excursion: f32,    // default: 1.0
    pub halo_ringing: f32,       // default: 0.3
    pub edge_overshoot: f32,     // default: 0.3
    pub texture_flattening: f32, // default: 0.1
}
```

**Default rationale:**
- GamutExcursion dominant (1.0) — proven v0.1 metric, solver is tuned against it.
- Halo and overshoot equal weight (0.3) — both are edge-related perceptual artifacts.
- Texture flattening lower (0.1) — least perceptually severe at typical sharpening levels.

**These are engineering defaults (`Provenance::EngineeringProxy`), not paper-confirmed.** They are a starting point for evaluation harness tuning.

No normalization across components in v1. All components are dimensionless, but their practical scales are only assumed to be comparable provisionally and must be validated against the evaluation harness.

### Aggregate computation

```
composite_score = w1 * gamut_excursion + w2 * halo_ringing + w3 * edge_overshoot + w4 * texture_flattening
```

Simple weighted sum. Per-component normalization can be added in a future version if scale divergence is observed, but not prematurely.

### Integration into the pipeline

**Two metric paths:**
1. **Selection metric** — `gamut_excursion` alone (v0.1 behavior). The solver fits and solves against this. No regression risk.
2. **Diagnostic metric** — the full composite breakdown. Computed per-probe and at final measurement. Logged in diagnostics alongside the selection metric. **Not used by the solver in v0.2.**

A future `ArtifactMetric::Composite` variant can be added when the evaluation harness confirms the weighted sum is stable enough to optimize against.

---

## 5. Luminance extraction and pipeline data flow

### SharpenResult (pipeline-internal)

```rust
/// Pipeline-internal result of a sharpening step.
/// Not part of the public API.
struct SharpenResult {
    pub image: LinearRgbImage,
    /// Luminance of the sharpened image in linear domain, before any
    /// clamp or transfer conversion.
    /// Hard invariant: luminance.len() == image.width() * image.height()
    pub luminance: Vec<f32>,
}
```

The public low-level sharpening API (`unsharp_mask_with_kernel`, etc.) is unchanged.

### Luminance sourcing by sharpening mode

- **Lightness mode:** `luminance` is the already-computed sharpened luma from the single-channel USM path. No extra analysis-only buffer needed.
- **RGB mode:** `luminance` is extracted from the sharpened RGB image via `extract_luminance()`. Adds one luminance extraction pass and one `Vec<f32>` allocation per probe in v0.2.

### Linear-domain invariant

The luminance in `SharpenResult` is always derived from the sharpened image in linear domain, before output transfer conversion and before any final clipping. This ensures artifact metrics see actual excursions.

### Evaluation domains

| Component | Input | Status |
|---|---|---|
| GamutExcursion | sharpened RGB image | Required by definition |
| HaloRinging | luma_original + luma_sharpened | Practical perceptual design choice for v0.2 |
| EdgeOvershoot | luma_original + luma_sharpened | Practical perceptual design choice for v0.2 |
| TextureFlattening | luma_original + luma_sharpened | Practical perceptual design choice for v0.2 |

### Updated `compute_metric_breakdown` signature

```rust
pub fn compute_metric_breakdown(
    sharpened: &LinearRgbImage,
    original: &LinearRgbImage,
    luma_original: &[f32],
    luma_sharpened: &[f32],
    artifact_metric: ArtifactMetric,
    weights: &MetricWeights,
) -> MetricBreakdown
```

---

## 6. Diagnostics and reporting

### Revised MetricBreakdown

```rust
pub struct MetricBreakdown {
    /// Individual component scores.
    pub components: BTreeMap<MetricComponent, f32>,

    /// Which metric drove selection (e.g. GamutExcursion in v0.2).
    pub selected_metric: MetricComponent,
    /// The value of the selected metric.
    pub selection_score: f32,

    /// Weighted composite score (diagnostic only in v0.2).
    pub composite_score: f32,

    /// Legacy alias for selection_score. Kept for backward compatibility.
    #[deprecated(note = "use selection_score")]
    pub aggregate: f32,
}
```

`BTreeMap<MetricComponent, f32>` instead of `Vec<(MetricComponent, f32)>` for deterministic ordering and named fields in JSON serialization.

### AutoSharpDiagnostics additions

```rust
/// Weights used for composite score computation.
pub metric_weights: MetricWeights,
/// Provenance of the weights.
pub metric_weights_provenance: Provenance,  // EngineeringProxy for v0.2
```

### CLI human-readable output

```
Selection:
  metric:              gamut_excursion
  selection_score:     0.00041

Metric breakdown (v0.2 — diagnostic only):
  gamut_excursion:     0.00041
  halo_ringing:        0.00022
  edge_overshoot:      0.00018
  texture_flattening:  0.00012
  composite_score:     0.00058  (not used for selection)
  weights:             [1.0, 0.3, 0.3, 0.1]  (engineering_proxy)
```

### CLI args additions

```
--metric-weights W1,W2,W3,W4    Composite metric weights [default: 1.0,0.3,0.3,0.1]
                                 Order: gamut, halo, overshoot, texture
--diagnostics-level {summary,full}  (default: summary)
                                 summary: final measurement breakdown only
                                 full: per-probe breakdowns included
```

### Sweep summary statistics

Per-component aggregates across the dataset: mean, median, p90, p95. This catches long-tail failures that mean/median miss.

### Per-probe breakdown verbosity

Per-probe per-component breakdowns are always computed internally (needed for evaluation), but JSON serialization is gated by `--diagnostics-level full` to prevent report size bloat in sweep mode.

---

## 7. Provenance and confidence

### Confirmed by design

- Module decomposition and API boundaries.
- GamutExcursion evaluated on RGB (required by definition).
- Composite metric is observation-only in v0.2; solver uses GamutExcursion.

### Engineering design choices (not paper-confirmed)

- Halo/Overshoot/Texture evaluated on luminance.
- Sobel gradient as edge-strength proxy.
- Cross-edge profile length (5 pixels) and interpolation method (bilinear).
- Epsilon thresholds (edge: 0.05, noise floor: 1e-5, texture: 0.001).
- Default metric weights (1.0, 0.3, 0.3, 0.1).
- log2 symmetry for texture flattening.
- Ringing threshold (2+ sign changes).

### Must be validated empirically

- Whether practical component scales are actually comparable.
- Whether the weighted sum is stable enough to eventually drive selection.
- Whether default thresholds work across diverse image categories.
- Whether the composite improves s* selection quality vs. GamutExcursion alone.

---

## 8. Testing strategy

### Unit tests per component

Each component file (`halo.rs`, `overshoot.rs`, `texture.rs`) includes:
- Known-zero cases (identical images, solid images).
- Known-positive cases (synthetic images with deliberate artifacts).
- Edge cases (empty images, single-pixel images, no edge pixels, no textured pixels).

### Integration tests

- Composite metric values are finite and non-negative for all probe strengths.
- `selection_score` matches `GamutExcursion` component value.
- `composite_score` equals weighted sum of components.
- `MetricBreakdown` has all 4 components populated.
- Backward compatibility: `aggregate` equals `selection_score`.
- Diagnostics JSON round-trips correctly (serialize then deserialize).

### Evaluation harness tests

- Sweep across test dataset produces stable per-component statistics.
- No component produces NaN/Inf on any test image.
- Per-probe breakdowns show monotonic or quasi-monotonic behavior for each component.
