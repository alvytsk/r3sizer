# v0.2 Composite Metrics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the three remaining metric components (HaloRinging, EdgeOvershoot, TextureFlattening) as real algorithms, add a weighted composite diagnostic score, update MetricBreakdown to use BTreeMap, and enrich CLI diagnostics — all without changing the solver's selection metric.

**Architecture:** Split `metrics.rs` into a `metrics/` submodule directory with one file per component. Add shared cross-edge profile infrastructure for halo and overshoot. Pipeline's `sharpen_image` returns a `SharpenResult` carrying luminance. Composite score is observation-only; solver continues using `GamutExcursion`.

**Tech Stack:** Rust 1.75+, serde, BTreeMap, no new external dependencies.

**Spec:** `docs/superpowers/specs/2026-03-28-v02-composite-metrics-design.md`

---

## File Map

### New files

| File | Responsibility |
|---|---|
| `crates/r3sizer-core/src/metrics/mod.rs` | Re-exports, `compute_metric_breakdown()`, `MetricWeights` |
| `crates/r3sizer-core/src/metrics/gamut.rs` | `channel_clipping_ratio`, `pixel_out_of_gamut_ratio` (moved from `metrics.rs`) |
| `crates/r3sizer-core/src/metrics/edges.rs` | Sobel gradient, `EdgeProfile`, `extract_edge_profiles()` |
| `crates/r3sizer-core/src/metrics/halo.rs` | `halo_ringing_score()` |
| `crates/r3sizer-core/src/metrics/overshoot.rs` | `edge_overshoot_score()` |
| `crates/r3sizer-core/src/metrics/texture.rs` | `texture_flattening_score()` |
| `crates/r3sizer-core/src/metrics/composite.rs` | `weighted_aggregate()` |

### Modified files

| File | What changes |
|---|---|
| `crates/r3sizer-core/src/metrics.rs` | Deleted — replaced by `metrics/` directory |
| `crates/r3sizer-core/src/types.rs` | `MetricBreakdown` revised (BTreeMap, `selection_score`, `composite_score`), add `MetricWeights`, add `DiagnosticsLevel`, add fields to `AutoSharpParams` and `AutoSharpDiagnostics` |
| `crates/r3sizer-core/src/lib.rs` | Re-export `MetricWeights`, `DiagnosticsLevel` |
| `crates/r3sizer-core/src/pipeline.rs` | `SharpenResult`, updated `probe_strengths` and `sharpen_image`, pass `original` + luminance to `compute_metric_breakdown` |
| `crates/r3sizer-core/tests/integration.rs` | Update existing tests for new `MetricBreakdown` shape, add v0.2 integration tests |
| `crates/r3sizer-cli/src/args.rs` | Add `--metric-weights`, `--diagnostics-level` |
| `crates/r3sizer-cli/src/output.rs` | Add Selection + Metric breakdown sections |
| `crates/r3sizer-cli/src/run.rs` | Pass `metric_weights` and `diagnostics_level` to params |
| `crates/r3sizer-cli/src/sweep.rs` | Add per-component stats, percentiles (p90, p95) |

---

## Task 1: Update `MetricBreakdown` and add `MetricWeights` in types.rs

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs:441-466` (MetricBreakdown and MetricComponent)
- Modify: `crates/r3sizer-core/src/types.rs:301-325` (AutoSharpParams)
- Modify: `crates/r3sizer-core/src/types.rs:509-570` (AutoSharpDiagnostics)
- Modify: `crates/r3sizer-core/src/lib.rs:28-34` (re-exports)

- [ ] **Step 1: Add `Ord` derive to `MetricComponent`**

In `crates/r3sizer-core/src/types.rs`, change the `MetricComponent` derive line:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricComponent {
    /// Fraction of channel values outside [0, 1] (existing metric_v0).
    GamutExcursion,
    /// Sign-alternating oscillations near strong edges (v0.2).
    HaloRinging,
    /// Sharpening exceeding local edge-strength proxy (v0.2).
    EdgeOvershoot,
    /// Changes in fine-scale local variance in textured regions (v0.2).
    TextureFlattening,
}
```

- [ ] **Step 2: Replace `MetricBreakdown` struct**

Replace the existing `MetricBreakdown` struct with:

```rust
/// Per-component breakdown of the composite artifact metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricBreakdown {
    /// Individual component scores.
    pub components: std::collections::BTreeMap<MetricComponent, f32>,

    /// Which metric drove solver selection (GamutExcursion in v0.2).
    pub selected_metric: MetricComponent,
    /// The value of the selected metric.
    pub selection_score: f32,

    /// Weighted composite score (diagnostic only in v0.2 — not used for selection).
    pub composite_score: f32,

    /// Legacy alias for `selection_score`. Kept for backward compatibility.
    #[deprecated(note = "use selection_score")]
    pub aggregate: f32,
}
```

- [ ] **Step 3: Add `MetricWeights` struct**

Add after `MetricBreakdown`:

```rust
/// Weights for the composite artifact metric.
///
/// Provenance: `EngineeringProxy` — these are starting-point defaults,
/// not paper-confirmed. Must be validated against the evaluation harness.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MetricWeights {
    pub gamut_excursion: f32,
    pub halo_ringing: f32,
    pub edge_overshoot: f32,
    pub texture_flattening: f32,
}

impl Default for MetricWeights {
    fn default() -> Self {
        Self {
            gamut_excursion: 1.0,
            halo_ringing: 0.3,
            edge_overshoot: 0.3,
            texture_flattening: 0.1,
        }
    }
}
```

- [ ] **Step 4: Add `DiagnosticsLevel` enum**

Add after `MetricWeights`:

```rust
/// Controls verbosity of serialized diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsLevel {
    /// Final measurement breakdown only (compact JSON).
    #[default]
    Summary,
    /// Per-probe breakdowns included (evaluation mode).
    Full,
}
```

- [ ] **Step 5: Add `metric_weights` and `diagnostics_level` to `AutoSharpParams`**

Add two fields to `AutoSharpParams`:

```rust
    /// Weights for the composite diagnostic metric. Default: [1.0, 0.3, 0.3, 0.1].
    pub metric_weights: MetricWeights,
    /// Verbosity level for serialized diagnostics.
    pub diagnostics_level: DiagnosticsLevel,
```

And in `Default for AutoSharpParams`, add:

```rust
            metric_weights: MetricWeights::default(),
            diagnostics_level: DiagnosticsLevel::default(),
```

- [ ] **Step 6: Add fields to `AutoSharpDiagnostics`**

Add after the `metric_components` field:

```rust
    /// Weights used for composite score computation.
    pub metric_weights: MetricWeights,
    /// Provenance of the metric weights.
    pub metric_weights_provenance: Provenance,
```

- [ ] **Step 7: Update re-exports in `lib.rs`**

In `crates/r3sizer-core/src/lib.rs`, add `MetricWeights` and `DiagnosticsLevel` to the `pub use types::` block:

```rust
pub use types::{
    ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, CrossingStatus,
    CubicPolynomial, DiagnosticsLevel, FallbackReason, FitQuality, FitStatus, FitStrategy,
    ImageSize, LinearRgbImage, MetricBreakdown, MetricComponent, MetricMode, MetricWeights,
    ProbeSample, ProbeConfig, ProcessOutput, Provenance, RobustnessFlags, SelectionMode,
    SharpenMode, SharpenModel, StageTiming, StageProvenance,
};
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo build -p r3sizer-core 2>&1 | head -30`

