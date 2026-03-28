# v0.3 Content-Adaptive Sharpening — Design Spec

**Date:** 2026-03-28
**Branch:** feature/content-adaptive-pipeline
**Scope:** Local sharpening gain via region classification. Typed contrast leveling strategy is a separate spec.

---

## 1. Summary

v0.3 introduces content-adaptive sharpening: the image is partitioned into region classes (flat, textured, strong edge, microtexture, risky halo zone), and sharpening strength is modulated per-pixel by a gain factor that depends on region class. The global solver is unchanged — it produces a scalar `s*` via uniform probing and cubic fitting as today. Adaptivity is applied only at the final sharpening step and validated against the artifact budget.

**Design contract: Global solve, local apply, validated final.**

---

## 2. Pipeline contract

```
[downscale] → [contrast leveling (stub)] →  ← pre-sharpen working image
    ↓
[Stage 2.5] classify(working_image) → RegionMap → GainMap      (ContentAdaptive only)
    ↓
[Stages 4–8] uniform probe / fit cubic / solve → s*_uniform    (unchanged)
    ↓
[Stage 9]  adaptive_sharpen(working_image, s*, gain_map)        (ContentAdaptive)
           sharpen(working_image, s*)                           (Uniform, unchanged)
    ↓
[Stage 9.5] validate P_adaptive ≤ P0; backoff if needed        (ContentAdaptive only)
    ↓
[Stage 10] clamp / normalize                                    (unchanged)
```

Key invariants:
- Classification is anchored to the **pre-sharpen working image** — the same image used by probes and final sharpening. If contrast leveling becomes real in a future spec, classification follows it automatically.
- Probe loop always uses **uniform** `s_i`. The gain map is not consulted during probing.
- `working_image` is never mutated. Each backoff iteration re-sharpens from the same base.
- Validation runs **before** Stage 10 clamp/normalize. Out-of-range linear values are the artifact signal being budgeted; clamping them before measurement would hide excursions.
- Backoff reuses pre-computed blur/detail buffers. It does not re-run probe, fit, or solve.

---

## 3. New types