Expected: Compilation errors in `metrics.rs`, `pipeline.rs`, and `integration.rs` that reference the old `MetricBreakdown` shape. This is expected — we fix them in subsequent tasks.

- [ ] **Step 9: Commit**

```bash
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/lib.rs
git commit -m "feat(types): revise MetricBreakdown, add MetricWeights and DiagnosticsLevel"
```

---

## Task 2: Split `metrics.rs` into `metrics/` submodule — gamut.rs and mod.rs

**Files:**
- Delete: `crates/r3sizer-core/src/metrics.rs`
- Create: `crates/r3sizer-core/src/metrics/mod.rs`
- Create: `crates/r3sizer-core/src/metrics/gamut.rs`

- [ ] **Step 1: Create `metrics/gamut.rs`**

Create `crates/r3sizer-core/src/metrics/gamut.rs` with the two existing metric functions and their tests moved verbatim from `metrics.rs`:

```rust
//! Gamut excursion metric (v0.1 baseline).
//!
//! Counts channel values or pixels outside [0, 1] in linear RGB.
//! This is the selection metric used by the solver in v0.2.

use crate::types::LinearRgbImage;

/// Per-channel clipping ratio. Denominator = W * H * 3.
pub fn channel_clipping_ratio(img: &LinearRgbImage) -> f32 {
    let pixels = img.pixels();
    let total = pixels.len();
    if total == 0 {
        return 0.0;
    }
    let out_of_range: u32 = pixels
        .iter()
        .map(|&v| (!(0.0..=1.0).contains(&v)) as u32)
        .sum();
    out_of_range as f32 / total as f32
}

/// Per-pixel out-of-gamut ratio. Denominator = W * H.
pub fn pixel_out_of_gamut_ratio(img: &LinearRgbImage) -> f32 {
    let pixels = img.pixels();
    let total_pixels = pixels.len() / 3;
    if total_pixels == 0 {
        return 0.0;
    }
    let oog: u32 = pixels
        .chunks_exact(3)
        .map(|rgb| rgb.iter().any(|&v| !(0.0..=1.0).contains(&v)) as u32)
        .sum();
    oog as f32 / total_pixels as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LinearRgbImage;

    fn solid(width: u32, height: u32, value: f32) -> LinearRgbImage {
        let n = (width * height * 3) as usize;
        LinearRgbImage::new(width, height, vec![value; n]).unwrap()
    }

    #[test]
    fn all_zero_is_clean() {
        let img = solid(10, 10, 0.0);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn all_one_is_clean() {
        let img = solid(10, 10, 1.0);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn all_mid_is_clean() {
        let img = solid(10, 10, 0.5);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn one_out_of_range_component() {
        let mut data = vec![0.5_f32; 12];
        data[0] = -0.001;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 12.0;
        assert!((channel_clipping_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn all_components_above_one() {
        let img = solid(4, 4, 1.001);
        assert_eq!(channel_clipping_ratio(&img), 1.0);
    }

    #[test]
    fn all_components_below_zero() {
        let img = solid(4, 4, -0.5);
        assert_eq!(channel_clipping_ratio(&img), 1.0);
    }

    #[test]
    fn boundary_values_not_counted() {
        let mut data = vec![0.0_f32; 6];
        data[3] = 1.0;
        let img = LinearRgbImage::new(1, 2, data).unwrap();
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_all_zero_is_clean() {
        let img = solid(10, 10, 0.0);
        assert_eq!(pixel_out_of_gamut_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_all_one_is_clean() {
        let img = solid(10, 10, 1.0);
        assert_eq!(pixel_out_of_gamut_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_one_bad_channel_counts_one_pixel() {
        let mut data = vec![0.5_f32; 12];
        data[0] = -0.001;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 4.0;
        assert!((pixel_out_of_gamut_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn pixel_all_three_bad_counts_one_pixel() {
        let mut data = vec![0.5_f32; 12];
        data[0] = 1.5;
        data[1] = -0.1;
        data[2] = 2.0;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 4.0;
        assert!((pixel_out_of_gamut_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn pixel_ratio_leq_channel_ratio() {
        let mut data = vec![0.5_f32; 12];
        data[0] = 1.5;
        data[1] = -0.1;
        data[2] = 2.0;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        assert!(pixel_out_of_gamut_ratio(&img) <= channel_clipping_ratio(&img));
    }
}
```

- [ ] **Step 2: Create `metrics/mod.rs` with temporary pass-through**

Create `crates/r3sizer-core/src/metrics/mod.rs`:

```rust
//! Artifact metrics for the auto-sharpness pipeline.
//!
//! v0.2: four components (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening)
//! computed per-probe and at final measurement. The solver uses GamutExcursion for
//! selection; the composite score is diagnostic only.

mod gamut;

pub use gamut::{channel_clipping_ratio, pixel_out_of_gamut_ratio};

use std::collections::BTreeMap;

use crate::{ArtifactMetric, MetricBreakdown, MetricComponent, MetricWeights};
use crate::types::LinearRgbImage;

/// Compute the full per-component metric breakdown.
///
/// In v0.2, all four components are populated. The solver uses `selection_score`
/// (GamutExcursion) for fitting; `composite_score` is diagnostic only.
#[allow(unused_variables)] // luma args unused until halo/overshoot/texture are wired
pub fn compute_metric_breakdown(
    sharpened: &LinearRgbImage,
    original: &LinearRgbImage,
    luma_original: &[f32],
    luma_sharpened: &[f32],
    artifact_metric: ArtifactMetric,
    weights: &MetricWeights,
) -> MetricBreakdown {
    let gamut = match artifact_metric {
        ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(sharpened),
        ArtifactMetric::PixelOutOfGamutRatio => pixel_out_of_gamut_ratio(sharpened),
    };

    // v0.2 stub: halo, overshoot, texture return 0.0 until wired in later tasks.
    let halo = 0.0_f32;
    let overshoot = 0.0_f32;
    let texture = 0.0_f32;

    let composite_score = weights.gamut_excursion * gamut
        + weights.halo_ringing * halo
        + weights.edge_overshoot * overshoot
        + weights.texture_flattening * texture;

    let mut components = BTreeMap::new();
    components.insert(MetricComponent::GamutExcursion, gamut);
    components.insert(MetricComponent::HaloRinging, halo);
    components.insert(MetricComponent::EdgeOvershoot, overshoot);
    components.insert(MetricComponent::TextureFlattening, texture);

    #[allow(deprecated)]
    MetricBreakdown {
        components,
        selected_metric: MetricComponent::GamutExcursion,
        selection_score: gamut,
        composite_score,
        aggregate: gamut,
    }
}

/// Deprecated alias for [`channel_clipping_ratio`].
#[deprecated(note = "renamed to channel_clipping_ratio")]
pub fn artifact_ratio(img: &LinearRgbImage) -> f32 {
    channel_clipping_ratio(img)
}
```

- [ ] **Step 3: Delete old `metrics.rs`**

```bash
rm crates/r3sizer-core/src/metrics.rs
```

- [ ] **Step 4: Verify gamut tests pass**

Run: `cargo test -p r3sizer-core -- metrics::gamut`

Expected: All 13 gamut tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A crates/r3sizer-core/src/metrics/ && git rm crates/r3sizer-core/src/metrics.rs 2>/dev/null; git add crates/r3sizer-core/src/metrics.rs
git commit -m "refactor(metrics): split into submodule directory with gamut.rs"
```

---

## Task 3: Update pipeline.rs for new `compute_metric_breakdown` signature

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs`

- [ ] **Step 1: Add `SharpenResult` struct**

Add after the existing imports in `pipeline.rs`:

```rust
/// Pipeline-internal result of a sharpening step.
struct SharpenResult {
    image: LinearRgbImage,
    /// Luminance in linear domain, before clamp/transfer.
    /// Hard invariant: luminance.len() == image.width() * image.height()
    luminance: Vec<f32>,
}
```

- [ ] **Step 2: Update `sharpen_image` to return `SharpenResult`**

Replace the existing `sharpen_image` function:

```rust
fn sharpen_image(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    mode: SharpenMode,
    model: SharpenModel,
    amount: f32,
    kernel: &[f32],
) -> Result<SharpenResult, CoreError> {
    match mode {
        SharpenMode::Rgb => {
            let image = unsharp_mask_with_kernel(base, amount, kernel);
            let luminance = color::extract_luminance(&image);
            Ok(SharpenResult { image, luminance })
        }
        SharpenMode::Lightness => {
            let lum = base_luminance.expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;
            let sharpened_l = match model {
                SharpenModel::PracticalUsm => {
                    unsharp_mask_single_channel_with_kernel(lum, w, h, amount, kernel)
                }
                SharpenModel::PaperLightnessApprox => {
                    crate::paper_sharpen::paper_sharpen_lightness(lum, w, h, amount, kernel)
                }
            };
            let image = color::reconstruct_rgb_from_lightness(base, &sharpened_l);
            Ok(SharpenResult { image, luminance: sharpened_l })
        }
    }
}
```

- [ ] **Step 3: Update `probe_strengths` to pass new args to `compute_metric_breakdown`**

Add `weights: &MetricWeights` parameter to `probe_strengths`. Update the closure:

```rust
#[allow(clippy::too_many_arguments)]
fn probe_strengths(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    sharpen_model: SharpenModel,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    weights: &MetricWeights,
) -> Result<Vec<ProbeSample>, CoreError> {
    // Extract base luminance for metric evaluation (needed even in RGB mode for v0.2 metrics).
    let base_luma_for_metrics: Vec<f32> = match base_luminance {
        Some(l) => l.to_vec(),
        None => color::extract_luminance(base),
    };

    let probe_one = |&s: &f32| -> Result<ProbeSample, CoreError> {
        let result = sharpen_image(base, base_luminance, sharpen_mode, sharpen_model, s, kernel)?;
        let breakdown = crate::metrics::compute_metric_breakdown(
            &result.image,
            base,
            &base_luma_for_metrics,
            &result.luminance,
            artifact_metric,
            weights,
        );
        let p_total = breakdown.selection_score;
        let metric_value = compute_metric_value(p_total, baseline_artifact_ratio, metric_mode);
        Ok(ProbeSample { strength: s, artifact_ratio: p_total, metric_value, breakdown: Some(breakdown) })
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        strengths.par_iter().map(probe_one).collect()
    }

    #[cfg(not(feature = "parallel"))]
    {
        strengths.iter().map(probe_one).collect()
    }
}
```

- [ ] **Step 4: Update the main pipeline function**

In `process_auto_sharp_downscale`, update the call to `probe_strengths` to pass `&params.metric_weights`:

```rust
    let probe_samples = probe_strengths(
        &strengths,
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        params.sharpen_model,
        params.metric_mode,
        params.artifact_metric,
        baseline_artifact_ratio,
        &kernel,
        &params.metric_weights,
    )?;
```

Update the final sharpening + measurement section (step 8-9):

```rust
    // 8. Final sharpening
    let t0 = Instant::now();
    let selected_strength = solve_result.selected_strength;
    let final_result = sharpen_image(
        &base,
        base_luminance.as_deref(),
        params.sharpen_mode,
        params.sharpen_model,
        selected_strength,
        &kernel,
    )?;
    let final_sharpen_us = t0.elapsed().as_micros() as u64;

    // 9. Measure actual artifact ratio (pre-clamp)
    let base_luma_for_metrics: Vec<f32> = match base_luminance.as_deref() {
        Some(l) => l.to_vec(),
        None => color::extract_luminance(&base),
    };
    let final_breakdown = crate::metrics::compute_metric_breakdown(
        &final_result.image,
        &base,
        &base_luma_for_metrics,
        &final_result.luminance,
        params.artifact_metric,
        &params.metric_weights,
    );
    let measured_artifact_ratio = final_breakdown.selection_score;
    let measured_metric_value = compute_metric_value(
        measured_artifact_ratio,
        baseline_artifact_ratio,
        params.metric_mode,
    );

    let mut final_image = final_result.image;
```

Update the diagnostics struct construction to add new fields:

```rust
        metric_components: Some(final_breakdown),
        metric_weights: params.metric_weights,
        metric_weights_provenance: Provenance::EngineeringProxy,
```

- [ ] **Step 5: Build and check**

Run: `cargo build -p r3sizer-core 2>&1 | head -40`

Expected: Core crate compiles. Integration tests and CLI may still fail (they reference old `MetricBreakdown` shape).

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs
git commit -m "feat(pipeline): SharpenResult, pass luminance to compute_metric_breakdown"
```

---

## Task 4: Update integration tests for new MetricBreakdown shape

**Files:**
- Modify: `crates/r3sizer-core/tests/integration.rs`

- [ ] **Step 1: Update the `metric_breakdown_present_in_diagnostics` test**

Replace:

```rust
#[test]
fn metric_breakdown_present_in_diagnostics() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.expect("metric_components should be present");
    assert_eq!(mc.components.len(), 4);
    assert_eq!(mc.selected_metric, MetricComponent::GamutExcursion);
    assert!(mc.selection_score.is_finite());
    assert!(mc.composite_score.is_finite());
    // selection_score == gamut excursion component
    let gamut = mc.components[&MetricComponent::GamutExcursion];
    assert!((mc.selection_score - gamut).abs() < 1e-10);
}
```

- [ ] **Step 2: Update the `probe_samples_have_breakdown` test**

Replace:

```rust
#[test]
fn probe_samples_have_breakdown() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    for sample in &out.diagnostics.probe_samples {
        let bd = sample.breakdown.as_ref().expect("each probe should have breakdown");
        // selection_score should match artifact_ratio (gamut excursion is the selection metric)
        assert!((bd.selection_score - sample.artifact_ratio).abs() < 1e-6,
            "breakdown selection_score {} != artifact_ratio {}", bd.selection_score, sample.artifact_ratio);
    }
}
```

- [ ] **Step 3: Update the `metric_breakdown_aggregate_matches_measured_artifact_ratio` test**

Replace:

```rust
#[test]
fn metric_breakdown_selection_score_matches_measured() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    assert!((mc.selection_score - out.diagnostics.measured_artifact_ratio).abs() < 1e-6);
}
```

- [ ] **Step 4: Add v0.2 composite score test**

Add a new test:

```rust
#[test]
fn composite_score_equals_weighted_sum() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let mc = out.diagnostics.metric_components.unwrap();
    let w = &out.diagnostics.metric_weights;
    let expected = w.gamut_excursion * mc.components[&MetricComponent::GamutExcursion]
        + w.halo_ringing * mc.components[&MetricComponent::HaloRinging]
        + w.edge_overshoot * mc.components[&MetricComponent::EdgeOvershoot]
        + w.texture_flattening * mc.components[&MetricComponent::TextureFlattening];
    assert!((mc.composite_score - expected).abs() < 1e-6,
        "composite_score {} != weighted sum {}", mc.composite_score, expected);
}
```

- [ ] **Step 5: Add MetricWeights import and diagnostics fields test**

Add to imports at top:

```rust
use r3sizer_core::MetricWeights;
```

Add a test:

```rust
#[test]
fn diagnostics_contain_metric_weights() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let w = &out.diagnostics.metric_weights;
    assert_eq!(w.gamut_excursion, 1.0);
    assert_eq!(w.halo_ringing, 0.3);
    assert_eq!(w.edge_overshoot, 0.3);
    assert_eq!(w.texture_flattening, 0.1);
    assert_eq!(out.diagnostics.metric_weights_provenance, Provenance::EngineeringProxy);
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test -p r3sizer-core 2>&1 | tail -20`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/tests/integration.rs
git commit -m "test: update integration tests for v0.2 MetricBreakdown shape"
```