### 3.1 `RegionClass`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub enum RegionClass {
    Flat,            // 0
    Textured,        // 1
    StrongEdge,      // 2
    Microtexture,    // 3
    RiskyHaloZone,   // 4
}
```

Stable `as usize` ordering is part of the public contract (documented explicitly, not implied by declaration order). `REGION_CLASS_COUNT = 5` constant provided for array sizing.

### 3.2 `RegionMap`

Dimension-carrying type. Dimensions are part of the contract to prevent accidental reuse with a wrong-sized image.

```rust
pub struct RegionMap {
    pub width:  u32,
    pub height: u32,
    data: Vec<RegionClass>,   // len == width * height, row-major
}
```

Constructor validates `data.len() == width * height`. Accessor: `get(x: u32, y: u32) -> RegionClass`.

### 3.3 `GainMap`

Same dimension-carrying layout as `RegionMap`.

```rust
pub struct GainMap {
    pub width:  u32,
    pub height: u32,
    data: Vec<f32>,           // len == width * height, row-major
}
```

Accessor: `get(x: u32, y: u32) -> f32`. Produced once per image from `RegionMap + GainTable`. Dimensions are verified against the image at the `adaptive_sharpen` call site.

### 3.4 `GainTable`

Named per-class fields. Construction validates all values.

```rust
pub struct GainTable {
    pub flat:            f32,   // v0.3 default: 0.75
    pub textured:        f32,   // v0.3 default: 0.95
    pub strong_edge:     f32,   // v0.3 default: 1.00
    pub microtexture:    f32,   // v0.3 default: 1.10
    pub risky_halo_zone: f32,   // v0.3 default: 0.70
}
```

**Hard validation bound:** all values must be in `[0.25, 4.0]`. This prevents absurd configuration but does not imply values near the bounds are supported or tested.

**Recommended operating range (documented):** `[0.5, 1.5]`. The v0.3 preset fits well inside this.

**Design criterion for defaults:** misclassification should degrade gently, not dramatically. If true microtexture is misclassified as textured, `1.10 vs 0.95` is modest. If textured is misclassified as flat, `0.95 vs 0.75` is noticeable but not disastrous. The final validation/backoff provides a budget safety net for any classification error.

`GainTable::v03_default()` constructor returns the canonical v0.3 preset and is the expected starting point.

`gain_for(class: RegionClass) -> f32` accessor.

### 3.5 `ClassificationParams`

```rust
pub struct ClassificationParams {
    pub gradient_low_threshold:  f32,   // default: 0.05
    pub gradient_high_threshold: f32,   // default: 0.40
    pub variance_low_threshold:  f32,   // default: 0.001
    pub variance_high_threshold: f32,   // default: 0.010
    pub variance_window:         usize, // default: 5
}
```

**Validation at construction:**
- `gradient_low_threshold <= gradient_high_threshold`
- `variance_low_threshold <= variance_high_threshold`
- `variance_window` is odd and `>= 3`

**Threshold definitions are tied to the specific operators used in `classifier.rs`:**
- Gradient thresholds are on the **unnormalized Sobel scale** (raw magnitude, theoretical max ≈ 5.66 for luminance in [0,1]).
- Variance thresholds are in **squared-luminance variance units** (computed over luminance in [0,1]; theoretical max 0.25 for bounded data).

Changing the Sobel normalization or variance formula invalidates these defaults. Provenance: `EngineeringChoice`. Not portable constants from papers.

`ClassificationParams::default()` returns these values.

### 3.6 `SharpenStrategy`

New orchestration axis for strength distribution. **Orthogonal to the existing `SharpenMode` (Rgb/Lightness) and `SharpenModel` (operator) controls.** Does not replace them.

```rust
pub enum SharpenStrategy {
    Uniform,
    ContentAdaptive {
        classification:        ClassificationParams,
        gain_table:            GainTable,
        max_backoff_iterations: u8,    // default: 4
        backoff_scale_factor:   f32,   // default: 0.8, must be in (0.0, 1.0)
    },
}
```

`SharpenStrategy::default()` returns `Uniform`. Added to `AutoSharpParams` alongside `SharpenMode` and `SharpenModel`.

### 3.7 `RegionCoverage`

```rust
pub struct RegionCoverage {
    pub total_pixels:             u32,
    pub flat:                     u32,
    pub textured:                 u32,
    pub strong_edge:              u32,
    pub microtexture:             u32,
    pub risky_halo_zone:          u32,
    pub flat_fraction:            f32,
    pub textured_fraction:        f32,
    pub strong_edge_fraction:     f32,
    pub microtexture_fraction:    f32,
    pub risky_halo_zone_fraction: f32,
}
```

Invariant: `flat + textured + strong_edge + microtexture + risky_halo_zone == total_pixels`.

### 3.8 `AdaptiveValidationOutcome`

`target_metric` lives in `AutoSharpParams` (existing `target_artifact_ratio`) and is not duplicated here.

```rust
pub enum AdaptiveValidationOutcome {
    PassedDirect {
        measured_metric: f32,
    },
    PassedAfterBackoff {
        iterations:      u8,
        final_scale:     f32,
        measured_metric: f32,
    },
    FailedBudgetExceeded {
        iterations:      u8,
        best_scale:      f32,
        best_metric:     f32,
    },
}
```

### 3.9 `AutoSharpDiagnostics` additions

```rust
pub region_coverage:     Option<RegionCoverage>,          // None when Uniform
pub adaptive_validation: Option<AdaptiveValidationOutcome>, // None when Uniform
```

`StageTiming` additions:
```rust
pub classification_us:        Option<u64>,  // None when Uniform
pub adaptive_validation_us:   Option<u64>,  // None when Uniform
```

---

## 4. `classifier.rs` module

### 4.1 Public API

```rust
pub fn classify(
    image: &LinearRgbImage,
    params: &ClassificationParams,
) -> RegionMap