---

## Task 5: Implement `edges.rs` — Sobel gradient and cross-edge profile sampling

**Files:**
- Create: `crates/r3sizer-core/src/metrics/edges.rs`
- Modify: `crates/r3sizer-core/src/metrics/mod.rs` (add `mod edges;`)

- [ ] **Step 1: Write failing tests for Sobel gradient**

Create `crates/r3sizer-core/src/metrics/edges.rs`:

```rust
//! Shared cross-edge profile infrastructure for halo and overshoot metrics.
//!
//! Computes Sobel gradient on original luminance, selects edge pixels,
//! and samples a 1D profile along the gradient direction in the diff image.

/// Default gradient magnitude threshold for edge detection.
pub const DEFAULT_EDGE_THRESHOLD: f32 = 0.05;

/// Epsilon floor for gradient magnitude to avoid division by zero.
const GRADIENT_EPSILON: f32 = 1e-6;

/// A cross-edge profile sampled along the local gradient direction.
#[derive(Debug, Clone)]
pub struct EdgeProfile {
    /// Gradient magnitude at this edge pixel (local edge-strength proxy).
    pub gradient_magnitude: f32,
    /// 5 diff samples along the gradient direction, centered on the edge pixel.
    pub diff_samples: [f32; 5],
}

/// Extract edge profiles from original and sharpened luminance images.
///
/// Returns an empty Vec if no edge pixels exceed `edge_threshold`.
pub fn extract_edge_profiles(
    luma_original: &[f32],
    luma_sharpened: &[f32],
    width: usize,
    height: usize,
    edge_threshold: f32,
) -> Vec<EdgeProfile> {
    if width < 3 || height < 3 {
        return Vec::new();
    }

    let (grad_mag, grad_dx, grad_dy) = sobel_gradient(luma_original, width, height);

    let mut profiles = Vec::new();

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let idx = y * width + x;
            let mag = grad_mag[idx];
            if mag < edge_threshold.max(GRADIENT_EPSILON) {
                continue;
            }

            // Normalized gradient direction (points across the edge).
            let nx = grad_dx[idx] / mag;
            let ny = grad_dy[idx] / mag;

            // Sample 5 points along gradient direction: offsets -2, -1, 0, +1, +2.
            let mut diff_samples = [0.0_f32; 5];
            for (i, offset) in [-2.0_f32, -1.0, 0.0, 1.0, 2.0].iter().enumerate() {
                let sx = x as f32 + offset * nx;
                let sy = y as f32 + offset * ny;
                let orig = bilinear_sample(luma_original, width, height, sx, sy);
                let sharp = bilinear_sample(luma_sharpened, width, height, sx, sy);
                diff_samples[i] = sharp - orig;
            }

            profiles.push(EdgeProfile {
                gradient_magnitude: mag,
                diff_samples,
            });
        }
    }

    profiles
}

/// Sobel gradient: returns (magnitude, dx, dy) arrays.
fn sobel_gradient(
    luma: &[f32],
    width: usize,
    height: usize,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let n = width * height;
    let mut mag = vec![0.0_f32; n];
    let mut dx = vec![0.0_f32; n];
    let mut dy = vec![0.0_f32; n];

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let tl = luma[(y - 1) * width + (x - 1)];
            let tc = luma[(y - 1) * width + x];
            let tr = luma[(y - 1) * width + (x + 1)];
            let ml = luma[y * width + (x - 1)];
            let mr = luma[y * width + (x + 1)];
            let bl = luma[(y + 1) * width + (x - 1)];
            let bc = luma[(y + 1) * width + x];
            let br = luma[(y + 1) * width + (x + 1)];

            let gx = -tl + tr - 2.0 * ml + 2.0 * mr - bl + br;
            let gy = -tl - 2.0 * tc - tr + bl + 2.0 * bc + br;
            let m = (gx * gx + gy * gy).sqrt();

            let idx = y * width + x;
            dx[idx] = gx;
            dy[idx] = gy;
            mag[idx] = m;
        }
    }

    (mag, dx, dy)
}

/// Bilinear interpolation on a single-channel image.
fn bilinear_sample(
    data: &[f32],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
) -> f32 {
    let x0 = (x.floor() as isize).clamp(0, width as isize - 1) as usize;
    let y0 = (y.floor() as isize).clamp(0, height as isize - 1) as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);

    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let fx = fx.clamp(0.0, 1.0);
    let fy = fy.clamp(0.0, 1.0);

    let v00 = data[y0 * width + x0];
    let v10 = data[y0 * width + x1];
    let v01 = data[y1 * width + x0];
    let v11 = data[y1 * width + x1];

    v00 * (1.0 - fx) * (1.0 - fy)
        + v10 * fx * (1.0 - fy)
        + v01 * (1.0 - fx) * fy
        + v11 * fx * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_produce_zero_diff() {
        let luma = vec![0.5_f32; 8 * 8];
        let profiles = extract_edge_profiles(&luma, &luma, 8, 8, DEFAULT_EDGE_THRESHOLD);
        // Solid image has no edges -> no profiles
        assert!(profiles.is_empty());
    }

    #[test]
    fn vertical_edge_produces_profiles() {
        // 8x8 image: left half = 0.0, right half = 1.0 (strong vertical edge at x=4).
        let mut luma = vec![0.0_f32; 8 * 8];
        for y in 0..8 {
            for x in 4..8 {
                luma[y * 8 + x] = 1.0;
            }
        }
        // Sharpened: same but with overshoot at the boundary.
        let mut sharpened = luma.clone();
        for y in 0..8 {
            sharpened[y * 8 + 4] = 1.3; // overshoot
            sharpened[y * 8 + 3] = -0.1; // undershoot
        }
        let profiles = extract_edge_profiles(&luma, &sharpened, 8, 8, DEFAULT_EDGE_THRESHOLD);
        assert!(!profiles.is_empty(), "should detect edge profiles");
        for p in &profiles {
            assert!(p.gradient_magnitude > 0.0);
        }
    }

    #[test]
    fn image_too_small_returns_empty() {
        let luma = vec![0.5_f32; 2 * 2];
        let profiles = extract_edge_profiles(&luma, &luma, 2, 2, DEFAULT_EDGE_THRESHOLD);
        assert!(profiles.is_empty());
    }

    #[test]
    fn bilinear_sample_integer_coords() {
        let data = vec![1.0, 2.0, 3.0, 4.0]; // 2x2
        assert!((bilinear_sample(&data, 2, 2, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 0.0, 1.0) - 3.0).abs() < 1e-6);
        assert!((bilinear_sample(&data, 2, 2, 1.0, 1.0) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn bilinear_sample_midpoint() {
        let data = vec![0.0, 1.0, 0.0, 1.0]; // 2x2
        let mid = bilinear_sample(&data, 2, 2, 0.5, 0.5);
        assert!((mid - 0.5).abs() < 1e-6);
    }

    #[test]
    fn sobel_detects_horizontal_edge() {
        // 5x5 image: top half = 0.0, bottom half = 1.0.
        let mut luma = vec![0.0_f32; 5 * 5];
        for y in 3..5 {
            for x in 0..5 {
                luma[y * 5 + x] = 1.0;
            }
        }
        let (mag, _dx, _dy) = sobel_gradient(&luma, 5, 5);
        // Edge should be strongest at row 2 (boundary between 0 and 1).
        let edge_mag = mag[2 * 5 + 2];
        assert!(edge_mag > 0.1, "edge magnitude should be significant: {edge_mag}");
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/r3sizer-core/src/metrics/mod.rs`, add after `mod gamut;`:

```rust
mod edges;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p r3sizer-core -- metrics::edges`

Expected: All 6 edge tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/r3sizer-core/src/metrics/edges.rs crates/r3sizer-core/src/metrics/mod.rs
git commit -m "feat(metrics): add edges.rs with Sobel gradient and cross-edge profile sampling"
```

---

## Task 6: Implement `halo.rs` — halo ringing score

**Files:**
- Create: `crates/r3sizer-core/src/metrics/halo.rs`
- Modify: `crates/r3sizer-core/src/metrics/mod.rs`

- [ ] **Step 1: Write `halo.rs` with implementation and tests**

Create `crates/r3sizer-core/src/metrics/halo.rs`:

```rust
//! Halo/ringing metric: detect sign-alternating oscillations near strong edges.
//!
//! Uses cross-edge profiles from `edges.rs`. A profile exhibits ringing if it
//! contains 2+ sign changes (ignoring values below a noise floor epsilon).

use super::edges::EdgeProfile;

/// Epsilon floor: diff samples with |value| below this are treated as zero.
const NOISE_FLOOR: f32 = 1e-5;

/// Minimum sign changes in a 5-sample profile to classify as ringing.
const RINGING_THRESHOLD: usize = 2;

/// Compute the halo ringing score from pre-extracted edge profiles.
///
/// Score = (profiles with ringing) / (total profiles).
/// Returns 0.0 if `profiles` is empty.
pub fn halo_ringing_score(profiles: &[EdgeProfile]) -> f32 {
    if profiles.is_empty() {
        return 0.0;
    }

    let ringing_count = profiles
        .iter()
        .filter(|p| has_ringing(&p.diff_samples))
        .count();

    ringing_count as f32 / profiles.len() as f32
}