pub fn gain_map_from_region_map(
    region_map: &RegionMap,
    gain_table: &GainTable,
) -> GainMap
```

`classifier.rs` is self-contained. It does not depend on `metrics/`. Any future shared low-level helpers (e.g. Sobel primitives) are extracted to a neutral utility layer only when real duplication accumulates — not preemptively.

### 4.2 Luminance coefficients

`L = 0.2126·R + 0.7152·G + 0.0722·B`

Same coefficients as `color.rs`. `classifier.rs` holds its own copy for module isolation. Source comment flags them as intentionally co-owned with `color.rs`; a shared constants location will be introduced if duplication spreads further.

### 4.3 Four-pass algorithm

**Pass 0 — Luminance extraction**
Compute `L` per pixel from RGB. Store in a temporary `Vec<f32>` of length `width * height`.

**Pass 1 — Gradient magnitude**
Unnormalized 3×3 Sobel on `L`. L2 norm: `g = sqrt(Gx² + Gy²)`.

```
Gx = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]]
Gy = [[-1, -2, -1], [0, 0, 0], [1, 2, 1]]
```

Theoretical maximum for luminance in [0, 1]: `4√2 ≈ 5.66`.

Thresholds in `ClassificationParams` are on **this unnormalized scale**. Border handling: edge-replicate (clamp pixel coordinates to valid range).

**Pass 2 — Local variance**
Square window of side `variance_window` (odd, `≥ 3`). For each pixel: mean and variance of the `variance_window²` luminance values in the window.

```
mean = (1/N) Σ L_i
variance = (1/N) Σ (L_i - mean)²
```

Thresholds in `ClassificationParams` are in **squared-luminance variance units** (max 0.25 for luminance in [0,1]). Border handling: edge-replicate.

**Pass 3 — Classification**
Per pixel, checked in the following **priority order** (part of public contract):

```
if g >= gradient_high && v >= variance_high  →  RiskyHaloZone
else if g >= gradient_high                   →  StrongEdge
else if v >= variance_high && g < gradient_low →  Microtexture
else if v >= variance_low || g >= gradient_low →  Textured
else                                         →  Flat
```

`Microtexture` requires high variance **and** low gradient — fine detail without significant edge energy. Moderate-gradient high-variance regions fall into `Textured`. `RiskyHaloZone` takes priority when both conditions are high.

### 4.4 `gain_map_from_region_map`

Simple per-pixel table lookup: `gain_map[i] = gain_table.gain_for(region_map.data[i])`. No computation beyond indexing.

---

## 5. `adaptive_sharpen` in `sharpen.rs`

### 5.1 Signature (lightness mode)

```rust
pub fn adaptive_sharpen_lightness(
    image: &LinearRgbImage,
    luminance: &[f32],        // pre-extracted, same pixel count as image
    strength: f32,
    gain_map: &GainMap,
    sigma: f32,
) -> LinearRgbImage
```

Equivalent RGB-mode variant: `adaptive_sharpen_rgb`.

Precondition: `gain_map.width == image.width && gain_map.height == image.height`. Enforced with a debug assertion at call site in `pipeline.rs`.

### 5.2 Detail buffer computation (once per adaptive-sharpen call group)

In lightness mode:
```
blur_L = gaussian_blur(luminance, sigma)
D = L - blur_L          // detail layer
```

In RGB mode: one blur and detail layer per channel.

These buffers are computed once. Backoff iterations reuse them — only the global scale factor changes.

### 5.3 Per-pixel application

```
effective_s(x, y) = strength × gain_map.get(x, y)
L'(x, y) = L(x, y) + effective_s(x, y) × D(x, y)
```

RGB reconstruction from `L'` via `k = L'/L` (lightness mode) — same formula as today.

**No clamping inside `adaptive_sharpen`.** Out-of-range values are the artifact signal. Clamping happens only in Stage 10.

---

## 6. Pipeline integration (detailed)

### 6.1 Stage 2.5 — Region classification *(ContentAdaptive only)*

Inserted after the pre-sharpen working image is established (post-downscale, post-contrast-leveling-stub). Timed separately as `classification_us`.

```rust
let region_map = classify(&working_image, &classification_params);
let gain_map   = gain_map_from_region_map(&region_map, &gain_table);
```

Both are immutable from this point forward.

`Uniform` path: stage entirely skipped.

### 6.2 Stages 4–8 — unchanged

Probe loop, fit, robustness checks, solve. Produce `s*_uniform` as today. Gain map not consulted.

### 6.3 Stage 9 — Final sharpening (branched)

*Uniform:* `sharpen(working_image, s*, sigma)` — unchanged.

*ContentAdaptive:*
Compute detail buffers once (`blur_L`, `D`). Then:

```rust
adaptive_sharpen(working_image, luminance_cache, s* × 1.0, &gain_map, sigma)
```

### 6.4 Stage 9.5 — Adaptive validation + backoff *(ContentAdaptive only)*

The metric used here is **the same selection metric the solver optimized against** (the artifact budget metric, not the composite diagnostic score). This preserves the contract between solver and budget check.

```
scale ← 1.0
result ← adaptive_sharpen(working_image, s*, scale, gain_map, detail_buffers)
P ← selection_metric(result)    // pre-clamp linear image