/// Check if a diff profile has sign-alternating oscillations.
fn has_ringing(samples: &[f32; 5]) -> bool {
    let mut sign_changes = 0usize;
    let mut prev_sign: Option<bool> = None; // true = positive, false = negative

    for &v in samples {
        if v.abs() < NOISE_FLOOR {
            continue; // skip near-zero samples
        }
        let positive = v > 0.0;
        if let Some(prev) = prev_sign {
            if positive != prev {
                sign_changes += 1;
            }
        }
        prev_sign = Some(positive);
    }

    sign_changes >= RINGING_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::edges::EdgeProfile;

    fn profile(diff: [f32; 5]) -> EdgeProfile {
        EdgeProfile {
            gradient_magnitude: 0.5,
            diff_samples: diff,
        }
    }

    #[test]
    fn empty_profiles_returns_zero() {
        assert_eq!(halo_ringing_score(&[]), 0.0);
    }

    #[test]
    fn no_ringing_monotonic_diff() {
        // All positive, increasing — no sign changes.
        let p = profile([0.01, 0.02, 0.05, 0.08, 0.1]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn one_sign_change_not_ringing() {
        // One sign change: negative then positive.
        let p = profile([-0.05, -0.02, 0.01, 0.03, 0.05]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn two_sign_changes_is_ringing() {
        // Classic ringing: positive, negative, positive.
        let p = profile([0.05, -0.03, 0.02, -0.01, 0.005]);
        assert_eq!(halo_ringing_score(&[p]), 1.0);
    }

    #[test]
    fn noise_floor_suppresses_false_positives() {
        // Values below noise floor should be ignored.
        let p = profile([0.05, 1e-6, -1e-6, 1e-6, 0.03]);
        // Only two non-zero samples (0.05, 0.03), both positive — no sign change.
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }

    #[test]
    fn mixed_profiles_correct_ratio() {
        let ringing = profile([0.05, -0.03, 0.02, -0.01, 0.005]);
        let clean = profile([0.01, 0.02, 0.05, 0.08, 0.1]);
        let score = halo_ringing_score(&[ringing, clean]);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn all_below_noise_floor_not_ringing() {
        let p = profile([1e-6, -1e-7, 1e-8, -1e-9, 1e-6]);
        assert_eq!(halo_ringing_score(&[p]), 0.0);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/r3sizer-core/src/metrics/mod.rs`, add:

```rust
mod halo;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p r3sizer-core -- metrics::halo`

Expected: All 7 halo tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/r3sizer-core/src/metrics/halo.rs crates/r3sizer-core/src/metrics/mod.rs
git commit -m "feat(metrics): add halo.rs with sign-change ringing detection"
```

---

## Task 7: Implement `overshoot.rs` — edge overshoot score

**Files:**
- Create: `crates/r3sizer-core/src/metrics/overshoot.rs`
- Modify: `crates/r3sizer-core/src/metrics/mod.rs`

- [ ] **Step 1: Write `overshoot.rs` with implementation and tests**

Create `crates/r3sizer-core/src/metrics/overshoot.rs`:

```rust
//! Edge overshoot metric: measure how much sharpening exceeds the local
//! edge-strength proxy (Sobel gradient magnitude).
//!
//! Uses cross-edge profiles from `edges.rs`. For each profile, finds the
//! peak excursion and compares it to the gradient magnitude.

use super::edges::EdgeProfile;

/// Compute the edge overshoot score from pre-extracted edge profiles.
///
/// Score = mean of `max(0, peak_excursion / gradient_magnitude - 1.0)`.
/// Returns 0.0 if `profiles` is empty or no overshoot is found.
pub fn edge_overshoot_score(profiles: &[EdgeProfile]) -> f32 {
    if profiles.is_empty() {
        return 0.0;
    }

    let total_overshoot: f32 = profiles
        .iter()
        .map(|p| {
            let peak = p
                .diff_samples
                .iter()
                .map(|v| v.abs())
                .fold(0.0_f32, f32::max);
            let ratio = peak / p.gradient_magnitude;
            (ratio - 1.0).max(0.0)
        })
        .sum();

    total_overshoot / profiles.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::edges::EdgeProfile;

    fn profile(grad_mag: f32, diff: [f32; 5]) -> EdgeProfile {
        EdgeProfile {
            gradient_magnitude: grad_mag,
            diff_samples: diff,
        }
    }

    #[test]
    fn empty_profiles_returns_zero() {
        assert_eq!(edge_overshoot_score(&[]), 0.0);
    }

    #[test]
    fn no_overshoot_when_diff_below_gradient() {
        // Peak diff = 0.04, gradient = 0.1 -> ratio = 0.4 -> no overshoot.
        let p = profile(0.1, [0.01, 0.02, 0.04, 0.02, 0.01]);
        assert_eq!(edge_overshoot_score(&[p]), 0.0);
    }

    #[test]
    fn exact_match_no_overshoot() {
        // Peak diff = 0.1, gradient = 0.1 -> ratio = 1.0 -> 0 overshoot.
        let p = profile(0.1, [0.01, 0.05, 0.1, 0.05, 0.01]);
        assert_eq!(edge_overshoot_score(&[p]), 0.0);
    }

    #[test]
    fn overshoot_detected() {
        // Peak diff = 0.2, gradient = 0.1 -> ratio = 2.0 -> overshoot = 1.0.
        let p = profile(0.1, [0.01, 0.05, 0.2, 0.05, 0.01]);
        assert!((edge_overshoot_score(&[p]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn negative_diff_counts_as_overshoot() {
        // Peak |diff| = 0.15 (negative), gradient = 0.1 -> ratio = 1.5 -> overshoot = 0.5.
        let p = profile(0.1, [0.01, -0.15, 0.05, 0.02, 0.01]);
        assert!((edge_overshoot_score(&[p]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mixed_profiles_averages_overshoot() {
        let p1 = profile(0.1, [0.01, 0.05, 0.2, 0.05, 0.01]); // overshoot = 1.0
        let p2 = profile(0.1, [0.01, 0.02, 0.04, 0.02, 0.01]); // overshoot = 0.0
        let score = edge_overshoot_score(&[p1, p2]);
        assert!((score - 0.5).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/r3sizer-core/src/metrics/mod.rs`, add:

```rust
mod overshoot;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p r3sizer-core -- metrics::overshoot`

Expected: All 6 overshoot tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/r3sizer-core/src/metrics/overshoot.rs crates/r3sizer-core/src/metrics/mod.rs
git commit -m "feat(metrics): add overshoot.rs with relative peak excursion metric"
```

---

## Task 8: Implement `texture.rs` — texture flattening score

**Files:**
- Create: `crates/r3sizer-core/src/metrics/texture.rs`
- Modify: `crates/r3sizer-core/src/metrics/mod.rs`

- [ ] **Step 1: Write `texture.rs` with implementation and tests**

Create `crates/r3sizer-core/src/metrics/texture.rs`:

```rust
//! Texture flattening metric: detect changes in fine-scale local variance,
//! penalizing both flattening and over-enhancement.
//!
//! Measures local fine-scale energy changes, not guaranteed semantic texture.

/// Default variance threshold for classifying a pixel as textured.
pub const DEFAULT_TEXTURE_THRESHOLD: f32 = 0.001;

/// Window half-size for local variance computation (5x5 window).
const HALF_WIN: usize = 2;

/// Compute the texture flattening score.
///
/// Score = mean of `|log2(var_sharpened / var_original)|` over textured pixels.
/// Returns 0.0 if no textured pixels are found.
pub fn texture_flattening_score(
    luma_original: &[f32],
    luma_sharpened: &[f32],
    width: usize,
    height: usize,
    texture_threshold: f32,
) -> f32 {
    if width < 2 * HALF_WIN + 1 || height < 2 * HALF_WIN + 1 {
        return 0.0;
    }

    let var_orig = local_variance(luma_original, width, height);
    let var_sharp = local_variance(luma_sharpened, width, height);

    let inner_w = width - 2 * HALF_WIN;
    let inner_h = height - 2 * HALF_WIN;

    let mut total_log_ratio = 0.0_f64;
    let mut textured_count = 0u32;

    for iy in 0..inner_h {
        for ix in 0..inner_w {
            let idx = iy * inner_w + ix;
            let vo = var_orig[idx];
            if vo <= texture_threshold {
                continue;
            }
            let vs = var_sharp[idx];
            // Guard against vs == 0 (complete flattening).
            let ratio = if vs < 1e-12 { 1e-12 / vo as f64 } else { vs as f64 / vo as f64 };
            total_log_ratio += ratio.log2().abs();
            textured_count += 1;
        }
    }

    if textured_count == 0 {
        return 0.0;
    }

    (total_log_ratio / textured_count as f64) as f32
}

/// Compute local variance in 5x5 windows.
///
/// Returns a Vec of length `(width - 2*HALF_WIN) * (height - 2*HALF_WIN)`.
fn local_variance(data: &[f32], width: usize, height: usize) -> Vec<f32> {
    let inner_w = width - 2 * HALF_WIN;
    let inner_h = height - 2 * HALF_WIN;
    let win_size = (2 * HALF_WIN + 1) * (2 * HALF_WIN + 1);
    let inv_n = 1.0 / win_size as f32;
    let mut result = Vec::with_capacity(inner_w * inner_h);

    for cy in HALF_WIN..height - HALF_WIN {
        for cx in HALF_WIN..width - HALF_WIN {
            let mut sum = 0.0_f32;
            let mut sum_sq = 0.0_f32;
            for wy in (cy - HALF_WIN)..=(cy + HALF_WIN) {
                for wx in (cx - HALF_WIN)..=(cx + HALF_WIN) {
                    let v = data[wy * width + wx];
                    sum += v;
                    sum_sq += v * v;
                }
            }
            let mean = sum * inv_n;
            let variance = (sum_sq * inv_n - mean * mean).max(0.0);
            result.push(variance);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_score_zero() {
        let luma = vec![0.5_f32; 8 * 8];
        let score = texture_flattening_score(&luma, &luma, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn solid_image_no_textured_pixels() {
        let orig = vec![0.5_f32; 8 * 8];
        let sharp = vec![0.6_f32; 8 * 8];
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        // Solid image has zero local variance -> no textured pixels -> 0.
        assert_eq!(score, 0.0);
    }

    #[test]
    fn image_too_small_returns_zero() {
        let luma = vec![0.5_f32; 4 * 4];
        let score = texture_flattening_score(&luma, &luma, 4, 4, DEFAULT_TEXTURE_THRESHOLD);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn doubled_variance_gives_score_one() {
        // Create a textured region: checkerboard pattern.
        let mut orig = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                if (x + y) % 2 == 0 {
                    orig[y * 8 + x] = 0.6;
                } else {
                    orig[y * 8 + x] = 0.4;
                }
            }
        }
        // Sharpened: double the contrast (doubled variance).
        let mut sharp = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                if (x + y) % 2 == 0 {
                    sharp[y * 8 + x] = 0.7;
                } else {
                    sharp[y * 8 + x] = 0.3;
                }
            }
        }
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        // variance doubles -> ratio = 2 -> |log2(2)| = 1.0.
        // Due to window edge effects, won't be exactly 1.0 but should be close.
        assert!(score > 0.5, "score should be roughly 1.0 for doubled variance: {score}");
    }

    #[test]
    fn flattening_gives_positive_score() {
        // Create textured region.
        let mut orig = vec![0.5_f32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                if (x + y) % 2 == 0 {
                    orig[y * 8 + x] = 0.7;
                } else {
                    orig[y * 8 + x] = 0.3;
                }
            }
        }
        // Sharpened: flatten to near-constant.
        let sharp = vec![0.5_f32; 8 * 8];
        let score = texture_flattening_score(&orig, &sharp, 8, 8, DEFAULT_TEXTURE_THRESHOLD);
        assert!(score > 0.0, "flattening should produce positive score: {score}");
    }

    #[test]
    fn score_is_finite() {
        let mut orig = vec![0.5_f32; 10 * 10];
        for i in 0..100 {
            orig[i] = (i as f32 / 100.0) * 0.8 + 0.1;
        }
        let sharp = orig.iter().map(|&v| v * 1.5).collect::<Vec<_>>();
        let score = texture_flattening_score(&orig, &sharp, 10, 10, DEFAULT_TEXTURE_THRESHOLD);
        assert!(score.is_finite(), "score must be finite: {score}");
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/r3sizer-core/src/metrics/mod.rs`, add:

```rust
mod texture;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p r3sizer-core -- metrics::texture`

Expected: All 6 texture tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/r3sizer-core/src/metrics/texture.rs crates/r3sizer-core/src/metrics/mod.rs
git commit -m "feat(metrics): add texture.rs with local variance ratio flattening metric"
```

---

## Task 9: Implement `composite.rs` and wire all components in `mod.rs`

**Files:**
- Create: `crates/r3sizer-core/src/metrics/composite.rs`
- Modify: `crates/r3sizer-core/src/metrics/mod.rs`

- [ ] **Step 1: Write `composite.rs`**

Create `crates/r3sizer-core/src/metrics/composite.rs`:

```rust
//! Composite metric: weighted sum of individual components.
//!
//! Observation-only in v0.2 — the solver uses GamutExcursion for selection.

use crate::MetricWeights;

/// Compute the weighted composite score.
pub fn weighted_aggregate(
    gamut_excursion: f32,
    halo_ringing: f32,
    edge_overshoot: f32,
    texture_flattening: f32,
    weights: &MetricWeights,
) -> f32 {
    weights.gamut_excursion * gamut_excursion
        + weights.halo_ringing * halo_ringing
        + weights.edge_overshoot * edge_overshoot
        + weights.texture_flattening * texture_flattening
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_gamut_dominant() {
        let w = MetricWeights::default();
        let score = weighted_aggregate(0.1, 0.1, 0.1, 0.1, &w);
        // 1.0*0.1 + 0.3*0.1 + 0.3*0.1 + 0.1*0.1 = 0.17
        assert!((score - 0.17).abs() < 1e-6);
    }

    #[test]
    fn zero_inputs_zero_output() {
        let w = MetricWeights::default();
        assert_eq!(weighted_aggregate(0.0, 0.0, 0.0, 0.0, &w), 0.0);
    }

    #[test]
    fn only_gamut_excursion() {
        let w = MetricWeights {
            gamut_excursion: 1.0,
            halo_ringing: 0.0,
            edge_overshoot: 0.0,
            texture_flattening: 0.0,
        };
        assert!((weighted_aggregate(0.5, 0.3, 0.2, 0.1, &w) - 0.5).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Wire all components into `mod.rs`**

Replace the `compute_metric_breakdown` function in `crates/r3sizer-core/src/metrics/mod.rs`:

```rust
//! Artifact metrics for the auto-sharpness pipeline.
//!
//! v0.2: four components (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening)
//! computed per-probe and at final measurement. The solver uses GamutExcursion for
//! selection; the composite score is diagnostic only.

mod composite;
mod edges;
mod gamut;
mod halo;
mod overshoot;
mod texture;

pub use gamut::{channel_clipping_ratio, pixel_out_of_gamut_ratio};

use std::collections::BTreeMap;

use crate::{ArtifactMetric, MetricBreakdown, MetricComponent, MetricWeights};
use crate::types::LinearRgbImage;

/// Compute the full per-component metric breakdown.
///
/// In v0.2, all four components are populated. The solver uses `selection_score`
/// (GamutExcursion) for fitting; `composite_score` is diagnostic only.
pub fn compute_metric_breakdown(
    sharpened: &LinearRgbImage,
    original: &LinearRgbImage,
    luma_original: &[f32],
    luma_sharpened: &[f32],
    artifact_metric: ArtifactMetric,
    weights: &MetricWeights,
) -> MetricBreakdown {
    let gamut = match artifact_metric {
        ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(sharpened),
        ArtifactMetric::PixelOutOfGamutRatio => pixel_out_of_gamut_ratio(sharpened),
    };

    let w = original.width() as usize;
    let h = original.height() as usize;

    // Shared edge profiling for halo + overshoot.
    let profiles = edges::extract_edge_profiles(
        luma_original,
        luma_sharpened,
        w,
        h,
        edges::DEFAULT_EDGE_THRESHOLD,
    );

    let halo_score = halo::halo_ringing_score(&profiles);
    let overshoot_score = overshoot::edge_overshoot_score(&profiles);
    let texture_score = texture::texture_flattening_score(
        luma_original,
        luma_sharpened,
        w,
        h,
        texture::DEFAULT_TEXTURE_THRESHOLD,
    );

    let composite_score = composite::weighted_aggregate(
        gamut,
        halo_score,
        overshoot_score,
        texture_score,
        weights,
    );

    let mut components = BTreeMap::new();
    components.insert(MetricComponent::GamutExcursion, gamut);
    components.insert(MetricComponent::HaloRinging, halo_score);
    components.insert(MetricComponent::EdgeOvershoot, overshoot_score);
    components.insert(MetricComponent::TextureFlattening, texture_score);

    #[allow(deprecated)]
    MetricBreakdown {
        components,
        selected_metric: MetricComponent::GamutExcursion,
        selection_score: gamut,
        composite_score,
        aggregate: gamut,
    }
}

/// Deprecated alias for [`channel_clipping_ratio`].
#[deprecated(note = "renamed to channel_clipping_ratio")]
pub fn artifact_ratio(img: &LinearRgbImage) -> f32 {
    channel_clipping_ratio(img)
}
```

- [ ] **Step 3: Run all core tests**

Run: `cargo test -p r3sizer-core 2>&1 | tail -30`

Expected: All tests pass (unit + integration).

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -p r3sizer-core -- -D warnings 2>&1 | tail -20`

Expected: No warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/metrics/
git commit -m "feat(metrics): wire all v0.2 components into composite metric breakdown"
```

---

## Task 10: Update CLI — args, output, and run

**Files:**
- Modify: `crates/r3sizer-cli/src/args.rs`
- Modify: `crates/r3sizer-cli/src/output.rs`
- Modify: `crates/r3sizer-cli/src/run.rs`

- [ ] **Step 1: Add `--metric-weights` and `--diagnostics-level` to `args.rs`**

Add two fields to the `Cli` struct, before the sweep section:

```rust
    /// Composite metric weights as W1,W2,W3,W4.
    /// Order: gamut_excursion, halo_ringing, edge_overshoot, texture_flattening.
    #[arg(long, value_delimiter = ',', value_name = "W1,W2,W3,W4")]
    pub metric_weights: Option<Vec<f32>>,

    /// Diagnostics verbosity: "summary" (final breakdown only) or "full" (per-probe breakdowns).
    #[arg(long, default_value = "summary")]
    pub diagnostics_level: DiagnosticsLevelArg,
```

Add the arg wrapper enum after `ArtifactMetricArg`:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum DiagnosticsLevelArg {
    Summary,
    Full,
}

impl From<DiagnosticsLevelArg> for r3sizer_core::DiagnosticsLevel {
    fn from(val: DiagnosticsLevelArg) -> Self {
        match val {
            DiagnosticsLevelArg::Summary => r3sizer_core::DiagnosticsLevel::Summary,
            DiagnosticsLevelArg::Full => r3sizer_core::DiagnosticsLevel::Full,
        }
    }
}
```

- [ ] **Step 2: Update `build_params` in `run.rs`**

Add `MetricWeights` import:

```rust
use r3sizer_core::{AutoSharpParams, ClampPolicy, DiagnosticsLevel, FitStrategy, MetricWeights, ProbeConfig};
```

In `build_params`, add metric weights parsing and add the new fields to `AutoSharpParams`:

```rust
pub fn build_params(args: &Cli, target_width: u32, target_height: u32) -> AutoSharpParams {
    let probe_strengths = if let Some(ref strengths) = args.probe_strengths {
        ProbeConfig::Explicit(strengths.clone())
    } else {
        ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0])
    };

    let metric_weights = if let Some(ref w) = args.metric_weights {
        if w.len() == 4 {
            MetricWeights {
                gamut_excursion: w[0],
                halo_ringing: w[1],
                edge_overshoot: w[2],
                texture_flattening: w[3],
            }
        } else {
            eprintln!("Warning: --metric-weights requires exactly 4 values, using defaults");
            MetricWeights::default()
        }
    } else {
        MetricWeights::default()
    };

    AutoSharpParams {
        target_width,
        target_height,
        probe_strengths,
        target_artifact_ratio: args.target_artifact_ratio,
        enable_contrast_leveling: args.enable_contrast_leveling,
        sharpen_sigma: args.sharpen_sigma,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
        sharpen_mode: args.sharpen_mode.into(),
        sharpen_model: args.sharpen_model.into(),
        metric_mode: args.metric_mode.into(),
        artifact_metric: args.artifact_metric.into(),
        metric_weights,
        diagnostics_level: args.diagnostics_level.into(),
    }
}
```

- [ ] **Step 3: Update `output.rs` with metric breakdown section**

Add the metric breakdown section to `print_summary`. Add after the existing measured artifact ratio line and before fit quality:

```rust
    // Selection and metric breakdown
    if let Some(ref mc) = diag.metric_components {
        println!();
        println!("Selection:");
        println!(
            "  metric                    : {:?}",
            mc.selected_metric
        );
        println!(
            "  selection_score           : {:.6}",
            mc.selection_score
        );
        println!();
        println!("Metric breakdown (v0.2 — diagnostic only):");
        for (component, value) in &mc.components {
            println!("  {:24}: {:.6}", format!("{:?}", component), value);
        }
        println!(
            "  composite_score           : {:.6}  (not used for selection)",
            mc.composite_score
        );
        println!(
            "  weights                   : [{:.1}, {:.1}, {:.1}, {:.1}]  ({})",
            diag.metric_weights.gamut_excursion,
            diag.metric_weights.halo_ringing,
            diag.metric_weights.edge_overshoot,
            diag.metric_weights.texture_flattening,
            provenance_label(diag.metric_weights_provenance),
        );
    }
```

Also add the `MetricWeights` related fields to the import block in `output.rs`. Update the existing import:

```rust
use r3sizer_core::{
    ArtifactMetric, AutoSharpDiagnostics, CrossingStatus, FallbackReason, FitStatus, MetricMode,
    Provenance, SelectionMode, SharpenMode, SharpenModel,
};
```

- [ ] **Step 4: Build the CLI**

Run: `cargo build -p r3sizer-cli 2>&1 | tail -20`

Expected: CLI compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-cli/src/args.rs crates/r3sizer-cli/src/run.rs crates/r3sizer-cli/src/output.rs
git commit -m "feat(cli): add --metric-weights, --diagnostics-level, metric breakdown output"
```

---

## Task 11: Update sweep.rs with per-component stats and percentiles

**Files:**
- Modify: `crates/r3sizer-cli/src/sweep.rs`

- [ ] **Step 1: Add per-component fields to `FileResult`**

Add to `FileResult`:

```rust
    gamut_excursion: f32,
    halo_ringing: f32,
    edge_overshoot: f32,
    texture_flattening: f32,
    composite_score: f32,
```

- [ ] **Step 2: Add per-component aggregate stats**

Add a struct and fields to `AggregateStats`:

```rust
#[derive(Debug, Serialize)]
struct ComponentStats {
    mean: f32,
    median: f32,
    p90: f32,
    p95: f32,
}

// Add to AggregateStats:
    gamut_excursion: ComponentStats,
    halo_ringing: ComponentStats,
    edge_overshoot: ComponentStats,
    texture_flattening: ComponentStats,
    composite_score: ComponentStats,
```

- [ ] **Step 3: Populate FileResult with component scores**

In `process_one`, after extracting `diag`, populate component fields from `metric_components`:

```rust
    let (gamut_excursion, halo_ringing, edge_overshoot, texture_flattening, composite_score) =
        if let Some(ref mc) = diag.metric_components {
            (
                mc.components.get(&r3sizer_core::MetricComponent::GamutExcursion).copied().unwrap_or(0.0),
                mc.components.get(&r3sizer_core::MetricComponent::HaloRinging).copied().unwrap_or(0.0),
                mc.components.get(&r3sizer_core::MetricComponent::EdgeOvershoot).copied().unwrap_or(0.0),
                mc.components.get(&r3sizer_core::MetricComponent::TextureFlattening).copied().unwrap_or(0.0),
                mc.composite_score,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0, 0.0)
        };
```

And add them to the `FileResult` constructor.

- [ ] **Step 4: Implement percentile helper and `compute_component_stats`**

```rust
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f32).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn compute_component_stats(values: &[f32]) -> ComponentStats {
    if values.is_empty() {
        return ComponentStats { mean: 0.0, median: 0.0, p90: 0.0, p95: 0.0 };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = sorted.len();
    let mean = sorted.iter().sum::<f32>() / n as f32;
    let median = if n % 2 == 0 { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 } else { sorted[n / 2] };
    ComponentStats {
        mean,
        median,
        p90: percentile(&sorted, 90.0),
        p95: percentile(&sorted, 95.0),
    }
}
```

- [ ] **Step 5: Wire stats into `compute_aggregate`**

In `compute_aggregate`, compute per-component stats:

```rust
    let gamut_stats = compute_component_stats(&results.iter().map(|r| r.gamut_excursion).collect::<Vec<_>>());
    let halo_stats = compute_component_stats(&results.iter().map(|r| r.halo_ringing).collect::<Vec<_>>());
    let overshoot_stats = compute_component_stats(&results.iter().map(|r| r.edge_overshoot).collect::<Vec<_>>());
    let texture_stats = compute_component_stats(&results.iter().map(|r| r.texture_flattening).collect::<Vec<_>>());
    let composite_stats = compute_component_stats(&results.iter().map(|r| r.composite_score).collect::<Vec<_>>());
```

And add them to the `AggregateStats` struct construction.

- [ ] **Step 6: Build and verify**

Run: `cargo build -p r3sizer-cli 2>&1 | tail -20`

Expected: Compiles successfully.

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-cli/src/sweep.rs
git commit -m "feat(cli): add per-component sweep stats with p90/p95 percentiles"
```

---

## Task 12: Run full test suite, clippy, and final verification

**Files:** None (verification only)

- [ ] **Step 1: Run all tests**

Run: `cargo test --workspace 2>&1 | tail -40`

Expected: All tests pass across all crates.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -20`

Expected: No warnings.

- [ ] **Step 3: Build all targets**

Run: `cargo build --workspace 2>&1 | tail -10`

Expected: Clean build.

- [ ] **Step 4: Commit any final fixes**

If any fixes were needed, commit them:

```bash
git add -A && git commit -m "fix: address clippy and test issues from v0.2 metrics implementation"
```