if P ≤ P0:
    outcome ← PassedDirect { measured_metric: P }
    final_result ← result

else:
    best ← (scale=1.0, metric=P, result=result)

    for i in 1..=max_backoff_iterations:
        scale ×= backoff_scale_factor
        result ← adaptive_sharpen(working_image, s*, scale, gain_map, detail_buffers)
        P ← selection_metric(result)

        if P < best.metric:
            best ← (scale, P, result)

        if P ≤ P0:
            outcome ← PassedAfterBackoff { iterations: i, final_scale: scale, measured_metric: P }
            final_result ← best.result
            break
    else:
        outcome ← FailedBudgetExceeded {
            iterations: max_backoff_iterations,
            best_scale: best.scale,
            best_metric: best.metric,
        }
        final_result ← best.result   // best seen, not last tried
```

Backoff only re-runs `adaptive_sharpen + selection_metric`. Detail buffers are reused. No re-probe, no re-fit, no re-solve.

### 6.5 Stage 10 — Clamp/normalize

Applied to `final_result`. Unchanged.

---

## 7. Testing strategy

### 7.1 `ClassificationParams` and `GainTable` validation

- `gradient_low > gradient_high` → construction error
- `variance_low > variance_high` → construction error
- Even `variance_window` → construction error
- `variance_window < 3` → construction error
- Any `GainTable` value outside `[0.25, 4.0]` → construction error
- `GainTable::v03_default()` passes its own validation and returns documented values
- `gain_for(class)` returns the correct field for each of the 5 classes

### 7.2 `RegionMap` / `GainMap` construction

- `data.len() != width * height` → construction error
- Border shapes: 1×1, 1×N, N×1, 2×2 — `classify` and `gain_map_from_region_map` complete without panic
- `get(x, y)` on a boundary pixel does not panic

### 7.3 `classifier.rs` — pure rule tests (layer a)

Factor the decision rule into a small internal helper:
```rust
fn classify_features(g: f32, v: f32, params: &ClassificationParams) -> RegionClass
```

Test all five classes directly with hand-picked `(g, v)` pairs chosen to fall unambiguously inside each class region. These tests cover the classification logic independent of feature extraction.

### 7.4 `classifier.rs` — image-level feature extraction tests (layer b)

Tests assert presence of expected classes in the relevant region, not exact pixel-perfect class maps:

- Solid uniform image → all pixels `Flat`; `region_coverage.flat == width * height`
- Single hard step edge in a flat image → some `StrongEdge` pixels detected in the edge region
- Gentle smooth gradient patch → some `Textured` pixels detected
- Noisy region alongside a hard edge → some `RiskyHaloZone` pixels at the boundary
- Low-frequency sinusoidal patch (high variance, low Sobel response) → some `Microtexture` pixels detected. A hard checkerboard is **not** used — it generates high gradient and drifts toward `StrongEdge` or `RiskyHaloZone`.

**Coverage invariant (all image-level tests):** sum of all five class counts equals `width * height` exactly.

### 7.5 `adaptive_sharpen` kernel tests

These tests construct synthetic `GainMap` values directly, independent of `GainTable` validation bounds. `GainTable` bounds apply only to `GainTable` construction, not to the lower-level kernel.

- Gain map all `1.0` → result matches `unsharp_mask` at the same strength (within f32 tolerance)
- Gain map all `0.0` → output equals input
- Output dimensions match input
- Out-of-range values preserved (no clamping) — verified the same way as existing unsharp mask tests

### 7.6 Pipeline integration tests

**Uniform regression guard:**
`SharpenStrategy::Uniform` must produce:
- Pixel-identical output to the current baseline
- Identical selection metric and `SelectionMode`
- Identical existing semantic diagnostics fields
- `region_coverage: None`, `adaptive_validation: None`, `classification_us: None`, `adaptive_validation_us: None`
- Timing fields compared structurally (Some/None shape), not by exact value

**ContentAdaptive happy path:**
`v03_default()` gain table, generous `P0` → `PassedDirect`, `region_coverage` non-None, class counts sum to pixel count, `measured_metric ≤ P0`.

**Backoff path:**
Tight `P0` and boosted gain table → `PassedAfterBackoff` or `FailedBudgetExceeded`; `iterations > 0`.

**`FailedBudgetExceeded` result selection:**
Metric recomputed on `final_result` matches `best_metric` in outcome — `final_result` is best seen, not last tried.

**Metric consistency (pre-clamp):**
`measured_metric` in `AdaptiveValidationOutcome` matches the selection metric recomputed on the **pre-clamp linear image** that was selected as the adaptive result. Recomputing on the post-clamp output is not equivalent.

**Determinism:**
For the same input and params:
- `classify(...)` called twice returns identical `RegionMap`
- `process_auto_sharp_downscale(...)` with `ContentAdaptive` called twice returns pixel-identical output

---

## 8. Explicit scope boundaries

### In scope

- New types in `types.rs`: `RegionClass`, `RegionMap`, `GainMap`, `GainTable`, `ClassificationParams`, `SharpenStrategy`, `RegionCoverage`, `AdaptiveValidationOutcome`
- `classifier.rs` — new module: `classify`, `gain_map_from_region_map`
- `adaptive_sharpen_lightness` / `adaptive_sharpen_rgb` — new functions in `sharpen.rs`
- Pipeline integration: Stage 2.5, adaptive Stage 9 branch, Stage 9.5
- `AutoSharpDiagnostics` and `StageTiming` additions
- All tests in Section 7

### Explicitly deferred

**Typed contrast leveling strategies** — `contrast.rs` remains a placeholder. The `NoContrastLeveling / LocalContrastCompression / LumaOnlyMicrocontrast / EdgeAwareContrastLeveling` enum is a separate spec.

**Adaptive probing (Option B)** — considered and rejected for v0.3. Probe loop remains uniform. Revisit only if backoff frequency or model-gap evidence justifies the coupling.

**Soft/blended classification** — hard per-pixel assignment is the v0.3 contract. Weighted blending is future work.

**Threshold calibration** — `ClassificationParams` defaults are `EngineeringChoice`. Calibrating against real photographs is evaluation harness work, not implementation work for this spec.

**CLI integration** — `SharpenStrategy` CLI flag follows once core types and pipeline are stable. Not part of this spec.

**Perceptual quality validation** — no automated metric for "does the adaptive result look better." Out of scope for unit and integration tests.

**WASM / Tauri** — `classifier.rs` is pure computation; WASM compatibility follows from existing crate structure without additional work here.

---

## 9. Open questions / known unknowns

- Threshold defaults (`ClassificationParams`) are engineering starting points. Real calibration requires an evaluation harness run on diverse image categories. The defaults should be treated as a first hypothesis, not a tuned result.
- The five-class taxonomy and the priority order in Pass 3 are `EngineeringChoice`. If evaluation shows a class is rarely used or frequently misclassified, the taxonomy may evolve.
- Gain table defaults (`v03_default`) follow the "gentle degradation" criterion. They have not been validated against perceptual studies. Widening the range (e.g. toward Option 1 defaults) is straightforward once the evaluation harness is in place.
- Whether `FailedBudgetExceeded` should trigger a further fallback (e.g. returning the `Uniform` result) is intentionally left for a future decision. For v0.3, `FailedBudgetExceeded` returns the best adaptive result seen and records the outcome in diagnostics.
